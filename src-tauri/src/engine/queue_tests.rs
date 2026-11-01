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
async fn an_automatic_review_that_cant_start_says_why_and_waits_to_try_again() {
    let (d, e) = autopilot_engine(trusted_pr()).await;
    e.synced_repos.lock().await.insert("a/b".into());
    let checkout = d.path().to_string_lossy().into_owned();
    e.update(|s| s.settings.repositories[0].path = d.path().join("gone").display().to_string())
        .await;
    e.schedule().await;
    let s = e.snapshot().await;
    assert!(s.reviews.is_empty());
    let pr = &s.prs[0];
    let item = crate::attention::items(&s)
        .into_iter()
        .find(|a| a.key == format!("unqueued:{}:{}", pr.id, pr.head_sha))
        .expect("the failure needs you");
    assert!(
        item.reason.contains("local checkout no longer exists"),
        "{}",
        item.reason
    );
    // Fixed now, but not tried again on every tick.
    e.update(|s| s.settings.repositories[0].path = checkout)
        .await;
    e.schedule().await;
    assert!(e.snapshot().await.reviews.is_empty());
    assert_eq!(e.snapshot().await.queue_failures.len(), 1);
    // Once it's time, it is queued and the item goes.
    e.queue_retry.lock().unwrap().clear();
    e.schedule().await;
    let s = e.snapshot().await;
    assert_eq!(s.reviews.len(), 1);
    assert!(s.queue_failures.is_empty());
    assert!(!crate::attention::items(&s)
        .iter()
        .any(|a| a.key.starts_with("unqueued:")));
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
/// Where Orbit stepped back for another Orbit, a review queued before
/// waits, and nothing new is queued.
#[tokio::test]
async fn nothing_is_reviewed_where_another_orbit_posts() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    let queued = automatic(&e, &trusted_pr().head_sha, "queued").await;
    let posting = crate::orbits::Posting {
        pr_id: trusted_pr().id,
        other: Some(crate::orbits::Post {
            url: "u".into(),
            at: "2026-10-31T09:00:00Z".into(),
            by: "f".repeat(32),
        }),
        ..Default::default()
    };
    e.update(|s| s.posting.push(posting)).await;
    e.synced_repos.lock().await.insert("a/b".into());
    e.schedule().await;
    let s = e.snapshot().await;
    assert_eq!(s.reviews.len(), 1);
    assert_eq!(
        (s.reviews[0].id.as_str(), s.reviews[0].status.as_str()),
        (queued.as_str(), "queued")
    );
    e.update(|s| s.reviews.clear()).await;
    e.schedule().await;
    assert!(e.snapshot().await.reviews.is_empty());
}
#[tokio::test]
async fn a_queued_run_stops_when_its_pr_closes() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    let queued = automatic(&e, &trusted_pr().head_sha, "queued").await;
    e.supersede_outdated_runs().await;
    assert_eq!(e.snapshot().await.reviews[0].status, "queued");
    e.update(|s| {
        let pr = s.prs.remove(0);
        s.history_prs.push(pr);
    })
    .await;
    e.supersede_outdated_runs().await;
    let s = e.snapshot().await;
    let r = s.reviews.iter().find(|r| r.id == queued).unwrap();
    assert_eq!(r.status, "cancelled");
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
type Sent = Arc<Mutex<Vec<(String, String, String)>>>;
/// Records each status sent; `gate`, while set, holds requests until it opens.
fn recording_sender(
    e: &super::Engine,
    refuse: Arc<std::sync::atomic::AtomicBool>,
    gate: Arc<tokio::sync::RwLock<()>>,
) -> Sent {
    let sent: Sent = Arc::new(Mutex::new(vec![]));
    let log = sent.clone();
    e.set_status_sender(Arc::new(move |_repo, sha, status: CommitStatus| {
        log.lock()
            .unwrap()
            .push((sha, status.state, status.description));
        let fail = refuse.load(std::sync::atomic::Ordering::SeqCst);
        let gate = gate.clone();
        Box::pin(async move {
            let _open = gate.read().await;
            if fail {
                Err("Resource not accessible by integration".into())
            } else {
                Ok(())
            }
        })
    }));
    sent
}
async fn sync(e: &Arc<super::Engine>) {
    e.sync_commit_statuses().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}
