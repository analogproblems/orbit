use super::{settings::matches_origin, worktrees::git, Engine};
use crate::types::*;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};

impl Engine {
    /// Runs `update_clones` in the background, such as after a manual sync.
    pub fn update_clones_soon(self: &Arc<Self>) {
        let e = self.clone();
        tokio::spawn(async move { e.update_clones().await });
    }
    /// Brings each clone marked "keep up to date" level with its upstream. A
    /// clone with local work is never changed; its card says why instead.
    pub(super) async fn update_clones(&self) {
        if self.updating_clones.swap(true, Ordering::SeqCst) {
            return;
        }
        let repos: Vec<Repository> = {
            let s = self.state.lock().await;
            s.settings
                .repositories
                .iter()
                .filter(|r| r.keep_updated)
                .cloned()
                .collect()
        };
        let mut results = vec![];
        for repo in &repos {
            let (state, message) = {
                let _git = self.git_lock.lock().await;
                match fast_forward(Path::new(&repo.path), &repo.name).await {
                    Ok(done) => done,
                    Err(e) => (CloneUpdateState::Failed, e),
                }
            };
            results.push(CloneUpdate {
                repo: repo.name.clone(),
                state,
                message,
                at: now(),
            });
        }
        self.update(|s| {
            // Settings may have changed while Git ran; keep only clones still
            // marked to stay up to date.
            let kept = |name: &str| {
                s.settings
                    .repositories
                    .iter()
                    .any(|r| r.keep_updated && same_repo(&r.name, name))
            };
            let mut updates: Vec<CloneUpdate> =
                results.into_iter().filter(|u| kept(&u.repo)).collect();
            for old in std::mem::take(&mut s.clone_updates) {
                if kept(&old.repo) && !updates.iter().any(|u| same_repo(&u.repo, &old.repo)) {
                    updates.push(old);
                }
            }
            s.clone_updates = updates;
        })
        .await;
        self.updating_clones.store(false, Ordering::SeqCst);
    }
}

/// Fetches `origin` and fast-forwards the checked-out branch to its upstream
/// when the clone has no local commits or uncommitted changes to tracked files.
pub(super) async fn fast_forward(
    path: &Path,
    repo: &str,
) -> Result<(CloneUpdateState, String), String> {
    let cwd = std::fs::canonicalize(path).map_err(|e| format!("Cannot access checkout: {e}"))?;
    let origin = git(&cwd, &["remote", "get-url", "origin"]).await?;
    if !matches_origin(origin.trim(), repo) {
        return Err(format!(
            "Checkout origin does not match {repo}. Update its path in Repositories."
        ));
    }
    catch_up(&cwd).await
}

/// The part of `fast_forward` after the origin check.
pub(super) async fn catch_up(cwd: &Path) -> Result<(CloneUpdateState, String), String> {
    let cwd = cwd.to_path_buf();
    git(&cwd, &["fetch", "--no-tags", "origin"]).await?;
    let skipped = |why: String| Ok((CloneUpdateState::Skipped, why));
    let Ok(branch) = git(&cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await else {
        return skipped("Not updated: the clone is not on a branch.".into());
    };
    let branch = branch.trim();
    let Ok(upstream) = git(
        &cwd,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .await
    else {
        return skipped(format!(
            "Not updated: {branch} does not track a remote branch."
        ));
    };
    let upstream = upstream.trim();
    let counts = git(
        &cwd,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )
    .await?;
    let (ahead, behind) = match counts
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .as_deref()
    {
        Ok([ahead, behind]) => (*ahead, *behind),
        _ => return Err(format!("Unexpected output from git rev-list: {counts}")),
    };
    if behind == 0 {
        return Ok((
            CloneUpdateState::Current,
            format!("{branch} is up to date with {upstream}."),
        ));
    }
    if ahead > 0 {
        return skipped(format!(
            "Not updated: {branch} has {ahead} local {} that {upstream} does not.",
            commits(ahead)
        ));
    }
    let changes = git(&cwd, &["status", "--porcelain", "--untracked-files=no"]).await?;
    if !changes.trim().is_empty() {
        return skipped(format!(
            "Not updated: {branch} has uncommitted changes. Commit or stash them and Orbit will catch it up."
        ));
    }
    git(&cwd, &["merge", "--ff-only", "--quiet", "@{upstream}"])
        .await
        .map_err(|e| format!("Could not fast-forward {branch}: {e}"))?;
    Ok((
        CloneUpdateState::Updated,
        format!(
            "Fast-forwarded {branch} by {behind} {} from {upstream}.",
            commits(behind)
        ),
    ))
}

fn commits(n: u64) -> &'static str {
    if n == 1 {
        "commit"
    } else {
        "commits"
    }
}
