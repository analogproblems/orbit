//! Asking GitHub whether each of Dependabot's pull requests can merge
//! (`crate::dependabot`).
use super::Engine;
use crate::{dependabot, logs};
use std::sync::atomic::Ordering;

impl Engine {
    /// Asks GitHub whether Dependabot's pull requests can merge, a few a
    /// pass, and forgets what it no longer shows. Only while Watch CI and
    /// Watch security alerts are both on. Nothing is written to GitHub.
    pub(super) async fn read_updates(&self) {
        let wanted = {
            let mut s = self.state.lock().await;
            if dependabot::prune(&mut s) {
                self.dirty.store(true, Ordering::SeqCst);
            }
            dependabot::wanted(&s, chrono::Utc::now())
        };
        for (repo, number, head, base) in &wanted {
            let result = dependabot::read(repo, *number, head, base).await;
            // After a rate limit, the rest wait for the next pass.
            let limited = result.as_ref().is_err_and(|e| logs::rate_limited(e));
            self.update(|s| {
                dependabot::record(s, repo, *number, head, base, result, chrono::Utc::now());
            })
            .await;
            if limited {
                break;
            }
        }
        let _ = self.flush().await;
    }
}
