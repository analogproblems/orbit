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
    /// Sends each PR's wanted commit status when it differs from the last one
    /// sent for that round, and closes checks left pending by rounds that were
    /// stopped or replaced. A status GitHub refused is recorded on the round
    /// and not retried until the wanted status changes; the review itself is
    /// unaffected.
    pub(super) async fn sync_commit_statuses(self: &Arc<Self>) {
        let wanted: Vec<(String, String, String, CommitStatus)> = {
            let s = self.state.lock().await;
            s.prs
                .iter()
                .filter_map(|pr| {
                    let (id, status) = crate::review_loop::wanted_status(&s, pr)?;
                    let r = s.reviews.iter().find(|r| r.id == id)?;
                    let sent = r.commit_status.as_ref().is_some_and(|c| {
                        c.state == status.state && c.description == status.description
                    });
                    (!sent).then(|| (id, pr.repo.clone(), r.head_sha.clone(), status))
                })
                .chain(s.reviews.iter().filter_map(|r| {
                    let status = crate::review_loop::closing_status(r)?;
                    let pr = s
                        .prs
                        .iter()
                        .chain(&s.history_prs)
                        .find(|p| p.id == r.pr_id)?;
                    Some((r.id.clone(), pr.repo.clone(), r.head_sha.clone(), status))
                }))
                .collect()
        };
        for (id, repo, sha, status) in wanted {
            if !self.sending_statuses.lock().unwrap().insert(id.clone()) {
                continue;
            }
            let send = self.status_sender.lock().unwrap().clone();
            let e = self.clone();
            tokio::spawn(async move {
                let result = send(repo, sha, status.clone()).await;
                e.update(|s| {
                    if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                        r.commit_status = Some(CommitStatus {
                            error: result.err(),
                            ..status
                        });
                    }
                })
                .await;
                e.sending_statuses.lock().unwrap().remove(&id);
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
