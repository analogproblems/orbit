use super::{test_support::*, Engine};
use crate::types::*;
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};

#[tokio::test]
async fn refuses_duplicate_instances_and_recovers_interrupted_runs() {
    let (d, e, _) = running_engine().await;
    assert!(Engine::new(d.path().into(), |_| {}).is_err());
    e.flush().await.unwrap();
    drop(e);
    let restored = Engine::new(d.path().into(), |_| {}).unwrap();
    assert_eq!(restored.snapshot().await.reviews[0].status, "cancelled");
}
#[tokio::test]
async fn streamed_text_is_shown_but_persisted_only_when_its_item_completes() {
    let d = tempfile::tempdir().unwrap();
    let emits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = emits.clone();
    let e = Engine::new(d.path().into(), move |_: &Snapshot| {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    let mut review = Review::new(&crate::types::tests::pr(), &Settings::default(), false);
    review.status = "reviewing".into();
    review.thread_id = Some("thread-1".into());
    review.turn_id = Some("turn-1".into());
    e.update(|s| s.reviews.push(review)).await;
    e.flush().await.unwrap();
    let saved = || std::fs::read_to_string(d.path().join("state.json")).unwrap();
    let shown = emits.load(Ordering::SeqCst);

    e.event(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"m","delta":"Streaming"}})).await;
    e.flush().await.unwrap();
    assert_eq!(emits.load(Ordering::SeqCst), shown + 1);
    assert!(!saved().contains("Streaming"));

    // Notifications Orbit does not use neither persist nor re-render.
    e.event(json!({"method":"item/reasoning/textDelta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"r","delta":"..."}})).await;
    e.flush().await.unwrap();
    assert_eq!(emits.load(Ordering::SeqCst), shown + 1);

    e.event(json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"m","type":"agentMessage","text":"Streaming done"}}})).await;
    e.flush().await.unwrap();
    assert!(saved().contains("Streaming done"));
}
#[tokio::test]
async fn merges_case_duplicate_repositories_saved_by_older_versions() {
    let d = tempfile::tempdir().unwrap();
    let repo = |name: &str, automatic: bool| Repository {
        name: name.into(),
        path: "/tmp/checkout".into(),
        automatic,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
    };
    let mut upper = crate::types::tests::pr();
    upper.id = "A/B#1".into();
    let mut lower = upper.clone();
    lower.id = "a/b#1".into();
    let mut saved = Snapshot {
        prs: vec![upper, lower],
        ..Snapshot::default()
    };
    saved.settings.repositories = vec![repo("a/b", false), repo("A/B", true), repo("c/d", false)];
    std::fs::write(
        d.path().join("state.json"),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    let s = Engine::new(d.path().into(), |_| {})
        .unwrap()
        .snapshot()
        .await;
    let names: Vec<_> = s
        .settings
        .repositories
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["a/b", "c/d"]);
    assert!(s.settings.repositories[0].automatic);
    assert_eq!(s.prs.len(), 1);
}
#[tokio::test]
async fn pr_ids_ignore_repository_capitalization() {
    let d = tempfile::tempdir().unwrap();
    let mut open = crate::types::tests::pr();
    open.id = "A/B#1".into();
    open.repo = "A/B".into();
    let mut closed = open.clone();
    closed.id = "A/B#2".into();
    closed.number = 2;
    let mut closed_lower = closed.clone();
    closed_lower.id = "a/b#2".into();
    let mut open_lower = open.clone();
    open_lower.id = "a/b#1".into();
    let mut review = Review::new(&open, &Settings::default(), false);
    review.pr_id = "A/B#1".into();
    let saved = Snapshot {
        prs: vec![open],
        history_prs: vec![open_lower, closed, closed_lower],
        reviews: vec![review],
        ..Snapshot::default()
    };
    std::fs::write(
        d.path().join("state.json"),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    let s = Engine::new(d.path().into(), |_| {})
        .unwrap()
        .snapshot()
        .await;
    assert_eq!(s.prs[0].id, "a/b#1");
    assert_eq!(s.reviews[0].pr_id, "a/b#1");
    let history: Vec<_> = s.history_prs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(history, ["a/b#2"]);
    assert_eq!(pr_id("A/B", 1), pr_id("a/b", 1));
}
#[tokio::test]
async fn persistence_survives_restart() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    e.update(|s| s.settings.poll_seconds = 240).await;
    e.flush().await.unwrap();
    drop(e);
    let restored = Engine::new(d.path().into(), |_| {}).unwrap();
    assert_eq!(restored.snapshot().await.settings.poll_seconds, 240);
}
#[test]
fn corrupt_state_is_never_overwritten() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("state.json"), "broken").unwrap();
    assert!(Engine::new(d.path().into(), |_| {}).is_err());
    assert_eq!(
        std::fs::read_to_string(d.path().join("state.json")).unwrap(),
        "broken"
    );
}
#[tokio::test]
async fn a_failed_save_pauses_autopilot_until_it_is_turned_back_on() {
    let d = tempfile::tempdir().unwrap();
    let e = super::Engine::new(d.path().into(), |_| {}).unwrap();
    let mut settings = e.snapshot().await.settings;
    settings.automatic = true;
    e.save_settings(settings.clone()).await.unwrap();
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    e.update(|s| s.github_user = Some("me".into())).await;
    assert!(e.flush().await.is_err());
    let s = e.snapshot().await;
    assert!(!s.settings.automatic);
    assert!(s.state_write_error.is_some());
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.attention.len(), 1);
    assert_eq!(shown.attention[0].kind, AttentionKind::AutopilotPaused);
    std::fs::remove_dir(d.path().join("state.json.tmp")).unwrap();
    e.save_settings(settings).await.unwrap();
    assert!(e.snapshot().await.state_write_error.is_none());
    assert!(e.shown_snapshot().await.attention.is_empty());
}
