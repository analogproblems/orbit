//! Telling agents about failing CI, and whether each pull request Orbit
//! reviews is clear to merge: after each CI read, Orbit reads the agents'
//! answers on its comments and issues, checks the fixes they name against
//! CI, reruns jobs that failed by bad luck (`crate::reruns`), then posts,
//! updates and closes its comments on pull requests and its issues about
//! default branches as their problems and answers need (see `crate::ops`).
//! Each write is recorded in the journal, and the journal saved, before it
//! is sent.
use super::{claims, Engine};
use crate::{
    ci::CiState,
    github,
    ops::{self, Action, Event, FixCheck, Planned, Posted},
    reruns::{self, Look},
    types::{trusted_source, Snapshot},
};
use serde_json::Value;
use std::sync::{atomic::Ordering, Arc};

/// The next thing any thread needs, as things stand now: reruns first, so
/// what Orbit posts says it reran a job. Nothing is planned on a pull
/// request Orbit stepped back from, as another Orbit posts there
/// (`crate::orbits`).
fn plan(s: &Snapshot, now: chrono::DateTime<chrono::Utc>) -> Option<Planned> {
    let held = |key: &str| {
        s.ops.iter().any(|t| {
            t.key == key
                && t.pr_id
                    .as_deref()
                    .is_some_and(|pr| crate::orbits::stepped_back(&s.posting, pr))
        })
    };
    let mut reruns = reruns::all(s, now).into_iter();
    reruns.find(|p| !held(&p.thread)).or_else(|| {
        s.ops.iter().filter(|t| !held(&t.key)).find_map(|t| {
            let problem = ops::problem_of(t, &s.problems, &s.ci);
            ops::next(t, problem, ops::may_post(s, t), now)
        })
    })
}
/// Whether `planned` is still what its thread needs, by everything as it
/// stands now: the rule for it, Bit, the switches, the repository and its
/// problem, as `next` weighs them.
fn still_next(s: &Snapshot, planned: &Planned, now: chrono::DateTime<chrono::Utc>) -> bool {
    s.ops
        .iter()
        .find(|t| t.key == planned.thread)
        .is_some_and(|t| {
            let problem = ops::problem_of(t, &s.problems, &s.ci);
            ops::next(t, problem, ops::may_post(s, t), now).as_ref() == Some(planned)
        })
}
/// The pull request a write is on, as its id and number: a comment on it,
/// or a rerun of one of its jobs.
fn on_pr(s: &Snapshot, planned: &Planned) -> Option<(String, u64)> {
    let t = s.ops.iter().find(|t| t.key == planned.thread)?;
    Some((t.pr_id.clone()?, t.number?))
}
/// Works out each pull request's answer to whether it is clear to merge
/// again, from things as they are now, so a write planned before Orbit
/// waited on GitHub never sends an answer that has gone stale meanwhile.
/// Returns whether anything changed.
fn ready_now(s: &mut Snapshot, now: chrono::DateTime<chrono::Utc>) -> bool {
    let answers = crate::readiness::all(s);
    let watch = s.settings.watch_ci;
    let ready = ops::ready(&mut s.ops, &answers, &s.prs, &s.problems, &s.ci, watch, now);
    // A quick call decides by the settings now, not as they were when the
    // pass began.
    decide(s) || ready
}
/// Gives each failing item why its job failed, as a quick call that
/// decides says (`crate::calls::decided`). Says whether any changed.
pub(super) fn decide(s: &mut Snapshot) -> bool {
    let read = crate::calls::decided(s);
    ops::explain(&mut s.ops, &read)
}

