//! Each connected repository's pull-request history, read from GitHub in the
//! background and kept under `ledger/` in Orbit's data folder, for the
//! Decisions page. See `crate::ledger` for how it is read and pieced
//! together. Only GitHub is read; no reviewer runs.
use super::*;
use crate::ledger::{self as store, Decisions, StoredPr};

/// How often the history is read again.
pub(super) const REFRESH: Duration = Duration::from_secs(5 * 60);

/// Whether a connected repository's history has not been read yet.
pub(super) fn unread(s: &Snapshot) -> bool {
    s.settings
        .repositories
        .iter()
        .any(|r| !s.ledger.repos.iter().any(|l| same_repo(&l.repo, &r.name)))
}

/// Marks the work as finished however it ends.
pub(super) struct Reading<'a>(pub(super) &'a AtomicBool);
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub(super) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

impl Engine {
    /// Starts reading the history in the background, unless a read is
    /// already running. Returns whether it started.
    pub fn refresh_ledger_soon(self: &Arc<Self>) -> bool {
        if self.refreshing_ledger.swap(true, Ordering::SeqCst) {
            return false;
        }
        let e = self.clone();
        tokio::spawn(async move {
            let _reading = Reading(&e.refreshing_ledger);
            e.refresh_ledger().await;
            e.write_up_soon();
        });
        true
    }
    /// Reads each connected repository's 100 most recently updated pull
    /// requests: those not stored yet, those updated since, and every open
    /// one, as an edited comment doesn't always update its pull request.
    /// The stored history of repositories no longer connected is removed.
    pub(super) async fn refresh_ledger(&self) {
        let repos: Vec<String> = {
            let s = self.state.lock().await;
            s.settings
                .repositories
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        self.update_live(|s| s.ledger.refreshing = true).await;
        let root = self.root.clone();
        let connected = repos.clone();
        // A folder that can't be removed now is tried again next time.
        let _ = blocking(move || store::prune(&root, &connected)).await;
        for repo in &repos {
            let mut changed = false;
            let result = self.refresh_repo(repo, &mut changed).await;
            self.update(|s| {
                let at = result.is_ok().then(now);
                let error = result.err();
                match s.ledger.repos.iter_mut().find(|l| same_repo(&l.repo, repo)) {
                    Some(l) => {
                        l.refreshed_at = at.or(l.refreshed_at.take());
                        l.error = error;
                    }
                    None => s.ledger.repos.push(LedgerRepo {
                        repo: repo.clone(),
                        refreshed_at: at,
                        error,
                    }),
                }
                if changed {
                    s.ledger.version += 1;
                }
            })
            .await;
        }
        self.update(|s| {
            s.ledger.refreshing = false;
            s.ledger.progress = None;
            let connected = &s.settings.repositories;
            s.ledger
                .repos
                .retain(|l| connected.iter().any(|r| same_repo(&r.name, &l.repo)));
        })
        .await;
    }
    /// Reads one repository's pull requests that need it, stopping at the
    /// first that fails. Sets `changed` when any stored history changed,
    /// even if a later pull request then fails.
    async fn refresh_repo(&self, repo: &str, changed: &mut bool) -> Result<(), String> {
        let listed = crate::github::recent_pulls(repo).await?;
        let listed: Vec<(u64, Value)> = listed
            .into_iter()
            .filter_map(|p| Some((p["number"].as_u64()?, p)))
            .collect();
        let root = self.root.clone();
        let name = repo.to_string();
        let numbers: Vec<u64> = listed.iter().map(|(n, _)| *n).collect();
        let stored: Vec<Option<StoredPr>> = blocking(move || {
            numbers
                .iter()
                .map(|n| store::load(&root, &name, *n))
                .collect()
        })
        .await?;
        let due: Vec<(u64, Option<StoredPr>)> = listed
            .into_iter()
            .zip(stored)
            .filter(|((_, p), previous)| {
                let current = previous.as_ref().is_some_and(|s| {
                    p["state"] != "open" && p["updated_at"].as_str() == Some(&s.pr.updated_at)
                });
                !current
            })
            .map(|((number, _), previous)| (number, previous))
            .collect();
        let total = due.len() as u32;
        for (done, (number, previous)) in due.into_iter().enumerate() {
            self.update_live(|s| {
                s.ledger.progress = Some(LedgerProgress {
                    repo: repo.into(),
                    number: None,
                    done: done as u32,
                    total,
                })
            })
            .await;
            let h = crate::github::pull_history(repo, number).await?;
            let next = store::from_github(
                repo,
                &h.pr,
                &h.issue_comments,
                &h.reviews,
                &h.review_comments,
                &h.commits,
                &h.files,
                previous.as_ref(),
                &now(),
            );
            if previous.as_ref().is_some_and(|p| {
                *p == StoredPr {
                    fetched_at: p.fetched_at.clone(),
                    ..next.clone()
                }
            }) {
                continue;
            }
            let root = self.root.clone();
            blocking(move || store::save(&root, &next)).await?;
            *changed = true;
        }
        Ok(())
    }
    /// Every connected repository's stored pull requests, pieced together,
    /// most recently updated first.
    pub async fn decisions(&self) -> Result<Decisions, String> {
        let repos: Vec<String> = {
            let s = self.state.lock().await;
            s.settings
                .repositories
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        let root = self.root.clone();
        blocking(move || {
            let mut out = Decisions::default();
            for repo in &repos {
                match store::load_repo(&root, repo) {
                    Ok((stored, problems)) => {
                        // A write-up shows until a newer one replaces it.
                        let mut writeups = store::load_writeups(&root, repo).unwrap_or_default();
                        out.prs.extend(stored.iter().map(|s| {
                            let mut h = store::derive(s);
                            h.writeup = writeups.remove(&h.number);
                            h
                        }));
                        out.problems.extend(problems);
                    }
                    Err(e) => out.problems.push(format!("{repo}: {e}")),
                }
            }
            out.prs.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.repo.cmp(&b.repo))
                    .then_with(|| b.number.cmp(&a.number))
            });
            Ok(out)
        })
        .await
    }
}
