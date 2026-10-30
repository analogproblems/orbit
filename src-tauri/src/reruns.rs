//! Rerunning a failed job that looks like bad luck rather than the code
//! (`crate::logs::flaky`), such as a lost runner or a network error. Bit's
//! rule Rerun a failed job once when it looks flaky covers the first rerun
//! of a check at a commit, and Rerun the same job a second time the second;
//! there is never a third. A job that fails again after a rerun is likely
//! real, and Orbit's comment or issue says so.
//!
//! A rerun is a write on GitHub like Orbit's comments and issues
//! (`crate::ops`): recorded in the journal, and on its item as unsure, and
//! saved before it is sent. A rerun Orbit didn't hear back about is looked
//! for on GitHub before anything else is rerun for that check, so a restart
//! never turns it into an extra one. Before each rerun Orbit also looks at
//! the job on GitHub: it must be that check's job at that commit, a job an
//! earlier try already reran isn't rerun again, and a job whose workflow is
//! still running waits for it to finish.
use crate::{
    logs,
    ops::{self, Action, Event, Item, Planned, Thread, ThreadKind, Write, WriteState},
    problems::{stamp, time, Problem},
    types::{same_repo, Snapshot},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most times Orbit reruns one check at one commit.
pub const MAX_RERUNS: usize = 2;
/// The most pages of a workflow run's jobs Orbit reads, 100 to a page.
pub const MAX_PAGES: u32 = 10;

/// One rerun of an item's job, or Orbit's tries at one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rerun {
    /// The job Orbit reran, by its link.
    pub url: String,
    /// Why it looked like bad luck.
    pub reason: String,
    /// When GitHub took it, or Orbit found it already rerun.
    pub at: String,
    pub done: bool,
    /// Orbit found it already rerun on GitHub, by an earlier try or by
    /// someone else.
    pub already: bool,
    /// Why the latest try didn't go through, how many in a row haven't, and
    /// when Orbit tries again.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    /// GitHub won't rerun it however often Orbit asks, so it doesn't.
    pub lasting: bool,
    /// Orbit saw the job fail again after this rerun.
    pub failed_again: bool,
    /// Orbit asked GitHub to rerun it and stopped, or didn't hear back,
    /// before GitHub answered, so it may have gone through. Orbit looks on
    /// GitHub before it reruns anything else for this check.
    pub unsure: bool,
}

/// A rerun Orbit plans: which item's job, its check, at which commit, and
/// why it looks like bad luck. `unsure` only looks whether an earlier try
/// went through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub item: u32,
    pub url: String,
    pub check: String,
    pub sha: String,
    pub reason: String,
    pub unsure: bool,
}

/// The reruns that went through at the item's commit.
pub fn done(item: &Item) -> usize {
    item.reruns.iter().filter(|r| r.done).count()
}
/// Why an item's job looks like bad luck, when Orbit may still rerun it:
/// it fails, Orbit read its log, it is a GitHub Actions job in `repo`,
/// Orbit hasn't rerun this job or given up on it, no rerun is unsure, and
/// fewer than `MAX_RERUNS` went through at this commit. With the reruns done
/// so far.
fn candidate<'a>(repo: &str, item: &'a Item) -> Option<(&'a str, usize)> {
    let why = item.why.as_ref().filter(|w| w.url == item.url)?;
    let reason = why.flaky.as_deref()?;
    logs::job_of(&item.url, repo)?;
    let tried = item.reruns.iter().find(|r| r.url == item.url);
    if !item.failing
        || tried.is_some_and(|r| r.done || r.lasting)
        || item.reruns.iter().any(|r| r.unsure)
    {
        return None;
    }
    let done = done(item);
    (done < MAX_RERUNS).then_some((reason, done))
}
/// Bit's rule for the next rerun of a check at a commit.
fn rule(rules: &crate::buddy::Rules, done: usize) -> &str {
    if done == 0 {
        &rules.rerun_flaky
    } else {
        &rules.rerun_again
    }
}
/// Whether the problem is about the latest commit Orbit knows of: a rerun
/// of an older one's job would test code that is gone.
fn current(s: &Snapshot, t: &Thread, p: &Problem) -> bool {
    match t.kind {
        ThreadKind::Comment => s
            .prs
            .iter()
            .any(|pr| t.pr_id.as_deref() == Some(pr.id.as_str()) && pr.head_sha == p.sha),
        ThreadKind::Issue => {
            s.ci.iter()
                .any(|c| same_repo(&c.repo, &t.repo) && c.head.sha == p.sha)
        }
    }
}