impl Engine {
    /// Takes the second look a new post needs before it goes, if any
    /// (`crate::looks`): asks the model without the state lock, then keeps
    /// its answers only while the post and the look's setup are as they
    /// were. Says whether the post may go on now, and Bit's note when the
    /// look holds it for you.
    async fn second_look(&self, planned: &Planned) -> (bool, Option<Event>) {
        let wanted = {
            let s = self.state.lock().await;
            crate::looks::wanted(&s, planned, chrono::Utc::now())
        };
        let Some(w) = wanted else {
            return (true, None);
        };
        let answers = match &w.prompt {
            Some(prompt) => self
                .look_answer(&w, prompt)
                .await
                .and_then(|content| crate::looks::parse(&content)),
            None => Err(w.why.clone()),
        };
        let mut out = (false, None);
        self.update(|s| {
            let now = chrono::Utc::now();
            let same = crate::looks::wanted(s, planned, now).is_some_and(|now| now.same(&w));
            if !same || !still_next(s, planned, now) {
                return;
            }
            if crate::looks::record(s, &w, answers, now) {
                let why = s
                    .looks
                    .iter()
                    .find(|l| l.thread == w.thread)
                    .map(|l| l.why.clone())
                    .unwrap_or_default();
                out.1 = Some(Event::Held {
                    thread: w.thread.clone(),
                    why,
                });
            }
            out.0 = still_next(s, planned, now);
        })
        .await;
        if self.flush().await.is_err() {
            return (false, out.1);
        }
        out
    }
    /// Asks whoever takes a second look (`crate::looks::who`) for its
    /// answers, as text: Ollama, or Bit's answerer, each answering only in
    /// `looks::schema` with no tools, as Bit's own questions are.
    async fn look_answer(&self, w: &crate::looks::Wanted, prompt: &str) -> Result<String, String> {
        use crate::{buddy::Answerer, looks, types::Provider};
        let schema = looks::schema();
        let instructions = looks::INSTRUCTIONS;
        let model = &w.selection.model;
        match w.by {
            looks::Who::Quick | looks::Who::Bit(Answerer::Ollama) => {
                crate::ollama::answer(&w.url, model, instructions, prompt, &schema, looks::TIMEOUT)
                    .await
            }
            looks::Who::Bit(Answerer::Codex) => {
                self.codex_once(
                    super::writeup::Once::Look,
                    instructions,
                    prompt,
                    &schema,
                    &w.selection,
                    looks::BIT_TIMEOUT,
                )
                .await
            }
            looks::Who::Bit(Answerer::Api(api)) => {
                let setup = self.apis.lock().unwrap().clone();
                let key = setup.key(api).await?.ok_or_else(|| {
                    format!(
                        "Your {} API key isn't in your password store any more.",
                        api.company()
                    )
                })?;
                crate::api::answer(
                    api,
                    setup.base(api),
                    &key,
                    model,
                    instructions,
                    prompt,
                    &schema,
                    looks::BIT_TIMEOUT,
                )
                .await
            }
            looks::Who::Bit(a) => {
                let by = a.provider().unwrap_or(Provider::Claude);
                self.cli_once(
                    by,
                    prompt,
                    instructions,
                    &schema,
                    &w.selection,
                    looks::BIT_TIMEOUT,
                )
                .await
            }
        }
    }
    /// Whether `planned` is still the next write once Orbit has looked on
    /// GitHub, as rules, settings and answers may have changed meanwhile.
    async fn still_wanted(&self, planned: &Planned) -> bool {
        let mut s = self.state.lock().await;
        let now = chrono::Utc::now();
        if ready_now(&mut s, now) {
            self.dirty.store(true, Ordering::SeqCst);
        }
        plan(&s, now).as_ref() == Some(planned)
    }
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
        let mut events = {
            let mut s = self.state.lock().await;
            let s = &mut *s;
            let now = chrono::Utc::now();
            // Nothing is on its way while this runs, so a write still marked
            // as sending was cut off when Orbit stopped.
            let mut changed = ops::interrupted(&mut s.journal, &mut s.ops, now);
            // Before the threads change, so a problem that closed is kept
            // as its thread last saw it, and a new one says what happened
            // last time before Orbit first tells its agent.
            let (followed, learned) = crate::cases::follow(s, now);
            changed |= followed;
            changed |= crate::playbooks::follow(s);
            changed |= crate::buddy::observe_lessons(s, &learned);
            changed |= ops::refresh(&mut s.ops, &s.problems, now);
            changed |= crate::looks::follow(s);
            // As a quick call that decides says, while it does.
            changed |= decide(s);
            let mut again = reruns::noticed(s);
            again.extend(ops::offer(s));
            changed |= !again.is_empty();
            if changed {
                self.dirty.store(true, Ordering::SeqCst);
            }
            again
        };
        events.extend(self.read_answers().await);
        for _ in 0..ops::WRITES_PER_PASS {
            let next = {
                let mut s = self.state.lock().await;
                let now = chrono::Utc::now();
                // Here rather than first, as reading answers or the write
                // before may have taken a while. Then what each look's post
                // has to say, as a pull request's answer may have changed.
                let ready = ready_now(&mut s, now);
                if crate::looks::follow(&mut s) || ready {
                    self.dirty.store(true, Ordering::SeqCst);
                }
                // Orbit's sign-in is how it finds its own comments again.
                let login = s.github_user.clone();
                plan(&s, now)
                    .zip(login)
                    .map(|(p, login)| (on_pr(&s, &p), s.instance.name.clone(), p, login))
            };
            let Some((pr, me, planned, login)) = next else {
                break;
            };
            if let Some(job) = &planned.rerun {
                // An earlier try may have rerun it before Orbit heard back,
                // and GitHub reruns a job only once its workflow is done.
                let look = reruns::look(&planned.repo, job).await;
                if job.unsure || look != Ok(Look::Ready) {
                    let failed = look.is_err();
                    self.update(|s| {
                        let now = chrono::Utc::now();
                        events.extend(reruns::looked(&mut s.ops, &planned, look, now));
                    })
                    .await;
                    if self.flush().await.is_err() || failed {
                        break;
                    }
                    continue;
                }
                if !self.still_wanted(&planned).await {
                    continue;
                }
            } else if ops::looks_first(planned.action) {
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
                if !self.still_wanted(&planned).await {
                    continue;
                }
                // Something new where you don't run things gets a second
                // look first, which may hold it for you.
                let (go, held) = self.second_look(&planned).await;
                events.extend(held);
                if !go {
                    continue;
                }
            }
            // Last, just before the write: not where another Orbit posted
            // later, which leaves this pull request out from now on.
            if let Some((pr_id, number)) = &pr {
                let clear = match self.clear_to_post(&planned.repo, *number, pr_id).await {
                    Ok(clear) => clear,
                    // GitHub couldn't say. Editing a comment GitHub shows
                    // this Orbit wrote last keeps what's on GitHub true,
                    // such as taking back a clear to merge. Anything new,
                    // another Orbit's comment, or anything once what was
                    // read shows another Orbit posted later, waits.
                    Err(unsure) => {
                        let own = match (planned.action, planned.comment_id) {
                            (Action::EditComment, Some(id)) if !unsure.behind => {
                                self.own_comment(&planned.repo, id, &planned.marker).await
                            }
                            _ => false,
                        };
                        if !own {
                            break;
                        }
                        true
                    }
                };
                if !clear || !self.still_wanted(&planned).await {
                    continue;
                }
            }
            // Last, just before the write: nothing about security alerts
            // or risky workflow settings goes to GitHub unless GitHub says
            // now that the repository is private, whatever Orbit read
            // before.
            let security = ops::sensitive(&*self.state.lock().await, &planned.thread);
            if security {
                match crate::security::private_now(&planned.repo).await {
                    Ok(true) => {}
                    Ok(false) => {
                        self.update(|s| {
                            let repo = &planned.repo;
                            crate::problems::see_private(
                                &mut s.problems,
                                &mut s.security,
                                &mut s.setups,
                                repo,
                                false,
                            );
                        })
                        .await;
                        continue;
                    }
                    // GitHub couldn't say, so it waits for a later pass.
                    Err(_) => break,
                }
            }
            let id = {
                let mut s = self.state.lock().await;
                let s = &mut *s;
                // Under the lock it is recorded in, as while Orbit asked
                // GitHub, a read may have found the repository isn't private
                // any more, or you may have changed a rule or a switch.
                if security && !still_next(s, &planned, chrono::Utc::now()) {
                    continue;
                }
                let id = ops::record(&mut s.journal, &planned, chrono::Utc::now());
                reruns::sending(&mut s.ops, &mut s.journal, &id, &planned);
                id
            };
            self.dirty.store(true, Ordering::SeqCst);
            // Saved before it is sent, so after a restart Orbit knows it may
            // have gone, and looks before sending it again.
            let result = match self.flush().await {
                Ok(()) => send(&planned, &me).await,
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
                let now = chrono::Utc::now();
                if planned.rerun.is_some() {
                    let reran =
                        reruns::settle(&mut s.ops, &mut s.journal, &id, &planned, result, now);
                    events.extend(reran);
                } else {
                    ops::settle(&mut s.ops, &mut s.journal, &id, &planned, result, now);
                    // A post a second look had shows as posted at once.
                    crate::looks::follow(s);
                }
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
            /// Posted before Orbit read answers, so when GitHub showed its
            /// items is read from its copy there first.
            unshown: bool,
            comment: Option<u64>,
            marker: String,
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
                        unshown: t.shown_at.is_empty(),
                        comment: t.comment_id,
                        marker: t.tag(),
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
            if ask.unshown {
                // No answer is read until Orbit knows which ones count.
                let copy = match ask.comment {
                    Some(id) => github::get_comment(&ask.repo, id, &ask.marker).await,
                    None => github::get_issue(&ask.repo, ask.number, &ask.marker).await,
                };
                let Ok(copy) = copy else {
                    continue;
                };
                self.update(|s| {
                    if let Some(t) = s.ops.iter_mut().find(|t| t.key == ask.key) {
                        ops::recover(t, &copy);
                    }
                })
                .await;
            }
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
    /// Lets Bit rerun an item's job it asks about first, and reruns it.
    pub async fn rerun_job(self: &Arc<Self>, key: &str, number: u32) -> Result<(), String> {
        self.allow_rerun(key, number).await?;
        let e = self.clone();
        tokio::spawn(async move { e.tell_agents().await });
        Ok(())
    }
    /// Lets Bit rerun an item's job it asks about first.
    pub(super) async fn allow_rerun(&self, key: &str, number: u32) -> Result<(), String> {
        let mut found = false;
        self.update(|s| {
            let item = s
                .ops
                .iter_mut()
                .find(|t| t.key == key)
                .and_then(|t| t.items.iter_mut().find(|i| i.number == number && i.failing));
            if let Some(item) = item {
                item.rerun_ok = item.url.clone();
                // Tried now, rather than after the wait.
                let url = &item.url;
                for r in item.reruns.iter_mut().filter(|r| &r.url == url) {
                    r.retry_at = None;
                }
                found = true;
            }
        })
        .await;
        if !found {
            return Err("Orbit has nothing to rerun there any more.".into());
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

/// Sends one write. A comment on a pull request is signed as this
/// Orbit's (`me`), so another Orbit there steps back; an issue isn't, so
/// two Orbits never edit one back and forth.
async fn send(p: &Planned, me: &str) -> Result<Posted, String> {
    let known =
        |n: Option<u64>| n.ok_or_else(|| "Orbit doesn't know where this is on GitHub.".to_string());
    match p.action {
        Action::Find => Err("Looking isn't a write.".into()),
        Action::ChangeSetting => Err("Orbit changes a setting only on your tap.".into()),
        Action::AskRebase => Err("Orbit asks Dependabot to rebase only on your tap.".into()),
        Action::PushTag => Err("Orbit pushes a release tag only through its own check.".into()),
        Action::PublishRelease => {
            Err("Orbit publishes a release only through its own check.".into())
        }
        Action::RerunJob => reruns::send(p).await,
        Action::PostComment => {
            let body = crate::orbits::signed(&p.body, me);
            github::post_comment(&p.repo, known(p.number)?, &body, &p.marker).await
        }
        Action::NoteOnIssue => {
            github::post_comment(&p.repo, known(p.number)?, &p.body, &p.marker).await
        }
        Action::EditComment => {
            let body = crate::orbits::signed(&p.body, me);
            github::edit_comment(&p.repo, known(p.comment_id)?, &body, &p.marker).await
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
