//! Each connected repository's security settings against its owner's
//! baseline (`crate::baseline`): reading a few repositories a pass, and
//! changing a setting when you tap Fix on it. Both run under
//! `reading_settings`, one at a time, so a read that began before a change
//! never undoes what the change showed, and a change still marked as
//! sending when this starts was cut off by Orbit stopping.
use super::{watch::repos_of, Engine};
use crate::{
    baseline::{self, Setting},
    logs,
    types::*,
};
use std::sync::{atomic::Ordering, Arc};

impl Engine {
    /// Notes your tap on Fix for a setting a repository drifts on, then
    /// changes it in the background.
    pub async fn fix_setting(self: &Arc<Self>, repo: &str, setting: Setting) -> Result<(), String> {
        self.ask_to_fix(repo, setting).await?;
        let e = self.clone();
        tokio::spawn(async move { e.read_settings().await });
        Ok(())
    }
    /// Notes your tap on Fix, and saves it. Only with Watch security alerts
    /// on, for a connected repository whose owner's baseline asks for the
    /// setting, and where Orbit last read it off.
    pub(super) async fn ask_to_fix(&self, repo: &str, setting: Setting) -> Result<(), String> {
        let mut asked = Ok(());
        self.update(|s| {
            asked = if watched_settings(s).iter().any(|r| same_repo(r, repo)) {
                let baselines = &s.settings.security_baselines;
                baseline::ask(&mut s.repo_settings, &s.journal, baselines, repo, setting)
            } else {
                Err(format!(
                    "Orbit doesn't watch {repo}'s security settings now. Watch security alerts is off, or the repository isn't connected."
                ))
            };
        })
        .await;
        asked?;
        self.flush().await
    }
    /// Makes the changes you asked for, then reads the security settings
    /// of a few repositories that are due, unless that is already running.
    /// A tap meanwhile found it running, so its change is made before this
    /// stops.
    pub(super) async fn read_settings(&self) {
        loop {
            if self.reading_settings.swap(true, Ordering::SeqCst) {
                return;
            }
            self.change_settings().await;
            self.read_due_settings().await;
            self.reading_settings.store(false, Ordering::SeqCst);
            if !baseline::any_asked(&self.state.lock().await.repo_settings) {
                return;
            }
        }
    }
    /// Makes each change you asked for, one at a time. Each tap tries once:
    /// Orbit reads the repository's settings again, and only if the setting
    /// is still off does it record the change in the journal, save that,
    /// and send it. Then it reads them once more, to show what GitHub has
    /// now.
    async fn change_settings(&self) {
        let cut_off = {
            let mut s = self.state.lock().await;
            // Nothing else changes a setting while this runs, so a change
            // still marked as sending was cut off when Orbit stopped.
            let repos = baseline::interrupted(&mut s.journal, chrono::Utc::now());
            if !repos.is_empty() {
                self.dirty.store(true, Ordering::SeqCst);
            }
            repos
        };
        for repo in &cut_off {
            self.read_again(repo).await;
        }
        loop {
            let next = {
                let mut s = self.state.lock().await;
                if baseline::any_asked(&s.repo_settings) {
                    self.dirty.store(true, Ordering::SeqCst);
                }
                let watched = watched_settings(&s);
                baseline::next_ask(&mut s.repo_settings, &watched)
            };
            let Some((repo, setting)) = next else {
                break;
            };
            // Read first: GitHub may have it on already, or Orbit's
            // sign-in may no longer be an admin there.
            let look = baseline::read(&repo).await;
            let read = look.as_ref().ok().cloned();
            let sending = {
                let mut s = self.state.lock().await;
                let s = &mut *s;
                let now = chrono::Utc::now();
                // The switch, the baseline or the repository may have
                // changed meanwhile.
                let watched = watched_settings(s).iter().any(|r| same_repo(r, &repo));
                if watched {
                    baseline::record(&mut s.repo_settings, &repo, look, now);
                }
                let baselines = &s.settings.security_baselines;
                let fresh = watched && read.is_some();
                let send = baseline::take(&mut s.repo_settings, baselines, &repo, setting, fresh);
                self.dirty.store(true, Ordering::SeqCst);
                send.then(|| baseline::record_change(&mut s.journal, &repo, setting, now))
                    .zip(read)
            };
            let Some((id, read)) = sending else {
                continue;
            };
            // Saved before it is sent, so after a restart Orbit knows it may
            // have gone, and doesn't send it again.
            let sent = match self.flush().await {
                Ok(()) => Some(baseline::turn_on(&repo, setting, &read).await),
                Err(_) => None,
            };
            let limited = matches!(&sent, Some(Err(e)) if logs::rate_limited(e));
            let result = match sent {
                Some(sent) => sent.map_err(|e| baseline::change_error(&e)),
                None => Err("Orbit couldn't save its journal, so it didn't send this.".into()),
            };
            self.update(|s| baseline::settle(&mut s.journal, &id, result, chrono::Utc::now()))
                .await;
            // What GitHub has now: a change GitHub takes may still not
            // turn the setting on, such as where a plan doesn't offer it.
            if !limited {
                self.read_again(&repo).await;
            }
            let _ = self.flush().await;
        }
    }
    /// Reads a repository's settings again, if Orbit still watches them.
    async fn read_again(&self, repo: &str) {
        let result = baseline::read(repo).await;
        self.update(|s| {
            if watched_settings(s).iter().any(|r| same_repo(r, repo)) {
                baseline::record(&mut s.repo_settings, repo, result, chrono::Utc::now());
            }
        })
        .await;
    }
    /// Reads the security settings of a few connected repositories whose
    /// owner's baseline asks for any, with Watch security alerts on, and
    /// forgets what it no longer watches.
    async fn read_due_settings(&self) {
        let now = chrono::Utc::now();
        let (watch, stale, wanted) = {
            let s = self.state.lock().await;
            let watched = watched_settings(&s);
            let stale = s
                .repo_settings
                .iter()
                .any(|r| !watched.iter().any(|w| same_repo(w, &r.repo)));
            let wanted = baseline::wanted(&watched, &s.repo_settings, now);
            (s.settings.watch_security, stale, wanted)
        };
        if stale {
            self.update(|s| {
                let watched = watched_settings(s);
                baseline::prune(&mut s.repo_settings, &watched);
            })
            .await;
        }
        for repo in &wanted {
            let result = baseline::read(repo).await;
            let limited = result.as_ref().is_err_and(|e| logs::rate_limited(e));
            self.update(|s| {
                // The switch, the baseline or the repository changed
                // meanwhile.
                if watched_settings(s).iter().any(|r| same_repo(r, repo)) {
                    baseline::record(&mut s.repo_settings, repo, result, chrono::Utc::now());
                }
            })
            .await;
            if limited {
                break;
            }
        }
        if watch || stale {
            let _ = self.flush().await;
        }
    }
}

/// The connected repositories whose security settings Orbit reads: with
/// Watch security alerts on, those whose owner's baseline asks for any.
fn watched_settings(s: &Snapshot) -> Vec<String> {
    if !s.settings.watch_security {
        return vec![];
    }
    repos_of(s)
        .into_iter()
        .filter(|r| {
            !baseline::wanted_for(&s.settings.security_baselines, baseline::owner(r)).is_empty()
        })
        .collect()
}
