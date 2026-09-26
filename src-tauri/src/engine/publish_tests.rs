use super::test_support::*;

#[tokio::test]
async fn holds_automatic_publication_for_external_pull_requests() {
    let (_d, e, id) = running_engine().await;
    ready_for_autopost(&e, "No actionable defects found.", false).await;
    let error = e.publish(&id, true, false).await.unwrap_err();
    assert!(
        error.contains("collaborators, or trusted accounts"),
        "{error}"
    );
    assert_eq!(e.snapshot().await.reviews[0].status, "ready");
}
#[tokio::test]
async fn holds_automatic_publication_of_credential_shaped_reports() {
    let (_d, e, id) = running_engine().await;
    let report = format!("Found ghp_{} in the fixture.", "a1".repeat(18));
    ready_for_autopost(&e, &report, true).await;
    // Confirmation is a manual-only escape hatch; automatic runs ignore it.
    let error = e.publish(&id, true, true).await.unwrap_err();
    assert!(error.contains("GitHub token"), "{error}");
    assert_eq!(e.snapshot().await.reviews[0].status, "ready");
}
#[tokio::test]
async fn manual_publication_of_credential_shaped_reports_needs_confirmation() {
    let (_d, e, id) = running_engine().await;
    let report = format!("-----BEGIN RSA {} KEY-----", "PRIVATE");
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].report = report;
    })
    .await;
    let error = e.publish(&id, false, false).await.unwrap_err();
    assert!(
        error.contains("private key") && error.contains("confirm"),
        "{error}"
    );
    // Stopping early releases the publication claim.
    assert_eq!(e.snapshot().await.reviews[0].status, "ready");
}
#[tokio::test]
async fn publication_waits_for_a_running_follow_up() {
    let (_d, e, id) = running_engine().await;
    start_follow_up(&e, "ready").await;
    assert!(e.publish(&id, false, true).await.is_err());
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "reviewing");
    assert!(r.before_follow_up.is_some());
}