/// The reruns Orbit may send now, as writes: for each failing item on an
/// open problem about the latest commit that Orbit watches, whose job looks
/// like bad luck, when Bit's rule allows it and any wait is over. Before
/// those, whatever the rules, a look for each rerun Orbit is unsure about.
pub fn all(s: &Snapshot, now: DateTime<Utc>) -> Vec<Planned> {
    let due = |r: &Rerun| {
        !r.retry_at
            .as_deref()
            .and_then(time)
            .is_some_and(|at| now < at)
    };
    let plan = |t: &Thread, item: &Item, url: &str, reason: &str, unsure: bool| Planned {
        thread: t.key.clone(),
        action: Action::RerunJob,
        repo: t.repo.clone(),
        number: t.number,
        comment_id: None,
        title: String::new(),
        body: String::new(),
        marker: String::new(),
        completed: false,
        closed_at: None,
        rerun: Some(Job {
            item: item.number,
            url: url.to_string(),
            check: item.check.clone(),
            sha: item.at.clone(),
            reason: reason.to_string(),
            unsure,
        }),
    };
    let mut out = vec![];
    for t in s.ops.iter().filter(|t| !t.gone) {
        for item in &t.items {
            for r in item.reruns.iter().filter(|r| r.unsure && due(r)) {
                out.push(plan(t, item, &r.url, &r.reason, true));
            }
        }
    }
    for t in s.ops.iter().filter(|t| !t.gone) {
        let Some(p) = s
            .problems
            .iter()
            .find(|p| p.id == t.problem_id && p.is_open())
        else {
            continue;
        };
        if !current(s, t, p) || !ops::watching(s, t) {
            continue;
        }
        for item in &t.items {
            let Some((reason, done)) = candidate(&t.repo, item) else {
                continue;
            };
            let allowed = item.rerun_ok == item.url;
            let rule = rule(&s.settings.buddy.rules, done);
            if !ops::rule_allows(rule, allowed, s.settings.buddy.enabled) {
                continue;
            }
            if item.reruns.iter().any(|r| r.url == item.url && !due(r)) {
                continue;
            }
            out.push(plan(t, item, &item.url, reason, false));
        }
    }
    out
}

/// Notices each item whose job failed again after its latest rerun, once
/// Orbit has read the new job's log, and marks that rerun so. Returns them
/// for Bit's notes, with whether Bit asks before rerunning it again, unless
/// Orbit is about to rerun it again on its own.
pub fn noticed(s: &mut Snapshot) -> Vec<Event> {
    let mut events = vec![];
    let bit_on = s.settings.buddy.enabled;
    let rules = &s.settings.buddy.rules;
    for t in s.ops.iter_mut().filter(|t| !t.gone) {
        for item in t.items.iter_mut().filter(|i| i.failing) {
            let read = item.why.as_ref().is_some_and(|w| w.url == item.url);
            let Some(i) = item.reruns.iter().rposition(|r| r.done) else {
                continue;
            };
            let last = &item.reruns[i];
            if !read || last.url == item.url || last.failed_again {
                continue;
            }
            item.reruns[i].failed_again = true;
            let next = candidate(&t.repo, item).map(|(_, done)| rule(rules, done));
            let allowed = item.rerun_ok == item.url;
            if next.is_some_and(|rule| ops::rule_allows(rule, allowed, bit_on)) {
                continue;
            }
            events.push(Event::FailedAgain {
                thread: t.key.clone(),
                number: item.number,
                ask: bit_on && next == Some("ask_first"),
            });
        }
    }
    events
}

