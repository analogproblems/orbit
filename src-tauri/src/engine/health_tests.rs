use super::*;
use crate::health::{Disk, HealthKind};
use chrono::Duration as Span;

/// Orbit tells how long Codex has been disconnected itself, and saves only
/// when what's wrong changes.
#[tokio::test]
async fn orbit_sees_what_is_wrong_with_itself_and_saves_only_changes() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    e.update(|s| {
        s.codex_ready = false;
        s.settings
            .repositories
            .push(Repository::new("a/b", "/tmp/a-b"));
    })
    .await;
    let roomy = Some(Disk {
        free: 100 << 30,
        total: 200 << 30,
    });
    let t0 = chrono::Utc::now();
    e.dirty.store(false, Ordering::SeqCst);
    e.see_health(roomy, t0).await;
    // Codex has only just been seen down.
    assert!(e.snapshot().await.health.is_empty());
    assert!(!e.dirty.load(Ordering::SeqCst));
    e.see_health(roomy, t0 + Span::minutes(6)).await;
    let health = e.snapshot().await.health;
    assert_eq!(health.len(), 1);
    assert_eq!(health[0].kind, HealthKind::Codex);
    assert!(e.dirty.swap(false, Ordering::SeqCst));
    // Nothing new, nothing to save.
    e.see_health(roomy, t0 + Span::minutes(7)).await;
    assert!(!e.dirty.load(Ordering::SeqCst));
    // A low disk is in Needs you; Codex has its own item there.
    let low = Some(Disk {
        free: 1 << 30,
        total: 200 << 30,
    });
    e.see_health(low, t0 + Span::minutes(8)).await;
    let shown = crate::attention::shown(&e.snapshot().await);
    let health: Vec<_> = shown
        .attention
        .iter()
        .filter(|a| a.kind == AttentionKind::Health)
        .collect();
    assert_eq!(health.len(), 1);
    assert!(health[0].reason.starts_with("Only 1.0 GB"));
    // Codex back: its issue closes, and a new disconnect starts the clock
    // again.
    e.update(|s| s.codex_ready = true).await;
    e.see_health(roomy, t0 + Span::minutes(9)).await;
    assert!(e.snapshot().await.health.iter().all(|i| !i.is_open()));
    e.update(|s| s.codex_ready = false).await;
    e.see_health(roomy, t0 + Span::minutes(10)).await;
    e.see_health(roomy, t0 + Span::minutes(12)).await;
    assert!(e.snapshot().await.health.iter().all(|i| !i.is_open()));
}

/// A full disk is one reason state can't be saved, so Orbit still checks
/// its own health then, once a minute.
#[tokio::test]
async fn orbit_checks_its_health_while_state_cant_be_saved() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    e.update(|s| {
        let pr = crate::types::tests::pr();
        let mut r = Review::new(&pr, &s.settings, false);
        r.worktree = Some(d.path().join("worktrees/1").display().to_string());
        r.cleanup_error = Some("Directory not empty".into());
        s.reviews.push(r);
    })
    .await;
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e.flush().await.is_err());
    e.housekeeping().await;
    let open = || async {
        e.snapshot()
            .await
            .health
            .iter()
            .filter(|i| i.is_open())
            .map(|i| i.kind)
            .collect::<Vec<_>>()
    };
    for _ in 0..200 {
        if !open().await.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(open().await.contains(&HealthKind::Leftovers));
    // Not again within the minute.
    e.update(|s| s.health.clear()).await;
    e.housekeeping().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(open().await.is_empty());
}
