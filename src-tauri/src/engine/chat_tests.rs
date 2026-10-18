use super::{test_support::*, Engine};
use crate::types::*;
use serde_json::json;

#[tokio::test]
async fn failed_follow_up_keeps_the_report_publishable() {
    let (d, e, id) = running_engine().await;
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].report = "Finding".into();
        s.reviews[0].worktree = Some(d.path().to_string_lossy().into());
    })
    .await;
    // Missing historical model information is rejected before claiming a turn.
    assert!(e
        .message(&id, "Why?")
        .await
        .unwrap_err()
        .contains("Choose a chat model"));
    let s = e.snapshot().await;
    let r = &s.reviews[0];
    assert_eq!(r.status, "ready");
    assert!(r.error.is_none());
    assert!(publishable(r, &s.prs[0]));
}
#[tokio::test]
async fn stopping_a_follow_up_restores_the_published_review() {
    let (_d, e, id) = running_engine().await;
    start_follow_up(&e, "posted").await;
    e.cancel(&id).await.unwrap();
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "posted");
    assert!(r.before_follow_up.is_none());
}
#[tokio::test]
async fn follow_up_failures_and_disconnects_restore_the_outcome() {
    let (_d, e, _) = running_engine().await;
    start_follow_up(&e, "ready").await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"failed","error":{"message":"boom"}}}})).await;
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "ready");
    assert_eq!(r.error.as_deref(), Some("The follow-up failed: boom"));
    start_follow_up(&e, "ready").await;
    e.event(json!({"method":"orbit/disconnected"})).await;
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "ready");
    assert!(r.error.unwrap().contains("disconnected"));
}
#[tokio::test]
async fn completed_follow_up_keeps_the_review_outcome() {
    let (_d, e, _) = running_engine().await;
    start_follow_up(&e, "failed").await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}})).await;
    // A chat answer does not turn a failed review into a publishable one.
    assert_eq!(e.snapshot().await.reviews[0].status, "failed");
}
#[tokio::test]
async fn restart_during_a_follow_up_restores_the_outcome() {
    let (d, e, _) = running_engine().await;
    start_follow_up(&e, "ready").await;
    e.flush().await.unwrap();
    drop(e);
    let restored = Engine::new(d.path().into(), |_| {}).unwrap();
    let r = restored.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "ready");
    assert!(r.error.unwrap().contains("restarted during the follow-up"));
}
#[tokio::test]
async fn limits_follow_ups_by_characters_not_bytes() {
    let (d, e, id) = running_engine().await;
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(d.path().to_string_lossy().into());
    })
    .await;
    // 32,000 two-byte characters are within the limit.
    let error = e.message(&id, &"é".repeat(32000)).await.unwrap_err();
    assert!(!error.contains("32,000"), "{error}");
    let error = e.message(&id, &"é".repeat(32001)).await.unwrap_err();
    assert!(error.contains("32,000"), "{error}");
}
#[tokio::test]
async fn a_disconnect_leaves_queued_reviews_waiting() {
    let (_d, e, _) = running_engine().await;
    e.update(|s| s.reviews[0].status = "queued".into()).await;
    e.event(json!({"method":"orbit/disconnected"})).await;
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "queued");
    assert!(r.error.is_none());
}
