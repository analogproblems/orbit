//! Asking GitHub whether each of Dependabot's pull requests can merge, and
//! asking Dependabot to rebase one that conflicts, on your tap
//! (`crate::dependabot`).
use super::Engine;
use crate::{dependabot, github, logs};
use std::sync::atomic::Ordering;

impl Engine {
    /// Asks GitHub whether Dependabot's pull requests can merge, a few a
    /// pass, and forgets what it no longer shows. Only while Watch CI and
    /// Watch security alerts are both on. Nothing is written to GitHub. An
    /// ask to rebase that Orbit stopping cut off is looked for on GitHub
    /// first, unless one is being sent now.
    pub(super) async fn read_updates(&self) {
        if let Ok(_one) = self.rebasing.try_lock() {
            self.look_for_cut_off_rebases().await;
        }
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
    /// Asks Dependabot to rebase one of its pull requests, on your tap: it
    /// posts `dependabot::REBASE` on it. Orbit asks GitHub about it again
    /// first, and only if it still conflicts at the same commit does Orbit
    /// record the comment in the journal, save that, and post it. One tap
    /// posts one comment, one tap at a time, and only one for each commit
    /// unless it didn't go through.
    pub async fn ask_rebase(&self, pr_id: &str) -> Result<(), String> {
        let _one = self.rebasing.lock().await;
        // None is being sent now, so one still marked as sending was cut
        // off by Orbit stopping.
        self.look_for_cut_off_rebases().await;
        let (repo, number, head, base) = dependabot::may_rebase(&*self.state.lock().await, pr_id)?;
        // Dependabot may have rebased it already, or GitHub may say it
        // merges now.
        let read = dependabot::read(&repo, number, &head, &base).await;
        let failed = read.as_ref().err().map(|e| logs::short(e));
        self.update(|s| {
            dependabot::record(s, &repo, number, &head, &base, read, chrono::Utc::now());
        })
        .await;
        if let Some(e) = failed {
            let _ = self.flush().await;
            return Err(format!(
                "Orbit couldn't check it with GitHub first, so it didn't ask Dependabot: {e}."
            ));
        }
        let id = {
            let mut s = self.state.lock().await;
            // Saved before it is sent, so after a restart Orbit knows it
            // may have gone, and doesn't send it again. What GitHub said
            // just now may mean it no longer conflicts.
            let id = self.save(
                &mut s,
                |s| {
                    dependabot::may_rebase(s, pr_id)?;
                    Ok(dependabot::record_rebase(
                        &mut s.journal,
                        &repo,
                        number,
                        &head,
                        chrono::Utc::now(),
                    ))
                },
                |e| format!("Orbit couldn't save its journal, so it didn't ask Dependabot: {e}"),
            );
            (self.emit)(&s);
            id
        };
        let id = match id {
            Ok(id) => id,
            Err(e) => {
                // What GitHub said just now is still worth showing.
                let _ = self.flush().await;
                return Err(e);
            }
        };
        let sent = github::post_comment(&repo, number, dependabot::REBASE, dependabot::REBASE)
            .await
            .map(|posted| posted.url)
            .map_err(|e| dependabot::rebase_error(&e));
        self.update(|s| {
            dependabot::settle_rebase(&mut s.journal, &id, sent.clone(), chrono::Utc::now());
        })
        .await;
        let _ = self.flush().await;
        sent.map(|_| ())
    }
    /// Looks on GitHub for each ask to rebase that Orbit stopping cut off,
    /// and records whether it got there. None is sent again. Only while
    /// holding `rebasing`, so none is being sent.
    async fn look_for_cut_off_rebases(&self) {
        let (cut, login) = {
            let mut s = self.state.lock().await;
            let cut = dependabot::interrupted(&mut s.journal, chrono::Utc::now());
            if cut.is_empty() {
                return;
            }
            self.dirty.store(true, Ordering::SeqCst);
            (cut, s.github_user.clone().unwrap_or_default())
        };
        for (id, repo, number, since) in &cut {
            let found = if login.is_empty() {
                Err("Orbit doesn't know its GitHub sign-in yet".to_string())
            } else {
                github::replies(repo, *number, since, 1)
                    .await
                    .map(|(comments, _)| dependabot::find_rebase(&comments, &login))
            };
            self.update(|s| {
                dependabot::found_rebase(&mut s.journal, id, found, chrono::Utc::now());
            })
            .await;
        }
        let _ = self.flush().await;
    }
}