/// What GitHub shows of a job Orbit is about to rerun.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// It can be rerun now.
    Ready,
    /// A newer try of it is there already.
    Already,
    /// Its workflow is still running, and GitHub reruns a job only once
    /// its workflow has finished.
    Running,
    /// It isn't the check's job at the commit Orbit read, such as a status
    /// that links to some other job, so it isn't Orbit's to rerun.
    Elsewhere,
}
/// Looks at a job on GitHub before Orbit reruns it: the job must be the
/// check's job at the commit, and its workflow's latest jobs, every page of
/// them, say whether it was rerun already or is still running.
pub async fn look(repo: &str, job: &Job) -> Result<Look, String> {
    crate::types::validate_repo(repo)?;
    let id = logs::job_of(&job.url, repo).ok_or("Orbit can only rerun GitHub Actions jobs.")?;
    let v =
        crate::github::api(&format!("repos/{repo}/actions/jobs/{id}"), "GET", &[], None).await?;
    if !ours(&v, job) {
        return Ok(Look::Elsewhere);
    }
    let run = v["run_id"]
        .as_u64()
        .ok_or("GitHub didn't say which workflow run this job is in.")?;
    let mut jobs: Vec<Value> = vec![];
    for page in 1..=MAX_PAGES {
        let fields = [
            ("filter", "latest".to_string()),
            ("per_page", "100".into()),
            ("page", page.to_string()),
        ];
        let list = crate::github::api(
            &format!("repos/{repo}/actions/runs/{run}/jobs"),
            "GET",
            &fields,
            None,
        )
        .await?;
        let (some, total) = page_of(&list)?;
        jobs.extend(some.iter().cloned());
        if jobs.len() as u64 >= total {
            return Ok(looked_at(&v, &jobs));
        }
        if some.is_empty() {
            return Err("GitHub's list of the run's jobs ended early.".into());
        }
    }
    Err("The run has too many jobs for Orbit to check them all.".into())
}
/// One page of a run's jobs and how many there are in all, or an error when
/// GitHub's answer doesn't say, so nothing is taken as safe from a list
/// Orbit couldn't read.
pub fn page_of(list: &Value) -> Result<(&[Value], u64), String> {
    let jobs = list["jobs"].as_array();
    let total = list["total_count"].as_u64();
    match (jobs, total) {
        (Some(jobs), Some(total)) => Ok((jobs, total)),
        _ => Err("GitHub's list of the run's jobs didn't say what it holds.".into()),
    }
}
/// Whether GitHub's job is the check Orbit plans to rerun: at the commit
/// Orbit read (a pull request's head, for its runs), and named as the check
/// is, alone or after its workflow.
pub fn ours(v: &Value, job: &Job) -> bool {
    let name = v["name"].as_str().unwrap_or_default();
    v["head_sha"] == job.sha.as_str()
        && !name.is_empty()
        && (job.check == name || job.check.ends_with(&format!(" / {name}")))
}
/// What a job and its workflow's latest jobs say, as `look` reads them.
pub fn looked_at(job: &Value, jobs: &[Value]) -> Look {
    let attempt = |j: &Value| j["run_attempt"].as_u64().unwrap_or(1);
    if jobs
        .iter()
        .any(|j| j["name"] == job["name"] && attempt(j) > attempt(job))
    {
        Look::Already
    } else if jobs.iter().any(|j| j["status"] != "completed") {
        Look::Running
    } else {
        Look::Ready
    }
}
/// Asks GitHub to rerun a job.
pub async fn send(p: &Planned) -> Result<ops::Posted, String> {
    let job = p.rerun.as_ref().ok_or("This isn't a rerun.")?;
    let id = logs::job_of(&job.url, &p.repo).ok_or("Orbit can only rerun GitHub Actions jobs.")?;
    crate::github::rerun_job(&p.repo, id).await?;
    Ok(ops::Posted {
        id,
        number: p.number,
        url: job.url.clone(),
        updated_at: String::new(),
        body: String::new(),
    })
}

