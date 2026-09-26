use super::queue::reuse_stats;
use super::test_support::*;

#[test]
fn reuses_size_statistics_only_for_unchanged_prs() {
    let mut known = crate::types::tests::pr();
    known.additions = 12;
    known.changed_files = 3;
    let mut listed = known.clone();
    listed.additions = 0;
    listed.changed_files = 0;
    assert!(reuse_stats(&mut listed, std::slice::from_ref(&known)));
    assert_eq!((listed.additions, listed.changed_files), (12, 3));
    let mut updated = listed.clone();
    updated.updated_at = "later".into();
    assert!(!reuse_stats(&mut updated, &[known]));
}
#[tokio::test]
async fn autopilot_reviews_trusted_prs_in_synced_repositories_once() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.schedule().await;
    assert!(
        e.snapshot().await.reviews.is_empty(),
        "nothing is scheduled before the repository syncs"
    );
    e.synced_repos.lock().await.insert("a/b".into());
    e.schedule().await;
    e.schedule().await;
    let reviews = e.snapshot().await.reviews;
    assert_eq!(reviews.len(), 1);
    assert!(reviews[0].automatic);
}
#[tokio::test]
async fn autopilot_skips_untrusted_prs() {
    let (_d, e) = autopilot_engine(crate::types::tests::pr()).await;
    e.synced_repos.lock().await.insert("a/b".into());
    e.schedule().await;
    assert!(e.snapshot().await.reviews.is_empty());
}
use crate::types::*;
use std::sync::{Arc, Mutex};

