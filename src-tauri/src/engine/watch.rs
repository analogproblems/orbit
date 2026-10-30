//! Watching CI: after each sync, the checks on every connected repository's
//! default branch and open pull requests, and the problems they open or
//! close (see `crate::ci` and `crate::problems`), whether CI runs on each
//! repository's pull requests (`crate::workflows`), the self-hosted
//! runners its runs wait for (`crate::runners`), its Actions storage
//! (`crate::actions_storage`), and whether GitHub says each pull request
//! Orbit reviews can merge (`crate::readiness`).
use super::Engine;
use crate::{
    actions_storage as storage, broke, ci, logs, problems, readiness, runners, types::*, workflows,
};
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
        self.read_breaks().await;
        self.read_setups().await;
        self.read_merges().await;
        self.watching_ci.store(false, Ordering::SeqCst);
        // Also with Watch CI off, so what Orbit posted is closed.
        self.tell_agents().await;
        // Last, so telling agents never waits for runners or storage.
        self.read_runners().await;
        self.read_storage().await;
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
                        let followed = followed(&s.prs, &s.settings.trusted_authors, &repo);
                        problems::observe(&mut s.problems, &fresh, &followed, now);
                        let setup = s.setups.iter_mut().find(|w| same_repo(&w.repo, &repo));
                        problems::see_setup(&mut s.problems, setup, &fresh, &followed, now);
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
            workflows::prune(&mut s.setups, &connected);
            runners::prune(&mut s.runners, &connected);
            storage::prune(&mut s.actions_storage, &connected);
            readiness::prune(s);
            problems::close_unwatched(&mut s.problems, &connected, false, now);
        })
        .await;
        let _ = self.flush().await;
    }
}
impl Engine {
    /// Reads why the jobs failing on open problems failed, a few a pass,
    /// and forgets the ones no longer failing. A log GitHub didn't answer
    /// for is tried again after a wait; after a rate limit, none is read
    /// until the wait is over.
    async fn read_failures(&self) {
        let wanted = {
            let s = self.state.lock().await;
            logs::wanted(&s.problems, &s.failures, chrono::Utc::now())
        };
        for (repo, check, url) in &wanted {
            let error = match logs::read(repo, check, url).await {
                Ok(failure) => {
                    self.update(|s| logs::record(&mut s.failures, failure))
                        .await;
                    continue;
                }
                Err(error) => error,
            };
            // After a rate limit, no log is read until its wait is over.
            let limited = logs::rate_limited(&error);
            self.update(|s| {
                logs::retry_later(&mut s.failures, check, url, &error, chrono::Utc::now())
            })
            .await;
            if limited {
                break;
            }
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
impl Engine {
    /// Reads where each newly broken default branch broke, a few a pass. A
    /// history GitHub didn't answer for is tried again after a wait.
    async fn read_breaks(&self) {
        let wanted = {
            let s = self.state.lock().await;
            broke::wanted(&s.problems, chrono::Utc::now())
        };
        for (id, repo, branch, start) in &wanted {
            let result = broke::read(repo, branch, start).await;
            self.update(|s| {
                broke::record(&mut s.problems, id, start, result, chrono::Utc::now());
            })
            .await;
        }
        let _ = self.flush().await;
    }
}
impl Engine {
    /// Reads what each repository's files say about its CI, a few a pass,
    /// at each new commit on its default branch, then opens or closes its
    /// problem. Files GitHub didn't answer for are tried again after a
    /// wait.
    async fn read_setups(&self) {
        let wanted = {
            let s = self.state.lock().await;
            if !s.settings.watch_ci {
                return;
            }
            workflows::wanted(&s.ci, &s.setups, chrono::Utc::now())
        };
        for (repo, branch, sha) in &wanted {
            let result = workflows::read(repo, branch, sha).await;
            self.update(|s| {
                // Watch CI went off, or the repository went, meanwhile.
                let watched = repos_of(s).iter().any(|r| same_repo(r, repo));
                if !s.settings.watch_ci || !watched {
                    return;
                }
                let now = chrono::Utc::now();
                workflows::record(&mut s.setups, repo, branch, sha, result, now);
                let Some(ci) = s.ci.iter().find(|c| same_repo(&c.repo, repo)) else {
                    return;
                };
                let followed = followed(&s.prs, &s.settings.trusted_authors, repo);
                let setup = s.setups.iter_mut().find(|w| same_repo(&w.repo, repo));
                problems::see_setup(&mut s.problems, setup, ci, &followed, now);
            })
            .await;
        }
        let _ = self.flush().await;
    }
}
impl Engine {
    /// Asks GitHub whether the pull requests Orbit reviews can merge, a
    /// few a pass, once nothing else is in the way, then forgets those it
    /// no longer reviews. One GitHub didn't answer for, or is still working
    /// out, is asked again after a wait.
    async fn read_merges(&self) {
        // While Watch CI is off, every answer waits on the checks, so
        // nothing is wanted.
        let wanted = readiness::wanted(&*self.state.lock().await, chrono::Utc::now());
        for (repo, number, head, base) in &wanted {
            let result = readiness::read(repo, *number, head, base).await;
            // After a rate limit, the rest wait for the next pass.
            let limited = result.as_ref().is_err_and(|e| logs::rate_limited(e));
            self.update(|s| {
                // Watch CI went off meanwhile.
                if !s.settings.watch_ci {
                    return;
                }
                let id = pr_id(repo, *number);
                readiness::record(&mut s.merges, &id, head, base, result, chrono::Utc::now());
                readiness::prune(s);
            })
            .await;
            if limited {
                break;
            }
        }
        let _ = self.flush().await;
    }
}
impl Engine {
    /// Reads the self-hosted runners of each connected repository and of
    /// each organization that owns one, and each repository's runs waiting
    /// for a runner, a few places a pass, then opens or closes the
    /// repository's problem about them. A place GitHub didn't answer for is
    /// tried again after a wait, and its problem stays as it was.
    async fn read_runners(&self) {
        if self.reading_runners.swap(true, Ordering::SeqCst) {
            return;
        }
        let wanted = {
            let s = self.state.lock().await;
            if s.settings.watch_ci {
                runners::wanted(&repos_of(&s), &s.runners, chrono::Utc::now())
            } else {
                vec![]
            }
        };
        for (name, kind) in &wanted {
            let result = runners::read(name, *kind).await;
            // After a rate limit, the rest wait for the next pass.
            let limited = result.as_ref().is_err_and(|e| logs::rate_limited(e));
            self.update(|s| {
                // Watch CI went off, or the repository went, meanwhile.
                let connected = repos_of(s);
                if !s.settings.watch_ci {
                    return;
                }
                let now = chrono::Utc::now();
                let repo = *kind == runners::PlaceKind::Repository;
                if let (true, Ok(place)) = (repo, &result) {
                    if connected.iter().any(|r| same_repo(r, name)) {
                        problems::see_waiting(
                            &mut s.problems,
                            name,
                            &place.waiting,
                            !place.more_queued,
                            now,
                        );
                    }
                }
                runners::record(&mut s.runners, name, *kind, result, now);
                runners::prune(&mut s.runners, &connected);
            })
            .await;
            if limited {
                break;
            }
        }
        self.reading_runners.store(false, Ordering::SeqCst);
        let _ = self.flush().await;
    }
    /// Reads each connected repository's caches and the artifacts it keeps,
    /// a few repositories a pass. One GitHub didn't answer for keeps what
    /// was read before and is tried again after a wait.
    async fn read_storage(&self) {
        if self.reading_storage.swap(true, Ordering::SeqCst) {
            return;
        }
        let wanted = {
            let s = self.state.lock().await;
            if s.settings.watch_ci {
                storage::wanted(&repos_of(&s), &s.actions_storage, chrono::Utc::now())
            } else {
                vec![]
            }
        };
        for repo in &wanted {
            let result = storage::read(repo).await;
            // After a rate limit, the rest wait for the next pass.
            let limited = result.as_ref().is_err_and(|e| logs::rate_limited(e));
            self.update(|s| {
                // Watch CI went off, or the repository went, meanwhile.
                if !s.settings.watch_ci {
                    return;
                }
                let connected = repos_of(s);
                storage::record(&mut s.actions_storage, repo, result, chrono::Utc::now());
                storage::prune(&mut s.actions_storage, &connected);
            })
            .await;
            if limited {
                break;
            }
        }
        self.reading_storage.store(false, Ordering::SeqCst);
        let _ = self.flush().await;
    }
}
/// The pull requests of a repository whose CI Orbit follows: open, not
/// drafts, and from a source Orbit trusts.
fn followed<'a>(prs: &'a [PullRequest], trusted: &[String], repo: &str) -> Vec<&'a PullRequest> {
    prs.iter()
        .filter(|p| same_repo(&p.repo, repo) && !p.draft && trusted_source(p, trusted))
        .collect()
}
/// With Watch CI off, Orbit forgets what it read and closes what was open.
fn stop_watching(s: &mut Snapshot) {
    s.ci.clear();
    s.setups.clear();
    s.runners.clear();
    s.actions_storage.clear();
    s.merges.clear();
    problems::close_unwatched(&mut s.problems, &[], true, chrono::Utc::now());
}
fn repos_of(s: &Snapshot) -> Vec<String> {
    s.settings
        .repositories
        .iter()
        .map(|r| r.name.clone())
        .collect()
}
