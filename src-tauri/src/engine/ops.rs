//! Telling agents about failing CI: after each CI read, Orbit's comments on
//! pull requests and its issues about default branches are posted, kept up
//! to date and closed as their problems need (see `crate::ops`). Each write
//! is recorded in the journal, and the journal saved, before it is sent.
use super::Engine;
use crate::{
    github,
    ops::{self, Action, Planned, Posted, ThreadKind},
};
use std::sync::{atomic::Ordering, Arc};

impl Engine {
    /// Brings Orbit's comments and issues up to date, one write at a time,
    /// unless that is already running.
    pub async fn tell_agents(&self) {
        if self.telling.swap(true, Ordering::SeqCst) {
            return;
        }
        self.tell_agents_now().await;
        self.telling.store(false, Ordering::SeqCst);
    }
    async fn tell_agents_now(&self) {
        {
            let mut s = self.state.lock().await;
            let s = &mut *s;
            let now = chrono::Utc::now();
            // Nothing is on its way while this runs, so a write still marked
            // as sending was cut off when Orbit stopped.
            let mut changed = ops::interrupted(&mut s.journal, now);
            changed |= ops::refresh(&mut s.ops, &s.problems, now);
            if changed {
                self.dirty.store(true, Ordering::SeqCst);
            }
        }
        for _ in 0..ops::WRITES_PER_PASS {
            let next = {
                let mut s = self.state.lock().await;
                let s = &mut *s;
                let now = chrono::Utc::now();
                // Orbit's sign-in is how it finds its own comments again.
                let Some(login) = s.github_user.clone() else {
                    break;
                };
                let rules = &s.settings.buddy.rules;
                let planned = s.ops.iter().find_map(|t| {
                    let rule = match t.kind {
                        ThreadKind::Comment => &rules.comment_on_prs,
                        ThreadKind::Issue => &rules.open_issues,
                    };
                    let may = ops::may_post(rule, t.allowed, s.settings.buddy.enabled);
                    let problem = s.problems.iter().find(|p| p.id == t.problem_id);
                    ops::next(t, problem, may, now)
                });
                planned.map(|planned| {
                    let id = ops::record(&mut s.journal, &planned, now);
                    self.dirty.store(true, Ordering::SeqCst);
                    (planned, id, login)
                })
            };
            let Some((planned, id, login)) = next else {
                break;
            };
            // Saved before it is sent, so after a restart Orbit knows it may
            // have gone, and looks before sending it again.
            let result = match self.flush().await {
                Ok(()) => send(&planned, &login).await,
                Err(_) => Err("Orbit couldn't save its journal, so it didn't send this.".into()),
            };
            let saved = result.is_ok();
            self.update(|s| {
                ops::settle(
                    &mut s.ops,
                    &mut s.journal,
                    &id,
                    &planned,
                    result,
                    chrono::Utc::now(),
                )
            })
            .await;
            if self.flush().await.is_err() || !saved {
                break;
            }
        }
        let _ = self.flush().await;
    }
    /// Lets Bit post a comment or issue it asks about first, and posts it.
    pub async fn tell_agent(self: &Arc<Self>, key: &str) -> Result<(), String> {
        self.allow_telling(key).await?;
        let e = self.clone();
        tokio::spawn(async move { e.tell_agents().await });
        Ok(())
    }
    /// Lets Bit post a comment or issue it asks about first.
    pub(super) async fn allow_telling(&self, key: &str) -> Result<(), String> {
        let mut found = false;
        self.update(|s| {
            if let Some(t) = s.ops.iter_mut().find(|t| t.key == key) {
                t.allowed = true;
                t.retry_at = None;
                found = true;
            }
        })
        .await;
        if !found {
            return Err("Orbit has nothing to tell about that any more.".into());
        }
        self.flush().await
    }
}

/// Sends one write. Before posting something new, Orbit looks for it on
/// GitHub, in case an earlier try got there before Orbit heard back.
async fn send(p: &Planned, login: &str) -> Result<Posted, String> {
    let known =
        |n: Option<u64>| n.ok_or_else(|| "Orbit doesn't know where this is on GitHub.".to_string());
    match p.action {
        Action::PostComment => {
            let number = known(p.number)?;
            if let Some(found) = github::find_comment(&p.repo, number, &p.marker, login).await? {
                return github::edit_comment(&p.repo, found.id, &p.body, &p.marker).await;
            }
            github::post_comment(&p.repo, number, &p.body, &p.marker).await
        }
        Action::EditComment => {
            github::edit_comment(&p.repo, known(p.comment_id)?, &p.body, &p.marker).await
        }
        Action::OpenIssue => {
            if let Some(found) = github::find_issue(&p.repo, &p.marker, login).await? {
                let number = known(found.number)?;
                return github::edit_issue(&p.repo, number, &p.body, &p.marker, None).await;
            }
            github::open_issue(&p.repo, &p.title, &p.body, ops::LABEL, &p.marker).await
        }
        Action::EditIssue => {
            github::edit_issue(&p.repo, known(p.number)?, &p.body, &p.marker, None).await
        }
        Action::ReopenIssue => {
            let state = Some(("open", "reopened"));
            github::edit_issue(&p.repo, known(p.number)?, &p.body, &p.marker, state).await
        }
        Action::CloseIssue => {
            let reason = if p.completed {
                "completed"
            } else {
                "not_planned"
            };
            let state = Some(("closed", reason));
            github::edit_issue(&p.repo, known(p.number)?, &p.body, &p.marker, state).await
        }
        Action::NoteOnIssue => {
            let number = known(p.number)?;
            if let Some(found) = github::find_comment(&p.repo, number, &p.marker, login).await? {
                return Ok(found);
            }
            github::post_comment(&p.repo, number, &p.body, &p.marker).await
        }
    }
}
