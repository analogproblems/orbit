//! Watching CI: after each sync, the checks on every connected repository's
//! default branch and open pull requests, and the problems they open or
//! close (see `crate::ci` and `crate::problems`).
use super::Engine;
use crate::{ci, logs, problems, types::*};
use std::sync::atomic::Ordering;

impl Engine {
    /// Reads CI in the background, as after Sync now.
    pub fn watch_ci_soon(self: &std::sync::Arc<Self>) {
        let e = self.clone();
        tokio::spawn(async move { e.watch_ci().await });
    }
    /// Reads each connected repository's CI and updates its problems,
    /// unless Watch CI is off or a read is already running, then tells
    /// agents what changed. A repository GitHub doesn't answer for keeps
    /// what was read before, with the error, and its problems stay as they
    /// were.
    pub async fn watch_ci(&self) {
        if self.watching_ci.swap(true, Ordering::SeqCst) {
            return;
        }
        self.watch_ci_now().await;
        self.read_failures().await;
        self.watching_ci.store(false, Ordering::SeqCst);
        // Also with Watch CI off, so what Orbit posted is closed.
        self.tell_agents().await;
    }
    async fn watch_ci_now(&self) {
        let (watch, repos) = {
            let s = self.state.lock().await;
            let repos: Vec<String> = s
                .settings
                .repositories
                .iter()
                .map(|r| r.name.clone())
                .collect();
            (s.settings.watch_ci, repos)
        };
        if !watch {
            self.update(stop_watching).await;
            return;
        }
        let mut read = vec![];
        // One repository at a time, as syncing does, for GitHub's limits.
        for repo in &repos {
            read.push((repo.clone(), ci::read(repo).await));
        }
        self.update(|s| {
            // Watch CI went off while CI was read: what came back is dropped.
            if !s.settings.watch_ci {
                stop_watching(s);
                return;
            }
            let now = chrono::Utc::now();
            for (repo, result) in read {
                // A repository disconnected while CI was read is left out.
                if !s
                    .settings
                    .repositories
                    .iter()
                    .any(|r| same_repo(&r.name, &repo))
                {
                    continue;
                }
                let known = s.ci.iter().position(|c| same_repo(&c.repo, &repo));
                match result {
                    Ok(fresh) => {
                        let followed: Vec<&PullRequest> = s
                            .prs
                            .iter()
                            .filter(|p| {
                                same_repo(&p.repo, &repo)
                                    && !p.draft
                                    && trusted_source(p, &s.settings.trusted_authors)
                            })
                            .collect();
                        problems::observe(&mut s.problems, &fresh, &followed, now);
                        match known {
                            Some(i) => s.ci[i] = fresh,
                            None => s.ci.push(fresh),
                        }
                    }
                    Err(error) => match known {
                        Some(i) => s.ci[i].error = Some(error),
                        None => s.ci.push(ci::RepoCi {
                            repo,
                            error: Some(error),
                            ..Default::default()
                        }),
                    },
                }
            }
            let connected = repos_of(s);
            s.ci.retain(|c| connected.iter().any(|r| same_repo(r, &c.repo)));
            problems::close_unwatched(&mut s.problems, &connected, false, now);
        })
        .await;
        let _ = self.flush().await;
    }
}
impl Engine {
    /// Reads why the jobs failing on open problems failed, a few new ones
    /// a pass, and forgets the ones no longer failing. A log GitHub didn't
    /// answer for is tried again on the next pass.
    async fn read_failures(&self) {
        let wanted = {
            let s = self.state.lock().await;
            logs::wanted(&s.problems, &s.failures)
        };
        for (repo, check, url) in wanted {
            let Ok(failure) = logs::read(&repo, &check, &url).await else {
                continue;
            };
            self.update(|s| {
                if !s.failures.iter().any(|f| f.url == failure.url) {
                    s.failures.push(failure);
                }
            })
            .await;
        }
        let pruned = {
            let mut s = self.state.lock().await;
            let s = &mut *s;
            logs::prune(&mut s.failures, &s.problems)
        };
        if pruned {
            self.dirty.store(true, Ordering::SeqCst);
        }
        let _ = self.flush().await;
    }
}
/// With Watch CI off, Orbit forgets what it read and closes what was open.
fn stop_watching(s: &mut Snapshot) {
    if s.ci.is_empty() && !s.problems.iter().any(|p| p.is_open()) {
        return;
    }
    s.ci.clear();
    problems::close_unwatched(&mut s.problems, &[], true, chrono::Utc::now());
}
fn repos_of(s: &Snapshot) -> Vec<String> {
    s.settings
        .repositories
        .iter()
        .map(|r| r.name.clone())
        .collect()
}