/// An automatic review of `head` in `status`.
async fn automatic(e: &super::Engine, head: &str, status: &str) -> String {
    let mut r = Review::new(&trusted_pr(), &Settings::default(), true);
    r.head_sha = head.into();
    r.status = status.into();
    if status == "reviewing" {
        r.thread_id = Some("thread-1".into());
        r.turn_id = Some("turn-1".into());
    }
    let id = r.id.clone();
    e.update(|s| s.reviews.insert(0, r)).await;
    id
}
#[tokio::test]
async fn a_new_head_replaces_queued_runs_and_supersedes_running_ones() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    let old = "c".repeat(40);
    let queued = automatic(&e, &old, "queued").await;
    let running = automatic(&e, &old, "reviewing").await;
    let current = automatic(&e, &trusted_pr().head_sha, "queued").await;
    e.supersede_outdated_runs().await;
    let s = e.snapshot().await;
    let find = |id: &str| s.reviews.iter().find(|r| r.id == id).unwrap().clone();
    assert_eq!(find(&queued).status, "cancelled");
    assert!(find(&queued).superseded);
    assert_eq!(find(&running).status, "reviewing");
    assert!(find(&running).superseded);
    assert!(!find(&current).superseded && find(&current).status == "queued");

    // Once the superseded run ends, the new head is queued.
    e.update(|s| {
        s.reviews.retain(|r| r.id != current);
        let r = s.reviews.iter_mut().find(|r| r.id == running).unwrap();
        r.status = "ready".into();
    })
    .await;
    e.synced_repos.lock().await.insert("a/b".into());
    e.schedule().await;
    let s = e.snapshot().await;
    let newest = &s.reviews[0];
    assert!(newest.automatic && newest.head_sha == trusted_pr().head_sha);
    assert_eq!(s.reviews.len(), 3);
}
#[tokio::test]
async fn a_follow_up_on_a_finished_round_keeps_its_round() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.repositories[0].round_limit = 1)
        .await;
    let old = "c".repeat(40);
    let id = automatic(&e, &old, "posted").await;
    let pr = trusted_pr();
    let allows = |s: &Snapshot| crate::review_loop::allows_new_round(&pr, s);
    assert!(!allows(&e.snapshot().await));
    // A follow-up chat makes the finished round active again for a while.
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.before_follow_up = Some(Outcome {
            status: r.status.clone(),
            error: None,
        });
        r.status = "reviewing".into();
    })
    .await;
    e.supersede_outdated_runs().await;
    e.update(|s| {
        let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
        r.status = r.before_follow_up.take().unwrap().status;
    })
    .await;
    let s = e.snapshot().await;
    assert!(!s.reviews[0].superseded);
    assert!(!allows(&s));
}
#[tokio::test]
async fn an_unchanged_scheduling_pass_does_not_save_state() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.automatic = false).await;
    e.flush().await.unwrap();
    e.schedule().await;
    assert!(!e.dirty.load(std::sync::atomic::Ordering::SeqCst));
    // Superseding a run does need saving.
    automatic(&e, &"c".repeat(40), "reviewing").await;
    e.flush().await.unwrap();
    e.supersede_outdated_runs().await;
    assert!(e.dirty.load(std::sync::atomic::Ordering::SeqCst));
}
#[tokio::test]
async fn superseded_runs_are_not_published_automatically() {
    let (_d, e, id) = running_engine().await;
    ready_for_autopost(&e, "Report", true).await;
    e.update(|s| s.reviews[0].superseded = true).await;
    e.autopost(&id);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "ready");
    assert!(r.error.is_none() && r.comment_url.is_none());
}
#[tokio::test]
async fn stop_and_continue_control_the_loop_for_one_pr() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    let pr_id = trusted_pr().id;
    let queued = automatic(&e, "c".repeat(40).as_str(), "queued").await;
    e.stop_loop(&pr_id).await.unwrap();
    let s = e.snapshot().await;
    assert!(s.loops[0].stopped);
    assert_eq!(s.reviews[0].id, queued);
    assert_eq!(s.reviews[0].status, "cancelled");
    e.synced_repos.lock().await.insert("a/b".into());
    e.schedule().await;
    assert_eq!(
        e.snapshot().await.reviews.len(),
        1,
        "a stopped loop queues nothing"
    );

    assert!(e.continue_loop(&pr_id, 0).await.is_err());
    e.continue_loop(&pr_id, 2).await.unwrap();
    let s = e.snapshot().await;
    assert!(!s.loops[0].stopped);
    // Nothing was used (the cancelled run is not a round), so no rounds are added.
    assert_eq!(s.loops[0].extra_rounds, 0);
    e.schedule().await;
    assert_eq!(e.snapshot().await.reviews.len(), 2);
}
#[tokio::test]
async fn continuing_at_the_round_limit_grants_more_rounds() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    let pr_id = trusted_pr().id;
    e.update(|s| s.settings.repositories[0].round_limit = 1)
        .await;
    automatic(&e, "c".repeat(40).as_str(), "posted").await;
    e.synced_repos.lock().await.insert("a/b".into());
    e.schedule().await;
    assert_eq!(e.snapshot().await.reviews.len(), 1, "limit reached");
    e.continue_loop(&pr_id, 1).await.unwrap();
    assert_eq!(e.snapshot().await.loops[0].extra_rounds, 1);
    e.schedule().await;
    assert_eq!(e.snapshot().await.reviews.len(), 2);
    // Manual reviews always work at the limit.
    e.update(|s| s.reviews.retain(|r| r.status == "posted"))
        .await;
    e.update(|s| s.loops.clear()).await;
    assert!(e.enqueue(&pr_id, false).await.is_ok());
}
#[tokio::test]
async fn emitted_snapshots_carry_loop_states_that_are_never_saved() {
    let d = tempfile::tempdir().unwrap();
    let seen: Arc<Mutex<Option<Snapshot>>> = Arc::default();
    let sink = seen.clone();
    let e = super::Engine::new(d.path().into(), move |s| {
        *sink.lock().unwrap() = Some(s.clone())
    })
    .unwrap();
    let pr = trusted_pr();
    let mut r = Review::new(&pr, &Settings::default(), true);
    r.status = "reviewing".into();
    e.update(|s| {
        s.prs.push(pr.clone());
        s.reviews.push(r);
    })
    .await;
    e.flush().await.unwrap();
    let shown = seen.lock().unwrap().clone().unwrap();
    assert_eq!(shown.loop_states.len(), 1);
    assert_eq!(shown.loop_states[0].status, LoopStatus::Reviewing);
    assert!(e.snapshot().await.loop_states.is_empty());
    let saved = std::fs::read_to_string(d.path().join("state.json")).unwrap();
    assert!(!saved.contains("loopStates"));
}
