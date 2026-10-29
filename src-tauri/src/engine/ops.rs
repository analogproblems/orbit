//! Telling agents about failing CI: after each CI read, Orbit reads the
//! agents' answers on its comments and issues, checks the fixes they name
//! against CI, then posts, updates and closes its comments on pull requests
//! and its issues about default branches as their problems need (see
//! `crate::ops`). Each write is recorded in the journal, and the journal
//! saved, before it is sent.
use super::{claims, Engine};
use crate::{
    ci::CiState,
    github,
    ops::{self, Action, Event, FixCheck, Planned, Posted},
    types::{trusted_source, Snapshot},
};
use serde_json::Value;
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
        let mut events = self.read_answers().await;
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
            if sent {
                match planned.action {
                    Action::PostComment | Action::OpenIssue => {
                        events.push(Event::Posted(planned.thread.clone()))
                    }
                    Action::CloseIssue => events.push(Event::Closed(planned.thread.clone())),
                    _ => {}
                }
            }
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
        if !events.is_empty() {
            self.update(|s| {
                crate::buddy::observe_ci(s, &events);
            })
            .await;
        }
        let _ = self.flush().await;
    }
    /// Reads the replies on each comment or issue about an open problem,
    /// from the accounts Orbit trusts, then checks the fixes they name
    /// against CI. Returns what Orbit heard, for Bit's notes. A read GitHub
    /// doesn't answer is tried again on the next pass.
    async fn read_answers(&self) -> Vec<Event> {
        struct Ask {
            key: String,
            repo: String,
            number: u64,
            since: String,
            page: u32,
            /// The pull request's author, when Orbit trusts its source.
            author: Option<String>,
        }
        let (asks, me, trusted) = {
            let s = self.state.lock().await;
            let open = |t: &ops::Thread| {
                s.problems
                    .iter()
                    .any(|p| p.id == t.problem_id && p.is_open())
            };
            let asks: Vec<Ask> = s
                .ops
                .iter()
                .filter(|t| t.is_posted() && !t.gone && !t.closed && open(t))
                .filter_map(|t| {
                    Some(Ask {
                        key: t.key.clone(),
                        repo: t.repo.clone(),
                        number: t.number?,
                        since: t.read_since.clone(),
                        page: t.read_page,
                        author: t
                            .pr_id
                            .as_deref()
                            .and_then(|id| s.prs.iter().find(|p| p.id == id))
                            .filter(|p| trusted_source(p, &s.settings.trusted_authors))
                            .map(|p| p.author.clone()),
                    })
                })
                .collect();
            (
                asks,
                s.github_user.clone(),
                s.settings.trusted_authors.clone(),
            )
        };
        let Some(me) = me else {
            return vec![];
        };
        let mut heard = vec![];
        for ask in asks {
            let read = github::replies(&ask.repo, ask.number, &ask.since, ask.page).await;
            let Ok((comments, more)) = read else {
                continue;
            };
            let trusts = |login: &str| {
                login.eq_ignore_ascii_case(&me)
                    || trusted.iter().any(|a| a.eq_ignore_ascii_case(login))
                    || ask
                        .author
                        .as_deref()
                        .is_some_and(|a| a.eq_ignore_ascii_case(login))
            };
            let replies: Vec<ops::Reply> = comments
                .iter()
                .filter_map(|c| reply(c, &me, &trusts))
                .collect();
            self.update(|s| {
                if let Some(t) = s.ops.iter_mut().find(|t| t.key == ask.key) {
                    heard.extend(ops::hear(t, &replies, more));
                }
            })
            .await;
        }
        let checks = {
            let s = self.state.lock().await;
            ops::to_verify(&s.ops, &s.problems)
        };
        // The ones tried longest ago first, so none waits on the others.
        for v in checks.into_iter().take(ops::CHECKS_PER_PASS) {
            let check = verify(&v).await;
            self.update(|s| heard.extend(ops::verified(&mut s.ops, &v, check, chrono::Utc::now())))
                .await;
        }
        heard
            .into_iter()
            .map(|h| Event::Heard(Box::new(h)))
            .collect()
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

/// A comment as a reply Orbit reads: what it says about items, when it is
/// from an account Orbit trusts and isn't one of Orbit's own.
fn reply(c: &Value, me: &str, trusts: &impl Fn(&str) -> bool) -> Option<ops::Reply> {
    let text = |key: &str| c[key].as_str().unwrap_or_default().to_string();
    let author = c["user"]["login"].as_str().unwrap_or_default();
    let body = c["body"].as_str().unwrap_or_default();
    let own = author.eq_ignore_ascii_case(me) && body.contains("<!-- orbit-");
    Some(ops::Reply {
        id: c["id"].as_u64()?,
        author: author.to_string(),
        created_at: text("created_at"),
        updated_at: text("updated_at"),
        url: text("html_url"),
        said: if trusts(author) && !own {
            claims::ci_lines(body)
        } else {
            vec![]
        },
    })
}
/// What CI says at the commit a fix names: whether it is on the branch,
/// then whether the item's check passes there.
async fn verify(v: &ops::Verify) -> Result<FixCheck, String> {
    let Some((status, full)) = github::compare(&v.repo, &v.commit, &v.head).await? else {
        return Ok(FixCheck::NotFound);
    };
    match status.as_str() {
        "identical" | "ahead" => {}
        // Newer than the commit Orbit last read: checked after the next read.
        "behind" => return Ok(FixCheck::Waiting),
        _ => return Ok(FixCheck::NotOnBranch),
    }
    Ok(match crate::ci::check_at(&v.repo, &full, &v.check).await? {
        CiState::Passing => FixCheck::Passes,
        CiState::Failing => FixCheck::Fails,
        CiState::Running | CiState::None => FixCheck::Waiting,
    })
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
