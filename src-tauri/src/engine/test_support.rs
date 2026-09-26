use super::{worktrees::git, Engine};
use crate::types::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub(super) async fn running_engine() -> (tempfile::TempDir, Arc<Engine>, String) {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    let p = crate::types::tests::pr();
    let mut review = Review::new(&p, &Settings::default(), false);
    review.status = "reviewing".into();
    review.thread_id = Some("thread-1".into());
    review.turn_id = Some("turn-1".into());
    let id = review.id.clone();
    e.update(|s| {
        s.prs.push(p);
        s.reviews.push(review);
    })
    .await;
    (d, e, id)
}
pub(super) async fn ready_for_autopost(e: &Engine, report: &str, trusted: bool) {
    e.update(|s| {
        s.settings.automatic = true;
        s.settings.auto_post = true;
        s.settings.repositories.push(Repository {
            name: "a/b".into(),
            path: "/tmp/a-b".into(),
            automatic: true,
            setup_command: String::new(),
            network_access: false,
            round_limit: 4,
        });
        if trusted {
            s.prs[0].head_repo = "a/b".into();
            s.prs[0].author_association = "MEMBER".into();
        }
        let r = &mut s.reviews[0];
        r.status = "ready".into();
        r.automatic = true;
        r.report = report.into();
    })
    .await;
}
pub(super) async fn start_follow_up(e: &Engine, before: &str) {
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.status = "reviewing".into();
        r.is_review = false;
        r.report = "Finding".into();
        r.before_follow_up = Some(Outcome {
            status: before.into(),
            error: None,
        });
    })
    .await;
}
pub(super) async fn git_repo() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("clone");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]).await.unwrap();
    let identity = [
        "-c",
        "user.name=Orbit",
        "-c",
        "user.email=orbit@example.com",
        "-c",
        "commit.gpgsign=false",
    ];
    let commit = ["commit", "-q", "--allow-empty", "-m", "init"];
    git(&repo, &[&identity[..], &commit[..]].concat())
        .await
        .unwrap();
    (d, repo)
}
pub(super) async fn worktree_count(repo: &Path) -> usize {
    git(repo, &["worktree", "list", "--porcelain"])
        .await
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("worktree "))
        .count()
}
pub(super) async fn autopilot_engine(pr: PullRequest) -> (tempfile::TempDir, Arc<Engine>) {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    let checkout = d.path().to_string_lossy().into_owned();
    e.update(|s| {
        s.codex_ready = true;
        s.model_catalog_ready = true;
        s.models = crate::models::tests::catalog();
        s.settings.model = "model-a".into();
        s.settings.effort = "medium".into();
        s.settings.automatic = true;
        s.settings.repositories.push(Repository {
            name: pr.repo.clone(),
            path: checkout,
            automatic: true,
            setup_command: String::new(),
            network_access: false,
            round_limit: 4,
        });
        s.prs.push(pr);
    })
    .await;
    (d, e)
}
pub(super) fn trusted_pr() -> PullRequest {
    let mut p = crate::types::tests::pr();
    p.head_repo = p.repo.clone();
    p.author_association = "OWNER".into();
    p
}
pub(super) async fn wait_for_status(e: &Engine, status: &str) -> Review {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let r = e.snapshot().await.reviews[0].clone();
            if r.status == status {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Timed out waiting for status {status}"))
}