fn states(sent: &Sent) -> Vec<String> {
    sent.lock()
        .unwrap()
        .iter()
        .map(|(_, s, _)| s.clone())
        .collect()
}
#[tokio::test]
async fn sends_each_commit_status_once_and_records_refusals() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.auto_post = true).await;
    let refuse = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sent = recording_sender(&e, refuse.clone(), Default::default());
    let id = automatic(&e, &trusted_pr().head_sha, "reviewing").await;
    sync(&e).await;
    sync(&e).await;
    assert_eq!(
        *sent.lock().unwrap(),
        [(
            trusted_pr().head_sha,
            "pending".to_string(),
            "Round 1 · reviewing".to_string()
        )]
    );
    // A refused status is recorded, not retried, and leaves the review as it was.
    refuse.store(true, std::sync::atomic::Ordering::SeqCst);
    e.update(|s| {
        let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
        r.status = "posted".into();
        r.report = "Report".into();
        r.findings = Some(vec![]);
        crate::review_loop::decide(r, Some(1));
    })
    .await;
    sync(&e).await;
    sync(&e).await;
    assert_eq!(states(&sent), ["pending", "success"]);
    let s = e.snapshot().await;
    assert_eq!(s.reviews[0].status, "posted");
    let check = &s.commit_checks[0];
    assert_eq!(check.accepted.as_ref().unwrap().state, "pending");
    let refused = check.refused.as_ref().unwrap();
    assert_eq!(refused.state, "success");
    assert_eq!(
        refused.error.as_deref(),
        Some("Resource not accessible by integration")
    );
}

