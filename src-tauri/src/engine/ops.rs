//! Telling agents about failing CI: after each CI read, Orbit's comments on
//! pull requests and its issues about default branches are posted, kept up
//! to date and closed as their problems need (see `crate::ops`). Each write
//! is recorded in the journal, and the journal saved, before it is sent.
use super::Engine;
use crate::{
    github,
    ops::{self, Action, Planned, Posted},
    types::Snapshot,
};
use std::sync::{atomic::Ordering, Arc};

/// The next thing any thread needs, as things stand now.
fn plan(s: &Snapshot, now: chrono::DateTime<chrono::Utc>) -> Option<Planned> {
    s.ops.iter().find_map(|t| {
        let problem = s.problems.iter().find(|p| p.id == t.problem_id);
        ops::next(t, problem, ops::may_post(s, t), now)
    })
}

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
            let mut changed = ops::interrupted(&mut s.journal, &mut s.ops, now);
            changed |= ops::refresh(&mut s.ops, &s.problems, now);
            if changed {
                self.dirty.store(true, Ordering::SeqCst);
            }
        }
        for _ in 0..ops::WRITES_PER_PASS {
            let next = {
                let s = self.state.lock().await;
                // Orbit's sign-in is how it finds its own comments again.
                let login = s.github_user.clone();
                plan(&s, chrono::Utc::now()).zip(login)
            };
            let Some((planned, login)) = next else {
                break;
            };
            if ops::looks_first(planned.action) {
                // An earlier try may have got there before Orbit heard back.
                let found = find(&planned, &login).await;
                if !matches!(found, Ok(None)) || planned.action == Action::Find {
                    let failed = found.is_err();
                    self.update(|s| ops::looked(&mut s.ops, &planned, found, chrono::Utc::now()))
                        .await;
                    if self.flush().await.is_err() || failed {
                        break;
                    }
                    continue;
                }
                // Nothing there: post it only if it is still wanted now the
                // search is done, as rules and settings may have changed.
                let wanted = {
                    let s = self.state.lock().await;
                    plan(&s, chrono::Utc::now()).as_ref() == Some(&planned)
                };
                if !wanted {
                    continue;
                }
            }
            let id = {
                let mut s = self.state.lock().await;
                ops::record(&mut s.journal, &planned, chrono::Utc::now())
            };
            self.dirty.store(true, Ordering::SeqCst);
            // Saved before it is sent, so after a restart Orbit knows it may
            // have gone, and looks before sending it again.
            let result = match self.flush().await {
                Ok(()) => send(&planned).await,
                Err(_) => Err("Orbit couldn't save its journal, so it didn't send this.".into()),
            };
            let sent = result.is_ok();
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
            if self.flush().await.is_err() || !sent {
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

/// Looks on GitHub for what a write would make: the thread's comment or
/// issue, or an issue's closing note.
async fn find(p: &Planned, login: &str) -> Result<Option<Posted>, String> {
    let comment = match p.action {
        Action::Find => p.thread.starts_with("pr:"),
        action => matches!(action, Action::PostComment | Action::NoteOnIssue),
    };
    if comment {
        let number = p
            .number
            .ok_or("Orbit doesn't know where this is on GitHub.")?;
        github::find_comment(&p.repo, number, &p.marker, login).await
    } else {
        github::find_issue(&p.repo, &p.marker, login).await
    }
}

/// Sends one write.
async fn send(p: &Planned) -> Result<Posted, String> {
    let known =
        |n: Option<u64>| n.ok_or_else(|| "Orbit doesn't know where this is on GitHub.".to_string());
    match p.action {
        Action::Find => Err("Looking isn't a write.".into()),
        Action::PostComment | Action::NoteOnIssue => {
            github::post_comment(&p.repo, known(p.number)?, &p.body, &p.marker).await
        }
        Action::EditComment => {
            github::edit_comment(&p.repo, known(p.comment_id)?, &p.body, &p.marker).await
        }
        Action::OpenIssue => {
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
    }
}
