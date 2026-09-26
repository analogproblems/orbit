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
