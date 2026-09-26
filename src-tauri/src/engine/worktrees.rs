use super::Engine;
use crate::{github, storage, types::*};
use std::{
    collections::HashSet,
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

impl Engine {
    pub async fn remove_worktrees(
        self: &Arc<Self>,
        ids: Vec<String>,
    ) -> Vec<storage::CleanupResult> {
        let mut results = vec![];
        let mut seen = HashSet::new();
        for id in ids {
            if seen.insert(id.clone()) {
                let (error, warning) = match self.remove_review_worktree(&id, false, false).await {
                    Ok(warning) => (None, warning),
                    Err(error) => (Some(error), None),
                };
                results.push(storage::CleanupResult {
                    review_id: id,
                    error,
                    warning,
                });
            }
        }
        results
    }
    pub(super) async fn resume_cleanup(self: &Arc<Self>) {
        self.resume_cleanup_with(|pr| async move {
            match github::pull(&pr.repo, pr.number, "").await {
                Err(error) if error == github::CLOSED => Ok(true),
                Ok(_) => Ok(false),
                Err(error) => Err(error),
            }
        })
        .await;
    }
    pub(super) async fn resume_cleanup_with<F, Fut>(self: &Arc<Self>, is_closed: F)
    where
        F: Fn(PullRequest) -> Fut,
        Fut: std::future::Future<Output = Result<bool, String>>,
    {
        let pending: Vec<_> = self
            .snapshot()
            .await
            .reviews
            .into_iter()
            .filter(|r| r.cleanup_pending)
            .collect();
        for r in pending {
            if r.cleanup_closed_only.unwrap_or(true) {
                let s = self.snapshot().await;
                let pr = s.prs.iter().chain(&s.history_prs).find(|p| p.id == r.pr_id);
                let closed = match pr {
                    Some(pr)
                        if s.settings
                            .repositories
                            .iter()
                            .any(|c| same_repo(&c.name, &pr.repo)) =>
                    {
                        is_closed(pr.clone()).await
                    }
                    Some(_) => Ok(true), // Disconnected repository.
                    None => Ok(false),   // Missing metadata cannot authorize automatic deletion.
                };
                // A skipped automatic removal is reconsidered by later sweeps.
                // It must not leave follow-ups blocked while another Git job runs.
                self.release_cleanup(
                    &r.id,
                    closed
                        .as_ref()
                        .err()
                        .map(|e| format!("Could not recheck interrupted cleanup: {e}")),
                )
                .await;
                if closed != Ok(true) {
                    continue;
                }
                let _ = self.remove_review_worktree(&r.id, false, true).await;
            } else {
                let _ = self.remove_review_worktree(&r.id, true, false).await;
            }
        }
    }
    pub(super) async fn release_cleanup(&self, id: &str, error: Option<String>) {
        self.update(|s| {
            if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                r.cleanup_pending = false;
                r.cleanup_closed_only = None;
                r.cleanup_error = error;
            }
        })
        .await;
        let _ = self.flush().await;
    }
    pub(super) async fn remove_review_worktree(
        self: &Arc<Self>,
        id: &str,
        resume: bool,
        closed_only: bool,
    ) -> Result<Option<String>, String> {
        // Stop may abort the preparing task which requested cleanup. Once
        // dispatched, cleanup has its own lifetime and must finish its claim.
        let e = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            e.remove_review_worktree_inner(&id, resume, closed_only)
                .await
        })
        .await
        .map_err(|error| format!("Cleanup worker failed: {error}"))?
    }
    pub(super) async fn remove_review_worktree_inner(
        &self,
        id: &str,
        resume: bool,
        closed_only: bool,
    ) -> Result<Option<String>, String> {
        if !resume
            && self
                .state
                .lock()
                .await
                .reviews
                .iter()
                .any(|r| r.id == id && r.cleanup_pending)
        {
            return Err("This worktree is already being removed.".into());
        }
        // Keep one worker per directory, including the automatic cleanup paths.
        let _cleanup = if closed_only {
            match self.cleanup_lock.try_lock() {
                Ok(guard) => guard,
                Err(_) => return Ok(None),
            }
        } else {
            self.cleanup_lock.lock().await
        };
        // Retain the try-lock guard throughout background removal. Never queue
        // a sweep behind a fetch that starts after an earlier availability check.
        let git_guard = if closed_only {
            match self.git_lock.try_lock() {
                Ok(guard) => Some(guard),
                Err(_) => return Ok(None),
            }
        } else {
            None
        };
        let path = {
            let mut s = self.state.lock().await;
            let r = s
                .reviews
                .iter()
                .find(|r| r.id == id)
                .ok_or("Unknown review.")?;
            if !storage::inactive(r) {
                return Err(
                    "Stop the run and wait for publication before removing its worktree.".into(),
                );
            }
            if r.cleanup_pending && !resume {
                return Err("This worktree is already being removed.".into());
            }
            if closed_only && s.prs.iter().any(|p| p.id == r.pr_id) {
                return Ok(None);
            }
            let Some(recorded) = &r.worktree else {
                drop(s);
                self.release_cleanup(id, None).await;
                return Ok(None);
            };
            let path = match storage::worktree_path(&self.root, id, recorded) {
                Ok(path) => path,
                Err(error) => {
                    let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
                    r.cleanup_pending = false;
                    r.cleanup_closed_only = None;
                    r.cleanup_error = Some(error.clone());
                    let note = format!("Could not remove this review's worktree: {error}");
                    if r.messages.last().map(|m| &m.text) != Some(&note) {
                        r.messages.push(Message::new("system", note));
                    }
                    self.dirty.store(true, Ordering::SeqCst);
                    drop(s);
                    // Flush reports persistence failures through the shared state;
                    // preserve the actual removal error for this run.
                    let _ = self.flush().await;
                    return Err(error);
                }
            };
            let mut next = s.clone();
            let r = next.reviews.iter_mut().find(|r| r.id == id).unwrap();
            r.cleanup_pending = true;
            r.cleanup_closed_only = Some(closed_only);
            r.cleanup_error = None;
            // Persist the intent before waiting on Git or deleting anything.
            if let Err(error) = self.persist(&next) {
                let error = format!("Cleanup was not started because Orbit could not save its removal record: {error}. Free space or restore write access for Orbit’s data directory, then retry.");
                let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
                r.cleanup_pending = false;
                r.cleanup_closed_only = None;
                r.cleanup_error = Some(error.clone());
                self.dirty.store(true, Ordering::SeqCst);
                (self.emit)(&s);
                return Err(error);
            }
            *s = next;
            (self.emit)(&s);
            path
        };
        let removed = async {
            let _git = match git_guard {
                Some(guard) => guard,
                None => self.git_lock.lock().await,
            };
            {
                let s = self.state.lock().await;
                let r = s
                    .reviews
                    .iter()
                    .find(|r| r.id == id)
                    .ok_or("Unknown review.")?;
                if !storage::inactive(r) || !r.cleanup_pending {
                    return Err("The review changed while cleanup was waiting.".into());
                }
                if closed_only && s.prs.iter().any(|p| p.id == r.pr_id) {
                    return Ok(false);
                }
                storage::worktree_path(
                    &self.root,
                    id,
                    r.worktree.as_deref().ok_or("Workspace changed.")?,
                )?;
            }
            remove_worktree(&path).await.map(|()| true)
        }
        .await;
        self.update(|s| {
            if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                r.cleanup_pending = false;
                r.cleanup_closed_only = None;
                r.cleanup_error = removed.as_ref().err().cloned();
                let note = match &removed {
                    Ok(false) => return, // Reopened PR: keeping its worktree is success.
                    Ok(true) => {
                        r.worktree = None;
                        if r.thread_id.is_none() && r.report.trim().is_empty() {
                            "Removed the unfinished review workspace. Start a new review to retry.".to_string()
                        } else if r.report.trim().is_empty() {
                            "Removed this review's worktree. Its conversation is preserved; start a new review to continue chatting.".to_string()
                        } else {
                            "Removed this review's worktree. Its report and conversation are preserved; start a new review to continue chatting.".to_string()
                        }
                    }
                    Err(e) => format!("Could not remove this review's worktree at {}: {e}", path.display()),
                };
                if r.messages.last().map(|m| &m.text) != Some(&note) { r.messages.push(Message::new("system", note)); }
            }
        }).await;
        let saved = self.flush().await;
        match removed {
            Ok(true) => Ok(saved.err().map(|e| format!("Cleanup finished, but its completion record could not be saved: {e}. Orbit will retry saving and reconcile the recorded removal on restart."))),
            Ok(false) => Ok(None), // The reopened PR kept its worktree.
            Err(error) => Err(error),
        }
    }
    /// Cleanup does not discard chat history or change automatic-review deduplication.
    pub(super) async fn clean_worktrees(self: &Arc<Self>) {
        self.clean_worktrees_with(|pr| async move {
            matches!(github::pull(&pr.repo, pr.number, "").await, Err(e) if e == github::CLOSED)
        })
        .await;
    }
    pub(super) async fn clean_worktrees_with<F, Fut>(self: &Arc<Self>, is_closed: F)
    where
        F: Fn(PullRequest) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let candidates: Vec<(String, PullRequest, bool)> = {
            let s = self.state.lock().await;
            s.reviews
                .iter()
                .filter(|r| {
                    r.worktree.is_some()
                        && storage::inactive(r)
                        && !r.cleanup_pending
                        && !s.prs.iter().any(|p| p.id == r.pr_id)
                })
                .filter_map(|r| {
                    let pr = s.history_prs.iter().find(|p| p.id == r.pr_id)?;
                    let connected = s
                        .settings
                        .repositories
                        .iter()
                        .any(|c| same_repo(&c.name, &pr.repo));
                    Some((r.id.clone(), pr.clone(), connected))
                })
                .collect()
        };
        for (id, pr, connected) in candidates {
            // Avoid GitHub requests for an entire sweep while Git or cleanup is
            // occupied. The removal worker still retains its own try-lock guard.
            if self.git_lock.try_lock().is_err() || self.cleanup_lock.try_lock().is_err() {
                return;
            }
            if connected && !is_closed(pr).await {
                continue;
            }
            let _ = self.remove_review_worktree(&id, false, true).await;
        }
    }
}
/// Removes a review worktree and its registration in the source clone.
pub(super) async fn remove_worktree(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let location = path.to_str().ok_or("Invalid workspace path")?;
    // A worktree has a `.git` file. Without one (a partial checkout), Git would
    // search parent directories and could act on an unrelated repository.
    let common = if path.join(".git").is_file() {
        git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await
    } else {
        Err(String::new())
    };
    if let Ok(common) = common {
        let common = common.trim();
        // Run against the source clone: Git will not remove the worktree it runs in.
        let removed = git(
            Path::new(common),
            &[
                "--git-dir",
                common,
                "worktree",
                "remove",
                "--force",
                location,
            ],
        )
        .await;
        if removed.is_ok() {
            return Ok(());
        }
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
        let _ = git(
            Path::new(common),
            &["--git-dir", common, "worktree", "prune"],
        )
        .await;
        return Ok(());
    }
    // The source clone is gone or broken; its registration is pruned by Git later.
    std::fs::remove_dir_all(path).map_err(|e| e.to_string())
}
/// Deletes Orbit's temporary PR refs from a clone.
pub(super) async fn prune_orbit_refs(cwd: &Path) {
    let Ok(refs) = git(cwd, &["for-each-ref", "--format=%(refname)", "refs/orbit/"]).await else {
        return;
    };
    for reference in refs.lines().filter(|r| !r.is_empty()) {
        let _ = git(cwd, &["update-ref", "-d", reference]).await;
    }
}
pub(super) async fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let mut full = vec![
        "-c".to_string(),
        format!(
            "core.hooksPath={}",
            if cfg!(windows) { "NUL" } else { "/dev/null" }
        ),
    ];
    full.extend(args.iter().map(|s| s.to_string()));
    // A first fetch or checkout of a large repository can take many minutes.
    github::run_with_timeout("git", &full, Some(cwd), None, Duration::from_secs(30 * 60)).await
}