/// An Orbit moving waits for a commit status on its way, and sends no new
/// one once it has stopped, so none lands after it's packed.
#[tokio::test]
async fn a_moving_orbit_sends_the_status_on_its_way_and_no_more() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.auto_post = true).await;
    let gate: Arc<tokio::sync::RwLock<()>> = Default::default();
    let held = gate.clone().write_owned().await;
    let sent = recording_sender(&e, Default::default(), gate);
    let id = automatic(&e, &trusted_pr().head_sha, "reviewing").await;
    sync(&e).await;
    assert_eq!(states(&sent), ["pending"]);
    e.shutdown().await;
    let fencing = tokio::spawn({
        let e = e.clone();
        async move { e.fence().await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!fencing.is_finished(), "waits for the status on its way");
    drop(held);
    fencing.await.unwrap().unwrap();
    e.update(|s| {
        let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
        r.status = "posted".into();
        r.report = "Report".into();
        r.findings = Some(vec![]);
        crate::review_loop::decide(r, Some(1));
    })
    .await;
    sync(&e).await;
    assert_eq!(states(&sent), ["pending"]);
}
#[tokio::test]
async fn a_refused_update_still_lets_a_pending_check_close() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.auto_post = true).await;
    let refuse = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sent = recording_sender(&e, refuse.clone(), Default::default());
    let old = trusted_pr().head_sha;
    let id = automatic(&e, &old, "reviewing").await;
    sync(&e).await;
    // An approval makes the check an error, but GitHub refuses that update.
    refuse.store(true, std::sync::atomic::Ordering::SeqCst);
    e.update(|s| {
        s.approvals.push(Approval {
            id: 1.into(),
            review_id: id.clone(),
            method: "item/commandExecution/requestApproval".into(),
            params: serde_json::Value::Null,
            requested_at: String::new(),
        })
    })
    .await;
    sync(&e).await;
    // GitHub recovers, and a push replaces the round.
    refuse.store(false, std::sync::atomic::Ordering::SeqCst);
    e.update(|s| s.prs[0].head_sha = "d".repeat(40)).await;
    e.supersede_outdated_runs().await;
    sync(&e).await;
    let sent = sent.lock().unwrap().clone();
    assert_eq!(
        sent.last().unwrap(),
        &(old, "error".into(), "Replaced by newer commits".into())
    );
}
#[tokio::test]
async fn a_slow_status_for_a_stopped_round_cannot_overwrite_a_newer_one() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.auto_post = true).await;
    let gate: Arc<tokio::sync::RwLock<()>> = Default::default();
    let sent = recording_sender(&e, Default::default(), gate.clone());
    let head = trusted_pr().head_sha;
    let stopped = automatic(&e, &head, "queued").await;
    sync(&e).await;
    // The round is stopped; closing its check is slow to reach GitHub.
    let hold = gate.write().await;
    e.update(|s| {
        s.reviews
            .iter_mut()
            .find(|r| r.id == stopped)
            .unwrap()
            .status = "cancelled".into()
    })
    .await;
    sync(&e).await;
    // Continue loop: a newer round on the same commit finishes clean.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let newer = automatic(&e, &head, "posted").await;
    e.update(|s| {
        let r = s.reviews.iter_mut().find(|r| r.id == newer).unwrap();
        r.findings = Some(vec![]);
        crate::review_loop::decide(r, Some(1));
    })
    .await;
    sync(&e).await;
    assert_eq!(states(&sent), ["pending", "error"]);
    drop(hold);
    sync(&e).await;
    sync(&e).await;
    assert_eq!(states(&sent), ["pending", "error", "success"]);
    let s = e.snapshot().await;
    assert_eq!(
        s.commit_checks[0].accepted.as_ref().unwrap().state,
        "success"
    );
}
#[tokio::test]
async fn statuses_update_while_codex_is_disconnected() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| {
        s.settings.auto_post = true;
        s.codex_ready = false;
    })
    .await;
    let sent = recording_sender(&e, Default::default(), Default::default());
    automatic(&e, &trusted_pr().head_sha, "failed").await;
    e.tick(false).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(states(&sent), ["error"]);
}
#[tokio::test]
async fn a_round_that_finishes_before_a_push_still_closes_its_check() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.auto_post = true).await;
    let sent = recording_sender(&e, Default::default(), Default::default());
    let old = trusted_pr().head_sha;
    let id = automatic(&e, &old, "reviewing").await;
    sync(&e).await;
    // It finishes, and the next poll finds a new head, before the next sync.
    e.update(|s| {
        let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
        r.status = "ready".into();
        r.findings = Some(vec![]);
        crate::review_loop::decide(r, Some(1));
        s.prs[0].head_sha = "d".repeat(40);
    })
    .await;
    sync(&e).await;
    let sent = sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        sent[1],
        (
            old,
            "success".into(),
            "Round 1 · no blocking findings".into()
        )
    );
}
#[tokio::test]
async fn a_refusal_is_forgotten_once_the_wanted_status_moves_on() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| s.settings.auto_post = true).await;
    let refuse = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sent = recording_sender(&e, refuse.clone(), Default::default());
    let id = automatic(&e, &trusted_pr().head_sha, "reviewing").await;
    let approval = |id: String| Approval {
        id: 1.into(),
        review_id: id,
        method: "item/commandExecution/requestApproval".into(),
        params: serde_json::Value::Null,
        requested_at: String::new(),
    };
    sync(&e).await;
    // An approval's error status is refused while GitHub is failing.
    refuse.store(true, std::sync::atomic::Ordering::SeqCst);
    e.update(|s| s.approvals.push(approval(id.clone()))).await;
    sync(&e).await;
    // GitHub recovers and the approval is answered: pending is already shown.
    refuse.store(false, std::sync::atomic::Ordering::SeqCst);
    e.update(|s| s.approvals.clear()).await;
    sync(&e).await;
    assert!(e.snapshot().await.commit_checks[0].refused.is_none());
    // A second approval in the same round is sent again.
    e.update(|s| s.approvals.push(approval(id.clone()))).await;
    sync(&e).await;
    assert_eq!(states(&sent), ["pending", "error", "error"]);
    let s = e.snapshot().await;
    assert_eq!(s.commit_checks[0].accepted.as_ref().unwrap().state, "error");
}
/// Polls that fail before any repository syncs, here because GitHub rejects
/// the credential, are counted and reset by the next good poll.
#[cfg(unix)]
#[tokio::test]
async fn counts_polls_that_fail_before_any_repository_syncs() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let e = super::Engine::new(d.path().join("state"), |_| {}).unwrap();
    let gh = d.path().join("gh");
    let script = format!(
        "#!/bin/sh\nif [ -e {0}/ok ]; then case \"$*\" in *user*) echo '{{\"login\":\"me\"}}' ;; *) echo '[]' ;; esac; exit 0; fi\necho 'HTTP 401: Bad credentials' >&2\nexit 1\n",
        d.path().display()
    );
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..3 {
        let _ = crate::github::GH.scope(gh.clone(), e.poll()).await;
    }
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.poll_failures, 3);
    assert_eq!(shown.attention.len(), 1);
    assert_eq!(shown.attention[0].key, "sync:github");
    assert!(shown.attention[0].reason.contains("Bad credentials"));
    std::fs::write(d.path().join("ok"), "").unwrap();
    crate::github::GH.scope(gh, e.poll()).await.unwrap();
    assert_eq!(e.snapshot().await.poll_failures, 0);
    assert!(e.shown_snapshot().await.attention.is_empty());
}
