//! The `orbit/review` commit status on each PR's latest automatic round.
use super::Engine;
#[cfg(not(test))]
use crate::github;
use crate::types::*;
use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::Ordering, Arc},
};

/// Sends one commit status to GitHub: repository, commit and status.
pub(super) type StatusSender = Arc<
    dyn Fn(String, String, CommitStatus) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;
#[cfg(not(test))]
pub(super) fn github_sender() -> StatusSender {
    Arc::new(|repo, sha, status| {
        Box::pin(async move { github::set_status(&repo, &sha, &status).await })
    })
}
/// Tests never reach GitHub; the ones about statuses set their own sender.
#[cfg(test)]
pub(super) fn github_sender() -> StatusSender {
    Arc::new(|_, _, _| Box::pin(async { Err("GitHub is not used in tests.".into()) }))
}

impl Engine {
    /// Sends each wanted commit status that GitHub neither shows nor refused
    /// for that commit. One request per commit at a time, recomputed after
    /// each, so a slow request for an older round cannot overwrite a newer
    /// one. A status GitHub refused is recorded and not sent again until the
    /// wanted status changes; reviews are unaffected. Runs whether or not
    /// Codex is connected.
    pub(super) async fn sync_commit_statuses(self: &Arc<Self>) {
        use crate::review_loop::{same_commit, wanted_commit_checks, WantedStatus};
        let wanted: Vec<WantedStatus> = {
            let mut s = self.state.lock().await;
            let wanted = wanted_commit_checks(&s);
            // A refusal only holds while the same status is wanted. Once the
            // wanted status moves on, even to what GitHub already shows, it is
            // forgotten, so a later return to the refused status is sent again.
            let mut changed = false;
            for w in &wanted {
                for c in s.commit_checks.iter_mut() {
                    if same_commit(&c.repo, &c.sha, &w.repo, &w.sha)
                        && c.refused.is_some()
                        && !w.refused_by(c)
                    {
                        c.refused = None;
                        changed = true;
                    }
                }
            }
            if changed {
                self.dirty.store(true, Ordering::SeqCst);
            }
            wanted
                .into_iter()
                .filter(|w| {
                    !s.commit_checks
                        .iter()
                        .any(|c| same_commit(&c.repo, &c.sha, &w.repo, &w.sha) && w.settled_by(c))
                })
                .collect()
        };
        for w in wanted {
            let key = format!("{}@{}", w.repo.to_ascii_lowercase(), w.sha);
            if !self.sending_statuses.lock().unwrap().insert(key.clone()) {
                continue;
            }
            let send = self.status_sender.lock().unwrap().clone();
            let e = self.clone();
            tokio::spawn(async move {
                let result = send(w.repo.clone(), w.sha.clone(), w.status.clone()).await;
                e.update(|s| {
                    let i = match s
                        .commit_checks
                        .iter()
                        .position(|c| same_commit(&c.repo, &c.sha, &w.repo, &w.sha))
                    {
                        Some(i) => i,
                        None => {
                            s.commit_checks.push(CommitCheck {
                                repo: w.repo.clone(),
                                sha: w.sha.clone(),
                                pr_id: w.pr_id.clone(),
                                accepted: None,
                                refused: None,
                            });
                            s.commit_checks.len() - 1
                        }
                    };
                    let c = &mut s.commit_checks[i];
                    c.pr_id = w.pr_id.clone();
                    match result {
                        Ok(()) => {
                            c.accepted = Some(w.status.clone());
                            c.refused = None;
                        }
                        // What GitHub shows is unchanged, so an accepted pending
                        // check is still closed later.
                        Err(error) => {
                            c.refused = Some(CommitStatus {
                                error: Some(error),
                                ..w.status.clone()
                            })
                        }
                    }
                    // Only pending checks and current heads are needed again.
                    let prs = s.prs.clone();
                    s.commit_checks.retain(|c| {
                        c.accepted.as_ref().is_some_and(|a| a.state == "pending")
                            || prs
                                .iter()
                                .any(|p| same_commit(&p.repo, &p.head_sha, &c.repo, &c.sha))
                    });
                })
                .await;
                e.sending_statuses.lock().unwrap().remove(&key);
            });
        }
    }
}

#[cfg(test)]
impl Engine {
    pub(super) fn set_status_sender(&self, sender: StatusSender) {
        *self.status_sender.lock().unwrap() = sender;
    }
}
