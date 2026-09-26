use super::{
    test_support::*, worktrees::git, worktrees::prune_orbit_refs, worktrees::remove_worktree,
    Engine,
};
use crate::types::*;
use std::{sync::atomic::Ordering, time::Duration};

#[tokio::test]
async fn notes_a_failing_worktree_removal_once() {
    let (_d, e, id) = running_engine().await;
    // A plain file where the worktree should be cannot be removed as a directory.
    let path = e.root.join("worktrees").join(&id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
        s.history_prs = std::mem::take(&mut s.prs);
    })
    .await;
    e.clean_worktrees().await;
    e.clean_worktrees().await;
    let r = e.snapshot().await.reviews[0].clone();
    assert!(
        r.worktree.is_some(),
        "a failed removal keeps the worktree recorded"
    );
    let notes = r
        .messages
        .iter()
        .filter(|m| m.text.starts_with("Could not remove"))
        .count();
    assert_eq!(notes, 1);
}
#[tokio::test]
async fn removes_worktrees_and_orbit_refs_from_the_clone() {
    let (d, repo) = git_repo().await;
    let worktree = d.path().join("review");
    let location = worktree.to_str().unwrap();
    git(&repo, &["worktree", "add", "--detach", location, "HEAD"])
        .await
        .unwrap();
    git(&repo, &["update-ref", "refs/orbit/old-run", "HEAD"])
        .await
        .unwrap();
    prune_orbit_refs(&repo).await;
    assert_eq!(
        git(&repo, &["for-each-ref", "refs/orbit/"]).await.unwrap(),
        ""
    );
    assert_eq!(worktree_count(&repo).await, 2);
    remove_worktree(&worktree).await.unwrap();
    assert!(!worktree.exists());
    assert_eq!(worktree_count(&repo).await, 1);
    // Already gone is fine.
    remove_worktree(&worktree).await.unwrap();
}
#[tokio::test]
async fn removes_worktrees_of_reviews_whose_pr_is_no_longer_open() {
    let (_d, repo) = git_repo().await;
    let (_state, e, id) = running_engine().await;
    let worktree = e.root.join("worktrees").join(&id);
    let location = worktree.to_str().unwrap();
    git(&repo, &["worktree", "add", "--detach", location, "HEAD"])
        .await
        .unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(location.into());
    })
    .await;
    e.clean_worktrees().await;
    assert!(worktree.exists(), "open PRs keep their worktrees");
    // The PR left the inbox; its repository is not connected, so no closure check.
    e.update(|s| s.history_prs = std::mem::take(&mut s.prs))
        .await;
    e.clean_worktrees().await;
    let r = e.snapshot().await.reviews[0].clone();
    assert!(r.worktree.is_none());
    assert!(!worktree.exists());
    assert_eq!(worktree_count(&repo).await, 1);
    let error = e.message(&id, "One more question").await.unwrap_err();
    assert!(error.contains("worktree is unavailable"), "{error}");
}
#[tokio::test]
async fn cleanup_claims_before_git_wait_and_rejects_competing_actions() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
    })
    .await;
    let git = e.git_lock.lock().await;
    let cleanup = tokio::spawn({
        let e = e.clone();
        let id = id.clone();
        async move { e.remove_worktrees(vec![id]).await }
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !e.snapshot().await.reviews[0].cleanup_pending {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let disk: Snapshot =
        serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap()).unwrap();
    assert!(disk.reviews[0].cleanup_pending);
    assert!(disk.reviews[0].worktree.is_some());
    assert!(e
        .message(&id, "follow up")
        .await
        .unwrap_err()
        .contains("being removed"));
    assert!(e.publish(&id, false, false).await.is_err());
    assert!(e.remove_worktrees(vec![id]).await[0]
        .error
        .as_ref()
        .unwrap()
        .contains("already"));
    drop(git);
    assert!(cleanup.await.unwrap()[0].error.is_none());
    assert!(!path.exists());
}
#[tokio::test]
async fn cleanup_partial_results_and_restart_preserve_reports_and_deduplication() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    let mut blocked = e.snapshot().await.reviews[0].clone();
    blocked.id = uuid::Uuid::new_v4().to_string();
    blocked.status = "publishing".into();
    let blocked_id = blocked.id.clone();
    e.update(|s| {
        s.reviews[0].status = "posted".into();
        s.reviews[0].automatic = true;
        s.reviews[0].report = "Saved report".into();
        s.reviews[0].comment_url = Some("https://github.com/a/b/pull/1#issuecomment-1".into());
        s.reviews[0]
            .messages
            .push(Message::new("user", "Saved chat"));
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
        s.reviews[0].cleanup_pending = true;
        s.reviews[0].cleanup_closed_only = Some(false);
        s.reviews.push(blocked);
    })
    .await;
    e.flush().await.unwrap();
    drop(e);
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    // Recover intent even when the directory already disappeared before exit.
    e.resume_cleanup().await;
    let r = e.snapshot().await.reviews[0].clone();
    assert!(!r.cleanup_pending && r.worktree.is_none());
    assert_eq!(r.report, "Saved report");
    assert_eq!(r.status, "posted");
    assert!(r.automatic && r.comment_url.is_some());
    assert_eq!(r.messages[0].text, "Saved chat");
    assert_eq!(r.id, id);
    e.update(|s| s.reviews[1].status = "publishing".into())
        .await;
    let results = e
        .remove_worktrees(vec![blocked_id, "unknown".into(), id.clone()])
        .await;
    assert!(results[0].error.is_some() && results[1].error.is_some() && results[2].error.is_none());
    assert!(e
        .snapshot()
        .await
        .reviews
        .iter()
        .any(|r| r.id == id && r.automatic));
}
#[tokio::test]
async fn cleanup_rechecks_after_git_wait_and_never_removes_outside_registered_root() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
    })
    .await;
    let lock = e.git_lock.lock().await;
    let cleanup = tokio::spawn({
        let e = e.clone();
        let id = id.clone();
        async move { e.remove_worktrees(vec![id]).await }
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !e.snapshot().await.reviews[0].cleanup_pending {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    e.update(|s| s.reviews[0].status = "publishing".into())
        .await;
    drop(lock);
    assert!(cleanup.await.unwrap()[0].error.is_some());
    assert!(path.exists());
    assert!(!e.snapshot().await.reviews[0].cleanup_pending);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep"), "original clone").unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(outside.path().to_string_lossy().into());
    })
    .await;
    assert!(e.remove_worktrees(vec![id]).await[0].error.is_some());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("keep")).unwrap(),
        "original clone"
    );
    assert!(e.snapshot().await.reviews[0].cleanup_error.is_some());
}
#[tokio::test]
async fn recovered_cleanup_removes_real_worktree_and_still_deduplicates_automation() {
    let (_repo_dir, repo) = git_repo().await;
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
    )
    .await
    .unwrap();
    e.update(|s| {
        let p = &mut s.prs[0];
        p.head_repo = p.repo.clone();
        p.author_association = "OWNER".into();
        s.settings.repositories.push(Repository {
            name: p.repo.clone(),
            path: repo.to_string_lossy().into(),
            automatic: true,
            setup_command: String::new(),
            network_access: false,
        });
        let r = &mut s.reviews[0];
        r.status = "ready".into();
        r.automatic = true;
        r.report = "keep report".into();
        r.cleanup_pending = true;
        r.cleanup_closed_only = Some(false);
        r.worktree = Some(path.to_string_lossy().into());
    })
    .await;
    e.flush().await.unwrap();
    drop(e);
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    e.resume_cleanup().await;
    assert!(!path.exists());
    assert_eq!(worktree_count(&repo).await, 1);
    let s = e.snapshot().await;
    assert_eq!(s.reviews[0].report, "keep report");
    assert!(!eligible(&s.prs[0], &s));
    let mut without_record = s.clone();
    without_record.reviews.clear();
    assert!(eligible(&s.prs[0], &without_record));
}
#[tokio::test]
async fn recovery_rechecks_automatic_cleanup_but_honors_confirmed_manual_removal() {
    for kind in [Some(true), Some(false), None] {
        let (d, e, id) = running_engine().await;
        let path = d.path().join("worktrees").join(&id);
        std::fs::create_dir_all(&path).unwrap();
        e.update(|s| {
            s.settings.repositories.push(Repository {
                name: "a/b".into(),
                path: "/tmp/source".into(),
                automatic: false,
                setup_command: String::new(),
                network_access: false,
            });
            let r = &mut s.reviews[0];
            r.status = "ready".into();
            r.report = "retained".into();
            r.cleanup_pending = true;
            r.cleanup_closed_only = kind;
            r.worktree = Some(path.to_string_lossy().into());
            s.history_prs = std::mem::take(&mut s.prs);
        })
        .await;
        e.flush().await.unwrap();
        drop(e);
        let e = Engine::new(d.path().into(), |_| {}).unwrap();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        // GitHub says it reopened while Orbit was closed; cached metadata is stale.
        e.resume_cleanup_with(|_| {
            checks.fetch_add(1, Ordering::SeqCst);
            async { Ok(false) }
        })
        .await;
        let r = e.snapshot().await.reviews[0].clone();
        assert!(!r.cleanup_pending);
        assert!(r.cleanup_error.is_none());
        assert_eq!(r.report, "retained");
        assert!(!r
            .messages
            .iter()
            .any(|m| m.text.starts_with("Could not remove")));
        assert_eq!(path.exists(), kind != Some(false));
        assert_eq!(
            checks.load(Ordering::SeqCst),
            usize::from(kind != Some(false))
        );
    }
}
#[tokio::test]
async fn automatic_cleanup_skips_busy_git_without_leaving_a_pending_claim() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
        s.history_prs = std::mem::take(&mut s.prs);
    })
    .await;
    let fetch = e.git_lock.lock().await;
    tokio::time::timeout(Duration::from_secs(1), e.clean_worktrees())
        .await
        .unwrap();
    assert!(!e.snapshot().await.reviews[0].cleanup_pending);
    assert!(path.exists());
    // An interrupted automatic cleanup also releases its old claim when Git is busy.
    e.update(|s| {
        s.reviews[0].cleanup_pending = true;
        s.reviews[0].cleanup_closed_only = Some(true);
    })
    .await;
    tokio::time::timeout(Duration::from_secs(1), e.resume_cleanup())
        .await
        .unwrap();
    assert!(!e.snapshot().await.reviews[0].cleanup_pending);
    drop(fetch);
    e.clean_worktrees().await;
    assert!(!path.exists());
}
#[tokio::test]
async fn aborting_preparation_cannot_abort_its_claimed_cleanup() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| {
        s.reviews[0].status = "preparing".into();
        s.reviews[0].thread_id = None;
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
    })
    .await;
    let fetch = e.git_lock.lock().await;
    let caller = tokio::spawn({
        let e = e.clone();
        let id = id.clone();
        async move {
            e.abandon_preparation(&id, Some("setup failed".into()))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !e.snapshot().await.reviews[0].cleanup_pending {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    drop(fetch);
    tokio::time::timeout(Duration::from_secs(2), async {
        while e.snapshot().await.reviews[0].cleanup_pending {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!path.exists());
    assert!(e.snapshot().await.reviews[0].worktree.is_none());
}
#[tokio::test]
async fn failed_cleanup_intent_is_visible_and_retryable_without_deleting_files() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("keep"), "contents").unwrap();
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
    })
    .await;
    // Deterministic write failure exercises the same path as a full state volume.
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    let results = e.remove_worktrees(vec![id.clone()]).await;
    let error = results[0].error.as_ref().unwrap();
    assert!(error.contains("not started") && error.contains("Free space"));
    let r = e.snapshot().await.reviews[0].clone();
    assert!(!r.cleanup_pending && r.cleanup_error.is_some());
    assert_eq!(
        std::fs::read_to_string(path.join("keep")).unwrap(),
        "contents"
    );
    std::fs::remove_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e.remove_worktrees(vec![id]).await[0].error.is_none());
    assert!(!path.exists());
}
#[tokio::test]
async fn removed_worktree_reports_completion_write_failure_separately_and_recovers() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.status = "ready".into();
        r.report = "preserve report".into();
        r.worktree = Some(path.to_string_lossy().into());
    })
    .await;
    let git = e.git_lock.lock().await;
    let worker = tokio::spawn({
        let e = e.clone();
        let id = id.clone();
        async move { e.remove_worktrees(vec![id]).await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !e.snapshot().await.reviews[0].cleanup_pending {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // Intent is durable; fail only the write after deletion.
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    drop(git);
    let result = worker.await.unwrap().remove(0);
    assert!(result.error.is_none());
    assert!(result
        .warning
        .unwrap()
        .contains("completion record could not be saved"));
    assert!(!path.exists());
    let s = e.snapshot().await;
    assert!(s.reviews[0].worktree.is_none());
    assert!(s.reviews[0].cleanup_error.is_none());
    assert!(s.codex_error.unwrap().contains("automation paused"));
    drop(e);
    std::fs::remove_dir(d.path().join("state.json.tmp")).unwrap();
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    assert!(e.snapshot().await.reviews[0].cleanup_pending);
    e.resume_cleanup().await;
    let r = &e.snapshot().await.reviews[0];
    assert!(r.worktree.is_none() && !r.cleanup_pending);
    assert_eq!(r.report, "preserve report");
}
#[tokio::test]
async fn path_error_is_not_masked_by_a_failed_flush() {
    let (d, e, id) = running_engine().await;
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some("/outside/orbit".into());
    })
    .await;
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    let result = e.remove_worktrees(vec![id]).await.remove(0);
    assert!(result
        .error
        .unwrap()
        .contains("outside its registered Orbit location"));
    assert!(e
        .snapshot()
        .await
        .codex_error
        .unwrap()
        .contains("automation paused"));
}
#[tokio::test]
async fn busy_sweeps_make_no_github_requests_and_recheck_open_prs_before_deletion() {
    let (d, e, id) = running_engine().await;
    let path = d.path().join("worktrees").join(&id);
    std::fs::create_dir_all(&path).unwrap();
    e.update(|s| {
        s.settings.repositories.push(Repository {
            name: "a/b".into(),
            path: "/tmp/source".into(),
            automatic: false,
            setup_command: String::new(),
            network_access: false,
        });
        s.reviews[0].status = "ready".into();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
        let mut second = s.reviews[0].clone();
        second.id = uuid::Uuid::new_v4().to_string();
        s.reviews.push(second);
        s.history_prs = std::mem::take(&mut s.prs);
    })
    .await;
    let requests = std::sync::atomic::AtomicUsize::new(0);
    let git = e.git_lock.lock().await;
    e.clean_worktrees_with(|_| {
        requests.fetch_add(1, Ordering::SeqCst);
        async { true }
    })
    .await;
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    drop(git);
    // The PR reopened during the network check; the worker must retain it.
    e.clean_worktrees_with(|pr| async {
        e.update(|s| s.prs.push(pr)).await;
        true
    })
    .await;
    assert!(path.exists());
    assert!(e.snapshot().await.reviews[0].cleanup_error.is_none());
}
