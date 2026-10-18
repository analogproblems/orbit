use super::events::ReviewEvent;
use super::test_support::*;
use crate::types::*;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn captures_streamed_report_and_finishes_review() {
    let (_d, e, _) = running_engine().await;
    e.event(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"m","delta":"Checking"}})).await;
    e.event(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"m","delta":" tests"}})).await;
    e.event(json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"review","type":"exitedReviewMode","review":"No actionable defects found."}}})).await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}})).await;
    // The report is followed by findings extraction, which cannot start here
    // without Codex, so the run finishes unstructured with its report kept.
    let r = wait_for_status(&e, "ready").await;
    assert_eq!(r.messages[0].text, "Checking tests");
    assert_eq!(r.report, "No actionable defects found.");
    assert!(!r.extracting && r.findings.is_none());
    let error = r.error.unwrap();
    assert!(
        error.contains("unstructured") && error.contains("Codex is disconnected"),
        "{error}"
    );
}
#[tokio::test]
async fn rejects_completion_without_a_review_report() {
    let (_d, e, _) = running_engine().await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}})).await;
    assert_eq!(e.snapshot().await.reviews[0].status, "failed");
}
#[tokio::test]
async fn ignores_stale_turn_completion_and_cancelled_streams() {
    let (_d, e, id) = running_engine().await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"old-turn","status":"failed"}}})).await;
    assert_eq!(e.snapshot().await.reviews[0].status, "reviewing");
    e.cancel(&id).await.unwrap();
    e.event(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"late","delta":"Late result"}})).await;
    let s = e.snapshot().await;
    assert_eq!(s.reviews[0].status, "cancelled");
    assert!(s.reviews[0].messages.is_empty());
}
#[tokio::test]
async fn approval_includes_actual_file_changes() {
    let (_d, e, _) = running_engine().await;
    let item = json!({"id":"file-1","type":"fileChange","changes":[{"path":"/tmp/source.rs","diff":"-old\n+new"}]});
    e.event(json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","item":item}})).await;
    e.event(json!({"id":71,"method":"item/fileChange/requestApproval","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"file-1"}})).await;
    let s = e.snapshot().await;
    assert_eq!(s.reviews[0].status, "awaitingApproval");
    assert_eq!(
        s.approvals[0].params["itemDetails"]["changes"][0]["diff"],
        "-old\n+new"
    );
}
#[tokio::test]
async fn records_credential_kinds_when_the_report_arrives() {
    let (_d, e, _) = running_engine().await;
    let review = format!("Leaked -----BEGIN {} KEY----- in docs.", "PRIVATE");
    e.event(json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"review","type":"exitedReviewMode","review":review}}})).await;
    assert_eq!(e.snapshot().await.reviews[0].sensitive, ["private key"]);
}
#[tokio::test]
async fn stopping_a_review_forgets_its_in_progress_items() {
    let (_d, e, id) = running_engine().await;
    e.event(json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"cmd","type":"commandExecution","command":"ls"}}})).await;
    assert_eq!(e.items.lock().await.len(), 1);
    e.cancel(&id).await.unwrap();
    assert!(e.items.lock().await.is_empty());
}
#[tokio::test]
async fn failed_preparation_refuses_follow_ups_while_removal_waits() {
    let (_d, e, id) = running_engine().await;
    let path = e.root.join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| s.reviews[0].worktree = Some(path.to_string_lossy().into()))
        .await;
    // Another review's long fetch holds the git lock.
    let fetch = e.git_lock.lock().await;
    let abandon = tokio::spawn({
        let e = e.clone();
        let id = id.clone();
        async move {
            e.abandon_preparation(&id, Some("review/start failed".into()))
                .await
        }
    });
    for _ in 0..100 {
        if e.snapshot().await.reviews[0].status == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "failed");
    assert!(r.thread_id.is_none());
    // Still recorded, so exiting now leaves the removal for a later cleanup.
    assert!(r.worktree.is_some());
    let error = e.message(&id, "What went wrong?").await.unwrap_err();
    assert!(error.contains("being removed"), "{error}");
    drop(fetch);
    abandon.await.unwrap();
    assert!(!path.exists());
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "failed");
    assert!(r.worktree.is_none());
}
#[tokio::test]
async fn stopping_a_preparing_review_aborts_its_task() {
    let (_d, e, id) = running_engine().await;
    e.update(|s| {
        s.reviews[0].status = "preparing".into();
        s.reviews[0].thread_id = None;
        s.reviews[0].turn_id = None;
    })
    .await;
    let task = tokio::spawn(tokio::time::sleep(Duration::from_secs(60)));
    e.preparing
        .lock()
        .unwrap()
        .insert(id.clone(), task.abort_handle());
    e.cancel(&id).await.unwrap();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(e.snapshot().await.reviews[0].status, "cancelled");
}
#[tokio::test]
async fn abandoned_preparations_do_not_claim_to_have_a_saved_report() {
    for error in [None, Some("setup failed".to_string())] {
        let (d, e, id) = running_engine().await;
        let path = d.path().join("worktrees").join(&id);
        std::fs::create_dir_all(&path).unwrap();
        e.update(|s| {
            s.reviews[0].status = if error.is_some() {
                "preparing"
            } else {
                "cancelled"
            }
            .into();
            s.reviews[0].thread_id = None;
        })
        .await;
        e.abandon_preparation(&id, error).await;
        let s = e.snapshot().await;
        assert_eq!(
            s.reviews[0].messages.last().unwrap().text,
            "Removed the unfinished review workspace. Start a new review to retry."
        );
    }
}
#[tokio::test]
async fn a_review_stopped_by_the_usage_limit_says_so() {
    let (_d, e, _) = running_engine().await;
    let error =
        json!({"message":"You've hit your usage limit.","codexErrorInfo":"usageLimitExceeded"});
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"failed","error":error}}})).await;
    let r = &e.snapshot().await.reviews[0];
    assert_eq!(r.status, "failed");
    assert!(r
        .error
        .as_deref()
        .unwrap()
        .contains("usage limit was reached"));
}
#[tokio::test]
async fn usage_updates_from_codex_reach_the_meter() {
    let (_d, e, _) = running_engine().await;
    let window = json!({"usedPercent":42,"windowDurationMins":10080,"resetsAt":1_800_000_000});
    e.event(json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","secondary":window}}})).await;
    let usage = e.shown_snapshot().await.usage.unwrap();
    assert_eq!(usage.rate_limits.secondary.unwrap().used_percent, 42.0);
    // The review the update arrived during is untouched.
    assert_eq!(e.snapshot().await.reviews[0].status, "reviewing");
}
#[test]
fn an_approval_answered_elsewhere_resumes_the_review() {
    let p = crate::types::tests::pr();
    let mut r = Review::new(&p, &Settings::default(), false);
    r.status = "awaitingApproval".into();
    let id = r.id.clone();
    let approval = |n: u64| Approval {
        id: serde_json::json!(n),
        review_id: id.clone(),
        method: "item/commandExecution/requestApproval".into(),
        params: serde_json::json!({}),
        requested_at: String::new(),
    };
    let mut s = Snapshot {
        reviews: vec![r],
        approvals: vec![approval(1), approval(2)],
        ..Snapshot::default()
    };
    let resolved = |n: u64| ReviewEvent::RequestResolved(serde_json::json!(n));
    super::run::apply_event(&mut s, &id, resolved(1));
    assert_eq!(s.reviews[0].status, "awaitingApproval");
    super::run::apply_event(&mut s, &id, resolved(2));
    assert_eq!(s.reviews[0].status, "reviewing");
}