/// The error a rerun gets when GitHub will keep refusing it, in plain
/// words; None for one that may go away, such as a rate limit.
fn lasting(error: &str) -> Option<String> {
    if logs::rate_limited(error) {
        return None;
    }
    if error.contains("HTTP 404") || error.contains("HTTP 410") {
        return Some("GitHub no longer has this job.".into());
    }
    // A conflict may only mean the run started again meanwhile.
    ["HTTP 403", "HTTP 422"]
        .iter()
        .any(|code| error.contains(code))
        .then(|| format!("GitHub won't rerun this job: {}.", logs::short(error)))
}
/// The item a rerun is for, while it is still at the commit Orbit planned
/// it at.
fn item_of<'a>(threads: &'a mut [Thread], planned: &'a Planned) -> Option<(&'a mut Item, &'a Job)> {
    let job = planned.rerun.as_ref()?;
    let t = threads.iter_mut().find(|t| t.key == planned.thread)?;
    let item = t
        .items
        .iter_mut()
        .find(|i| i.number == job.item && i.at == job.sha)?;
    Some((item, job))
}
/// The item's entry for the job Orbit plans to rerun, made if it has none.
fn entry<'a>(item: &'a mut Item, job: &Job) -> &'a mut Rerun {
    let i = match item.reruns.iter().position(|r| r.url == job.url) {
        Some(i) => i,
        None => {
            item.reruns.push(Rerun {
                url: job.url.clone(),
                reason: job.reason.clone(),
                ..Default::default()
            });
            item.reruns.len() - 1
        }
    };
    &mut item.reruns[i]
}
/// Records a try that didn't go through: kept for good when GitHub will
/// keep refusing it, otherwise tried again after a wait that starts at a
/// minute and doubles with each try in a row.
fn wait(r: &mut Rerun, error: &str, now: DateTime<Utc>) {
    if let Some(lasting) = lasting(error) {
        r.lasting = true;
        r.unsure = false;
        r.error = Some(lasting);
        r.retry_at = None;
        return;
    }
    r.tries += 1;
    let wait = Duration::minutes(1i64 << (r.tries - 1).min(6)).min(ops::MAX_RETRY);
    r.retry_at = Some(stamp(now + wait));
    r.error = Some(format!(
        "Orbit couldn't rerun it yet: {}. It tries again later.",
        logs::short(error)
    ));
}
/// Records what Orbit found when it looked at a job before rerunning it:
/// already rerun counts as done, still running waits for it to finish, a
/// job that isn't the check's at that commit is never rerun, and a look that
/// failed waits like a failed rerun. For a rerun Orbit was unsure about, a
/// newer try is its own rerun, which returns Bit's note, and none means it
/// didn't go through. Returns Bit's note.
pub fn looked(
    threads: &mut [Thread],
    planned: &Planned,
    result: Result<Look, String>,
    now: DateTime<Utc>,
) -> Option<Event> {
    let thread = planned.thread.clone();
    let (item, job) = item_of(threads, planned)?;
    let number = item.number;
    let r = entry(item, job);
    match result {
        Ok(Look::Already) => {
            r.done = true;
            r.already = !r.unsure;
            r.at = stamp(now);
            r.error = None;
            r.retry_at = None;
            let mine = std::mem::take(&mut r.unsure);
            return mine.then_some(Event::Reran { thread, number });
        }
        Ok(Look::Elsewhere) => {
            r.lasting = true;
            r.unsure = false;
            r.error = Some(
                "Its link isn't this check's job at this commit, so Orbit won't rerun it.".into(),
            );
            r.retry_at = None;
        }
        // An unsure rerun that GitHub has no newer try of didn't go through.
        Ok(_) if r.unsure => r.unsure = false,
        Ok(Look::Ready) => {}
        Ok(Look::Running) => {
            r.tries += 1;
            let wait = Duration::minutes(1i64 << (r.tries - 1).min(6)).min(ops::MAX_RETRY);
            r.retry_at = Some(stamp(now + wait));
            r.error = Some(
                "Its workflow is still running, so Orbit reruns it once that finishes.".into(),
            );
        }
        Err(error) => wait(r, &error, now),
    }
    None
}
/// Records a rerun about to be sent: on its journal entry, the job, and on
/// its item, as unsure until GitHub answers. Saved before it is sent.
pub fn sending(threads: &mut [Thread], journal: &mut [Write], id: &str, planned: &Planned) {
    let Some(job) = planned.rerun.as_ref() else {
        return;
    };
    if let Some(w) = journal.iter_mut().find(|w| w.id == id) {
        w.url = job.url.clone();
    }
    if let Some((item, job)) = item_of(threads, planned) {
        entry(item, job).unsure = true;
    }
}
/// Records what GitHub said to a rerun, on its journal entry and its item.
/// Returns Bit's note when it went through.
pub fn settle(
    threads: &mut [Thread],
    journal: &mut [Write],
    id: &str,
    planned: &Planned,
    result: Result<ops::Posted, String>,
    now: DateTime<Utc>,
) -> Option<Event> {
    let at = stamp(now);
    if let Some(w) = journal.iter_mut().find(|w| w.id == id) {
        w.done_at = Some(at.clone());
        match &result {
            Ok(posted) => {
                w.state = WriteState::Done;
                w.url = posted.url.clone();
            }
            Err(error) => {
                w.state = WriteState::Failed;
                w.error = Some(error.clone());
            }
        }
    }
    let (item, job) = item_of(threads, planned)?;
    let number = item.number;
    let r = entry(item, job);
    match result {
        Ok(_) => {
            r.done = true;
            r.unsure = false;
            r.at = at;
            r.error = None;
            r.tries = 0;
            r.retry_at = None;
            Some(Event::Reran {
                thread: planned.thread.clone(),
                number,
            })
        }
        // It may have gone through all the same, so it stays unsure.
        Err(error) => {
            wait(r, &error, now);
            None
        }
    }
}

/// What Orbit's comment or issue says under a failing item about its reruns
/// at this commit: that Orbit reran its job and why, or that it failed again
/// after Orbit reran it, so it is likely real.
pub fn text(item: &Item) -> Option<String> {
    let done: Vec<&Rerun> = item.reruns.iter().filter(|r| r.done).collect();
    let last = done.last()?;
    if last.url == item.url {
        return Some(match (last.already, done.len()) {
            (true, _) => "This job was already rerun on GitHub.".into(),
            (false, 1) => format!(
                "Orbit reran this job, as it looks like bad luck: {}",
                last.reason
            ),
            (false, _) => format!(
                "Orbit reran this job a second time, as it still looks like bad luck: {}",
                last.reason
            ),
        });
    }
    Some(if done.len() == 1 {
        "It failed again after it was rerun, so it's likely real.".into()
    } else {
        "It failed again after two reruns, so it's likely real.".into()
    })
}

#[cfg(test)]
#[path = "reruns_tests.rs"]
mod tests;
