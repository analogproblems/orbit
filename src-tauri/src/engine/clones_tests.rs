use super::{clones::catch_up, test_support::*, worktrees::git};
use crate::types::*;
use std::path::{Path, PathBuf};

async fn commit(repo: &Path, file: &str) {
    std::fs::write(repo.join(file), file).unwrap();
    git(repo, &["add", file]).await.unwrap();
    git(
        repo,
        &[
            "-c",
            "user.name=Orbit",
            "-c",
            "user.email=orbit@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            file,
        ],
    )
    .await
    .unwrap();
}
/// An upstream repository and a clone of it.
async fn cloned() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (d, upstream) = git_repo().await;
    let clone = d.path().join("local");
    git(
        d.path(),
        &[
            "clone",
            "-q",
            upstream.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    )
    .await
    .unwrap();
    (d, upstream, clone)
}
async fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).await.unwrap()
}

#[tokio::test]
async fn fast_forwards_a_clean_clone_that_is_behind() {
    let (_d, upstream, clone) = cloned().await;
    commit(&upstream, "one").await;
    commit(&upstream, "two").await;
    let (state, message) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Updated, "{message}");
    assert!(message.contains("by 2 commits"), "{message}");
    assert_eq!(head(&clone).await, head(&upstream).await);
    let (state, _) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Current);
}
#[tokio::test]
async fn untracked_files_do_not_block_an_update() {
    let (_d, upstream, clone) = cloned().await;
    commit(&upstream, "one").await;
    std::fs::write(clone.join("notes.txt"), "mine").unwrap();
    let (state, message) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Updated, "{message}");
    assert_eq!(
        std::fs::read_to_string(clone.join("notes.txt")).unwrap(),
        "mine"
    );
}
#[tokio::test]
async fn never_overwrites_an_ignored_local_file() {
    let (_d, upstream, clone) = cloned().await;
    std::fs::write(clone.join(".git/info/exclude"), "settings.local\n").unwrap();
    std::fs::write(clone.join("settings.local"), "mine").unwrap();
    commit(&upstream, "settings.local").await;
    let before = head(&clone).await;
    let error = catch_up(&clone).await.unwrap_err();
    assert!(error.contains("Could not fast-forward"), "{error}");
    assert_eq!(head(&clone).await, before);
    assert_eq!(
        std::fs::read_to_string(clone.join("settings.local")).unwrap(),
        "mine"
    );
}
#[tokio::test]
async fn leaves_local_commits_alone() {
    let (_d, upstream, clone) = cloned().await;
    commit(&upstream, "theirs").await;
    commit(&clone, "mine").await;
    let before = head(&clone).await;
    let (state, message) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Skipped, "{message}");
    assert!(message.contains("1 local commit "), "{message}");
    assert_eq!(head(&clone).await, before);
}
#[tokio::test]
async fn leaves_uncommitted_changes_alone() {
    let (_d, upstream, clone) = cloned().await;
    commit(&upstream, "shared").await;
    catch_up(&clone).await.unwrap();
    commit(&upstream, "theirs").await;
    std::fs::write(clone.join("shared"), "edited").unwrap();
    let before = head(&clone).await;
    let (state, message) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Skipped, "{message}");
    assert!(message.contains("uncommitted changes"), "{message}");
    assert_eq!(head(&clone).await, before);
    assert_eq!(
        std::fs::read_to_string(clone.join("shared")).unwrap(),
        "edited"
    );
}
#[tokio::test]
async fn skips_a_detached_head_and_a_branch_without_upstream() {
    let (_d, upstream, clone) = cloned().await;
    commit(&upstream, "theirs").await;
    git(&clone, &["checkout", "-q", "--detach"]).await.unwrap();
    let (state, message) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Skipped);
    assert!(message.contains("not on a branch"), "{message}");
    git(&clone, &["checkout", "-q", "-b", "local-only"])
        .await
        .unwrap();
    let (state, message) = catch_up(&clone).await.unwrap();
    assert_eq!(state, CloneUpdateState::Skipped);
    assert!(message.contains("does not track"), "{message}");
}
#[tokio::test]
async fn records_updates_only_for_repositories_kept_up_to_date() {
    let (_d, e, _) = running_engine().await;
    let (_g, repo) = git_repo().await;
    let repository = |name: &str, keep_updated| Repository {
        name: name.into(),
        path: repo.to_string_lossy().into(),
        automatic: false,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated,
        provider: Default::default(),
    };
    e.update(|s| {
        s.settings.repositories = vec![repository("a/b", true), repository("c/d", false)];
    })
    .await;
    e.update_clones().await;
    let updates = e.snapshot().await.clone_updates;
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].repo, "a/b");
    // The test clone has no GitHub origin, so Orbit refuses to fetch into it.
    assert_eq!(updates[0].state, CloneUpdateState::Failed);
    assert!(
        updates[0].message.contains("origin"),
        "{}",
        updates[0].message
    );
    e.update(|s| s.settings.repositories[0].keep_updated = false)
        .await;
    e.update_clones().await;
    assert!(e.snapshot().await.clone_updates.is_empty());
}
