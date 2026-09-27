use super::Engine;
use crate::{github, types::*};
use std::{
    collections::HashSet,
    path::Path,
    sync::{atomic::Ordering, Arc},
};

impl Engine {
    pub async fn poll(&self) -> Result<(), String> {
        let Ok(_guard) = self.poll_lock.try_lock() else {
            return Ok(());
        };
        self.update(|s| s.polling = true).await;
        let _ = self.flush().await;
        let result = async {
            let login = github::user().await?;
            let (repos, previous) = {
                let s = self.state.lock().await;
                let repos: Vec<String> = s
                    .settings
                    .repositories
                    .iter()
                    .map(|r| r.name.clone())
                    .collect();
                (repos, s.prs.clone())
            };
            let mut prs = vec![];
            let mut synced = HashSet::new();
            let mut failed = vec![];
            // Sequential requests respect GitHub secondary rate limits.
            for repo in &repos {
                match sync_repo(repo, &login, &previous).await {
                    Ok(found) => {
                        prs.extend(found);
                        synced.insert(repo.to_ascii_lowercase());
                    }
                    Err(e) => {
                        // Keep what is known about this repository until it syncs again.
                        prs.extend(
                            previous
                                .iter()
                                .filter(|p| same_repo(&p.repo, repo))
                                .cloned(),
                        );
                        failed.push((repo.clone(), e));
                    }
                }
            }
            self.update(|s| {
                for pr in &mut prs {
                    // Keep when the head was first seen, so pushes can settle.
                    pr.head_seen_at = s
                        .prs
                        .iter()
                        .find(|p| p.id == pr.id && p.head_sha == pr.head_sha)
                        .map_or_else(now, |p| p.head_seen_at.clone());
                }
                for previous in &s.prs {
                    if !prs.iter().any(|p| p.id == previous.id) {
                        s.history_prs.retain(|p| p.id != previous.id);
                        s.history_prs.push(previous.clone());
                    }
                }
                s.history_prs
                    .retain(|p| !prs.iter().any(|current| current.id == p.id));
                s.prs = prs;
                s.github_user = Some(login);
                s.github_error = (!failed.is_empty()).then(|| {
                    let each: Vec<String> =
                        failed.iter().map(|(r, e)| format!("{r}: {e}")).collect();
                    format!("Could not sync {}", each.join("; "))
                });
                crate::attention::count_sync_failures(s, &failed);
                s.last_poll = Some(now());
            })
            .await;
            *self.synced_repos.lock().await = synced;
            Ok::<(), String>(())
        }
        .await;
        if result.is_err() {
            self.synced_repos.lock().await.clear();
        }
        self.update(|s| {
            s.polling = false;
            match &result {
                Ok(()) => s.poll_failures = 0,
                Err(e) => {
                    s.github_error = Some(e.clone());
                    s.poll_failures += 1;
                }
            }
        })
        .await;
        let _ = self.flush().await;
        result
    }
    pub async fn enqueue(&self, pr_id: &str, automatic: bool) -> Result<String, String> {
        let mut s = self.state.lock().await;
        let pr = s
            .prs
            .iter()
            .find(|p| p.id == pr_id)
            .cloned()
            .ok_or("Pull request is no longer in the inbox. Sync first.")?;
        if !s.codex_ready {
            return Err("Connect Codex in Settings before starting a review.".into());
        }
        ModelSelection::workspace(&s.settings).validate_execution(&s)?;
        if let Some(error) = &s.model_configuration_error {
            return Err(error.clone());
        }
        if s.reviews
            .iter()
            .any(|r| r.pr_id == pr_id && active(&r.status))
        {
            return Err("This pull request already has an active review.".into());
        }
        if automatic && (!s.settings.automatic || !eligible(&pr, &s)) {
            return Err("This PR is not eligible for automatic review.".into());
        }
        let repo = s
            .settings
            .repositories
            .iter()
            .find(|r| same_repo(&r.name, &pr.repo))
            .ok_or("Add this repository’s local checkout in Repositories first.")?;
        if !Path::new(&repo.path).is_dir() {
            return Err(
                "The local checkout no longer exists. Update its path in Repositories.".into(),
            );
        }
        let r = Review::new(&pr, &s.settings, automatic);
        let id = r.id.clone();
        let mut next = s.clone();
        next.reviews.insert(0, r);
        self.persist(&next)
            .map_err(|e| format!("Review was not queued: {e}"))?;
        *s = next;
        (self.emit)(&s);
        Ok(id)
    }
    pub(super) async fn schedule(self: &Arc<Self>) {
        self.supersede_outdated_runs().await;
        let candidates: Vec<String> = {
            let s = self.state.lock().await;
            let synced = self.synced_repos.lock().await;
            if s.settings.automatic
                && s.model_configuration_error.is_none()
                && ModelSelection::workspace(&s.settings)
                    .validate_execution(&s)
                    .is_ok()
            {
                s.prs
                    .iter()
                    .filter(|p| synced.contains(&p.repo.to_ascii_lowercase()) && eligible(p, &s))
                    .map(|p| p.id.clone())
                    .collect()
            } else {
                vec![]
            }
        };
        for id in candidates {
            let _ = self.enqueue(&id, true).await;
        }
        loop {
            let next = {
                let mut s = self.state.lock().await;
                let running = s
                    .reviews
                    .iter()
                    .filter(|r| active(&r.status) && r.status != "queued")
                    .count();
                if running >= s.settings.concurrency {
                    None
                } else {
                    let enabled: std::collections::HashSet<String> = s
                        .prs
                        .iter()
                        .filter(|p| {
                            s.settings.automatic
                                && trusted_source(p, &s.settings.trusted_authors)
                                && s.settings
                                    .repositories
                                    .iter()
                                    .any(|r| same_repo(&r.name, &p.repo) && r.automatic)
                        })
                        .map(|p| p.id.clone())
                        .collect();
                    s.reviews
                        .iter_mut()
                        .rev()
                        .find(|r| {
                            r.status == "queued" && (!r.automatic || enabled.contains(&r.pr_id))
                        })
                        .map(|r| {
                            r.status = "preparing".into();
                            r.id.clone()
                        })
                }
            };
            let Some(id) = next else { break };
            self.dirty.store(true, Ordering::SeqCst);
            let e = self.clone();
            let task_id = id.clone();
            let task = tokio::spawn(async move {
                if let Err(err) = e.prepare(&task_id).await {
                    e.abandon_preparation(&task_id, Some(err)).await;
                }
                e.preparing.lock().unwrap().remove(&task_id);
            });
            let mut preparing = self.preparing.lock().unwrap();
            if !task.is_finished() {
                preparing.insert(id, task.abort_handle());
            }
        }
    }
}
impl Engine {
    /// Replaces automatic runs whose PR moved to a new head or base. A run
    /// that has not started inference is stopped so the new head can be
    /// queued; one that has keeps running but is marked superseded, so it is
    /// not published automatically, and the new head is queued once it ends.
    /// A follow-up chat on a finished round is not a run and keeps its round.
    /// State is only marked for saving when a run was superseded, since this
    /// runs on every scheduler tick.
    pub(super) async fn supersede_outdated_runs(self: &Arc<Self>) {
        let mut stop = vec![];
        {
            let mut s = self.state.lock().await;
            let prs = s.prs.clone();
            let mut changed = false;
            for r in s.reviews.iter_mut() {
                let outdated = r.automatic
                    && !r.superseded
                    && active(&r.status)
                    && r.before_follow_up.is_none()
                    && prs.iter().any(|p| {
                        p.id == r.pr_id && (p.head_sha != r.head_sha || p.base_sha != r.base_sha)
                    });
                if outdated {
                    r.superseded = true;
                    changed = true;
                    if matches!(r.status.as_str(), "queued" | "preparing") {
                        stop.push(r.id.clone());
                    }
                }
            }
            if changed {
                self.dirty.store(true, Ordering::SeqCst);
            }
        }
        for id in stop {
            let _ = self.cancel(&id).await;
        }
    }
    /// Pauses the automatic loop for a PR. A round that has not started yet
    /// is stopped; a running one finishes.
    pub async fn stop_loop(self: &Arc<Self>, pr_id: &str) -> Result<(), String> {
        let queued: Vec<String> = {
            let mut s = self.state.lock().await;
            let mut next = s.clone();
            match next.loops.iter_mut().find(|c| c.pr_id == pr_id) {
                Some(c) => c.stopped = true,
                None => next.loops.push(LoopControl {
                    pr_id: pr_id.into(),
                    stopped: true,
                    extra_rounds: 0,
                }),
            }
            self.persist(&next)
                .map_err(|e| format!("The loop was not stopped: {e}"))?;
            *s = next;
            (self.emit)(&s);
            s.reviews
                .iter()
                .filter(|r| r.pr_id == pr_id && r.automatic && r.status == "queued")
                .map(|r| r.id.clone())
                .collect()
        };
        for id in queued {
            let _ = self.cancel(&id).await;
        }
        Ok(())
    }
    /// Resumes a stopped loop and allows `rounds` more automatic rounds.
    pub async fn continue_loop(&self, pr_id: &str, rounds: u32) -> Result<(), String> {
        if !(1..=10).contains(&rounds) {
            return Err("Continue for 1 to 10 more rounds.".into());
        }
        let mut s = self.state.lock().await;
        let pr = s
            .prs
            .iter()
            .find(|p| p.id == pr_id)
            .cloned()
            .ok_or("Pull request is no longer in the inbox. Sync first.")?;
        let used = crate::review_loop::state(&s, &pr).map_or(0, |l| l.rounds);
        let base = crate::review_loop::round_limit(&s, &pr)
            - s.loops
                .iter()
                .find(|c| c.pr_id == pr_id)
                .map_or(0, |c| c.extra_rounds);
        let mut next = s.clone();
        let control = match next.loops.iter_mut().find(|c| c.pr_id == pr_id) {
            Some(c) => c,
            None => {
                next.loops.push(LoopControl {
                    pr_id: pr_id.into(),
                    ..LoopControl::default()
                });
                next.loops.last_mut().unwrap()
            }
        };
        control.stopped = false;
        // `rounds` more from here, whatever was granted before.
        control.extra_rounds = (used + rounds)
            .saturating_sub(base)
            .max(control.extra_rounds);
        self.persist(&next)
            .map_err(|e| format!("The loop was not continued: {e}"))?;
        *s = next;
        (self.emit)(&s);
        Ok(())
    }
}
/// Open PRs in one repository. Size statistics only come from the per-PR
/// endpoint, so a PR is fetched individually only when it is new or changed.
pub(super) async fn sync_repo(
    repo: &str,
    login: &str,
    previous: &[PullRequest],
) -> Result<Vec<PullRequest>, String> {
    let mut found = vec![];
    for mut pr in github::open_pulls(repo, login).await? {
        if !reuse_stats(&mut pr, previous) {
            match github::pull(repo, pr.number, login).await {
                Ok(detail) => pr = detail,
                Err(e) if e == github::CLOSED => continue,
                Err(e) => return Err(e),
            }
        }
        found.push(pr);
    }
    Ok(found)
}
pub(super) fn reuse_stats(pr: &mut PullRequest, previous: &[PullRequest]) -> bool {
    let Some(known) = previous
        .iter()
        .find(|p| p.id == pr.id && p.updated_at == pr.updated_at && p.head_sha == pr.head_sha)
    else {
        return false;
    };
    pr.additions = known.additions;
    pr.deletions = known.deletions;
    pr.changed_files = known.changed_files;
    true
}
