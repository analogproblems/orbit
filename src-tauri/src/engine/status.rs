//! The `orbit/review` commit status on each PR's latest automatic round.
use super::Engine;
#[cfg(not(test))]
use crate::github;
use crate::types::*;
use std::{future::Future, pin::Pin, sync::Arc};

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
    /// Sends each wanted commit status that differs from the last one sent
    /// for that commit. One request per commit at a time, recomputed after
    /// each, so a slow request for an older round cannot overwrite a newer
    /// one. A status GitHub refused is recorded and not sent again until the
    /// wanted status changes; reviews are unaffected. Runs whether or not
    /// Codex is connected.
    pub(super) async fn sync_commit_statuses(self: &Arc<Self>) {
        let wanted: Vec<CommitCheck> = {
            let s = self.state.lock().await;
            crate::review_loop::wanted_commit_checks(&s)
                .into_iter()
                .filter(|w| {
                    !s.commit_checks.iter().any(|c| {
                        c.pr_id == w.pr_id
                            && c.sha == w.sha
                            && c.status.state == w.status.state
                            && c.status.description == w.status.description
                    })
                })
                .collect()
        };
        for check in wanted {
            let key = format!("{}@{}", check.repo.to_ascii_lowercase(), check.sha);
            if !self.sending_statuses.lock().unwrap().insert(key.clone()) {
                continue;
            }
            let send = self.status_sender.lock().unwrap().clone();
            let e = self.clone();
            tokio::spawn(async move {
                let result =
                    send(check.repo.clone(), check.sha.clone(), check.status.clone()).await;
                e.update(|s| {
                    s.commit_checks
                        .retain(|c| !(c.pr_id == check.pr_id && c.sha == check.sha));
                    s.commit_checks.push(CommitCheck {
                        status: CommitStatus {
                            error: result.err(),
                            ..check.status
                        },
                        ..check
                    });
                    // Only open checks and current heads are needed again.
                    let prs = s.prs.clone();
                    s.commit_checks.retain(|c| {
                        c.status.state == "pending"
                            || prs.iter().any(|p| p.id == c.pr_id && p.head_sha == c.sha)
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
