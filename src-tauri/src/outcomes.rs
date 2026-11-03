//! How what Orbit and the agents did turned out. Each thing Orbit does on
//! GitHub with a result it can see later, and each fix an agent names,
//! gets an outcome check when it's done (`record`): what it was, and what
//! done means, such as "it reads as on". Orbit then checks each one by code
//! against what it reads next (`settle`), and it held or missed, with why,
//! or Orbit couldn't tell. Outcome checks only read what Orbit already
//! reads: they post nothing, change nothing on GitHub, and never close or
//! reopen a problem. A miss is a note from Bit (`noted_elsewhere` says
//! which misses Bit already notes from the CI thread).
//!
//! What gets one:
//! - a security setting Orbit turned on (`crate::baseline`) reads as on,
//!   and is still on a week later;
//! - a tag Orbit pushed (`crate::tagging`) points at its commit, and its
//!   release build (`crate::builds`) passes within a day;
//! - a draft Orbit published (`crate::drafts`) reads as published;
//! - a job Orbit reran (`crate::reruns`) passed the second time;
//! - Dependabot rebased a pull request Orbit asked it to
//!   (`crate::dependabot`), within a day;
//! - an agent's `Fixed I2 in <sha>` passed at that commit (`crate::ops`);
//! - a default branch that passed after failing (`crate::problems`) stays
//!   fixed for 3 days.
//!
//! A check due later (`Outcome.due`) settles on its date, or sooner when
//! Orbit sees it miss.
use crate::{
    baseline::Setting,
    broker::clean,
    ci::{short, CiState},
    ops::{Action, AnswerKind, FixCheck, Thread, WriteState},
    problems::{stamp, time, ProblemKind},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How long after it was done something still gets an outcome check when
/// Orbit first sees it, so an upgrade doesn't check old work.
pub const RECENT_HOURS: i64 = 24;
/// How long Orbit waits for an outcome before it says it couldn't tell.
pub const GIVE_UP_DAYS: i64 = 7;
/// How long settled outcomes are kept: misses longer, so Orbit can learn
/// from them; and the most kept.
pub const KEEP_DAYS: i64 = 14;
pub const KEEP_MISSED_DAYS: i64 = 90;
pub const MAX_KEPT: usize = 200;
/// The most a summary of them names.
pub const SHOWN: usize = 10;
/// When the checks due later are due: a default branch stays fixed, a
/// setting stays on, a tag's release build passes, Dependabot rebases.
pub const HOLDS_DAYS: i64 = 3;
pub const STILL_ON_DAYS: i64 = 7;
pub const BUILD_HOURS: i64 = 24;
pub const REBASE_HOURS: i64 = 24;

/// What an outcome check is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Setting,
    Tag,
    Release,
    Rerun,
    Rebase,
    Fix,
    /// A default branch that passed after failing stays fixed.
    Holds,
    /// A setting Orbit turned on is still on a week later.
    StillOn,
    /// A tag Orbit pushed has a release build that passes.
    Build,
}
/// How it turned out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Orbit hasn't read enough yet to tell.
    #[default]
    Waiting,
    Held,
    Missed,
    /// Orbit can't tell any more, such as when what it would read is gone.
    Unclear,
}

/// One outcome check.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Outcome {
    /// Stays the same for the same thing: `write:<id>` for a write in the
    /// journal, `rerun:<thread>:<item>:<job>`,
    /// `fix:<thread>:<item>:<commit>`, `holds:<problem>:<commit>` and
    /// `later:<id>` for the check due later after one held.
    pub id: String,
    pub kind: Kind,
    pub repo: String,
    /// The pull request it is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// What was done, in a sentence, and what done means, as a clause:
    /// "it reads as on".
    pub what: String,
    pub expect: String,
    /// Where GitHub shows it.
    pub url: String,
    /// When it was done, and when it settled.
    pub at: String,
    pub verdict: Verdict,
    /// Why it held or missed, or why Orbit couldn't tell, in a sentence.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<String>,
    /// For a check due later: when. It settles then, or sooner on a miss.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// What it is checked against: the setting; the commit a tag should
    /// point at, a pull request was at when Orbit asked for a rebase, a
    /// job was rerun at, or an agent named; the tag; the release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting: Option<Setting>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sha: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<u64>,
    /// For a rerun or a fix: the thread and item it is on, and the job
    /// rerun.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub thread: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub item: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub job: String,
    /// For a default branch that stays fixed: its problem.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub problem: String,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Adds an outcome check for each thing done lately that has none yet.
/// Says whether it added any.
pub fn record(s: &mut Snapshot, now: DateTime<Utc>) -> bool {
    let recent = |at: &str| {
        time(at).is_some_and(|t| now.signed_duration_since(t) <= Duration::hours(RECENT_HOURS))
    };
    let mut new: Vec<Outcome> = vec![];
    for w in s.journal.iter().filter(|w| w.state == WriteState::Done) {
        let Some(done) = w.done_at.as_deref().filter(|at| recent(at)) else {
            continue;
        };
        let repo = &w.repo;
        let tag = w.tag.clone().unwrap_or_default();
        let head = w.head.clone().unwrap_or_default();
        let base = Outcome {
            id: format!("write:{}", w.id),
            repo: repo.clone(),
            number: w.number,
            url: w.url.clone(),
            at: done.to_string(),
            ..Default::default()
        };
        let o = match w.action {
            Action::ChangeSetting => {
                let Some(setting) = w.setting else { continue };
                let did = setting.fixed_text();
                let did = format!("{}{}", did[..1].to_lowercase(), &did[1..]);
                Outcome {
                    kind: Kind::Setting,
                    what: format!("Orbit {did} in {repo}."),
                    expect: if setting.is_switch() {
                        "it reads as on".into()
                    } else {
                        "GitHub shows the change".into()
                    },
                    setting: Some(setting),
                    ..base
                }
            }
            Action::PushTag if valid_sha(&head) && !tag.is_empty() => Outcome {
                kind: Kind::Tag,
                what: format!("Orbit pushed the tag {tag} in {repo}."),
                expect: format!("it points at {}", short(&head)),
                sha: head,
                tag,
                ..base
            },
            Action::PublishRelease if w.release.is_some() => Outcome {
                kind: Kind::Release,
                what: format!("Orbit published the draft release for {tag} in {repo}."),
                expect: "it reads as published".into(),
                release: w.release,
                tag,
                ..base
            },
            Action::AskRebase if valid_sha(&head) && w.number.is_some() => Outcome {
                kind: Kind::Rebase,
                what: format!(
                    "Orbit asked Dependabot to rebase {repo} #{}.",
                    w.number.unwrap_or(0)
                ),
                expect: "Dependabot rebases it within a day".into(),
                sha: head,
                due: later(done, Duration::hours(REBASE_HOURS)),
                ..base
            },
            _ => continue,
        };
        new.push(o);
    }
    for t in s.ops.iter().filter(|t| !t.gone) {
        let place = crate::ops::place(t);
        for item in &t.items {
            let check = clean(&item.name(), 160);
            for r in item.reruns.iter().filter(|r| r.done && recent(&r.at)) {
                new.push(Outcome {
                    id: format!("rerun:{}:{}:{}", t.key, item.number, r.url),
                    kind: Kind::Rerun,
                    repo: t.repo.clone(),
                    number: t.number,
                    what: format!("Orbit reran {check} at {} on {place}.", short(&item.at)),
                    expect: "the job passes the second time".into(),
                    url: r.url.clone(),
                    at: r.at.clone(),
                    sha: item.at.clone(),
                    thread: t.key.clone(),
                    item: item.number,
                    job: r.url.clone(),
                    ..Default::default()
                });
            }
            let Some(a) = item.answer.as_ref() else {
                continue;
            };
            let Some(commit) = a.commit.as_deref().filter(|_| a.kind == AnswerKind::Fixed) else {
                continue;
            };
            if !recent(&a.at) {
                continue;
            }
            new.push(Outcome {
                id: format!("fix:{}:{}:{commit}", t.key, item.number),
                kind: Kind::Fix,
                repo: t.repo.clone(),
                number: t.number,
                what: format!(
                    "{} said I{} ({check}) is fixed in {} on {place}.",
                    clean(&a.author, 60),
                    item.number,
                    short(commit)
                ),
                expect: format!("its checks pass at {}", short(commit)),
                url: a.url.clone(),
                at: a.at.clone(),
                sha: commit.to_string(),
                thread: t.key.clone(),
                item: item.number,
                ..Default::default()
            });
        }
    }
    // A default branch that passed after failing stays fixed.
    for p in s
        .problems
        .iter()
        .filter(|p| p.kind == ProblemKind::DefaultBranch && p.passed())
    {
        let Some(closed) = p.closed_at.as_deref().filter(|at| recent(at)) else {
            continue;
        };
        let branch = if p.branch.is_empty() {
            "the default branch"
        } else {
            &p.branch
        };
        new.push(Outcome {
            id: format!("holds:{}:{}", p.id, p.closed_sha),
            kind: Kind::Holds,
            repo: p.repo.clone(),
            what: format!(
                "CI on {} in {} passed at {} after failing.",
                clean(branch, 200),
                p.repo,
                short(&p.closed_sha)
            ),
            expect: format!("it stays fixed for {HOLDS_DAYS} days"),
            url: format!("https://github.com/{}/commit/{}", p.repo, p.closed_sha),
            at: closed.to_string(),
            due: later(closed, Duration::days(HOLDS_DAYS)),
            sha: p.closed_sha.clone(),
            problem: p.id.clone(),
            ..Default::default()
        });
    }
    // Once a setting or a tag held, a check due later.
    for o in s.outcomes.iter().filter(|o| o.verdict == Verdict::Held) {
        if !o.settled_at.as_deref().is_some_and(recent) {
            continue;
        }
        let (kind, expect, after) = match o.kind {
            Kind::Setting if o.setting.is_some_and(Setting::is_switch) => (
                Kind::StillOn,
                "it's still on a week later".to_string(),
                Duration::days(STILL_ON_DAYS),
            ),
            Kind::Setting => (
                Kind::StillOn,
                "GitHub still shows the change a week later".to_string(),
                Duration::days(STILL_ON_DAYS),
            ),
            Kind::Tag => (
                Kind::Build,
                "its release build passes within a day".to_string(),
                Duration::hours(BUILD_HOURS),
            ),
            _ => continue,
        };
        new.push(Outcome {
            id: format!("later:{}", o.id),
            kind,
            expect,
            due: later(&o.at, after),
            verdict: Verdict::Waiting,
            why: String::new(),
            settled_at: None,
            ..o.clone()
        });
    }
    let before = s.outcomes.len();
    for o in new {
        if !s.outcomes.iter().any(|x| x.id == o.id) {
            s.outcomes.push(o);
        }
    }
    s.outcomes.len() != before
}
/// The time `after` from `at`, as Orbit keeps times.
fn later(at: &str, after: Duration) -> Option<String> {
    time(at).map(|t| stamp(t + after))
}
/// Whether a check due later is due.
fn is_due(o: &Outcome, now: DateTime<Utc>) -> bool {
    o.due
        .as_deref()
        .and_then(time)
        .is_some_and(|due| now >= due)
}

/// Checks each outcome still waiting against what Orbit read since, and
/// returns those that settled just now.
pub fn settle(s: &mut Snapshot, now: DateTime<Utc>) -> Vec<Outcome> {
    let mut decided = vec![];
    for (i, o) in s.outcomes.iter().enumerate() {
        if o.verdict != Verdict::Waiting {
            continue;
        }
        let found = match o.kind {
            Kind::Setting => setting(s, o),
            Kind::Tag => tag(s, o),
            Kind::Release => release(s, o),
            Kind::Rerun => rerun(s, o),
            Kind::Rebase => rebase(s, o, now),
            Kind::Fix => fix(s, o),
            Kind::Holds => holds(s, o, now),
            Kind::StillOn => still_on(s, o, now),
            Kind::Build => build(s, o, now),
        };
        // From when it was done, or for a check due later, from its date.
        let from = o.due.as_deref().unwrap_or(&o.at);
        let gave_up = time(from)
            .is_none_or(|at| now.signed_duration_since(at) > Duration::days(GIVE_UP_DAYS));
        let found = found.or_else(|| {
            gave_up.then(|| {
                (
                    Verdict::Unclear,
                    format!("Orbit couldn't tell within {GIVE_UP_DAYS} days."),
                )
            })
        });
        if let Some(found) = found {
            decided.push((i, found));
        }
    }
    let mut settled = vec![];
    for (i, (verdict, why)) in decided {
        let o = &mut s.outcomes[i];
        o.verdict = verdict;
        o.why = why;
        o.settled_at = Some(stamp(now));
        settled.push(o.clone());
    }
    settled
}
type Found = Option<(Verdict, String)>;
/// Whether `read` is later than `at`: Orbit read it after it was done.
fn after(read: &str, at: &str) -> bool {
    matches!((time(read), time(at)), (Some(r), Some(a)) if r > a)
}
fn connected(s: &Snapshot, repo: &str) -> bool {
    s.settings
        .repositories
        .iter()
        .any(|r| same_repo(&r.name, repo))
}
fn gone(s: &Snapshot, o: &Outcome) -> Found {
    (!connected(s, &o.repo)).then(|| {
        (
            Verdict::Unclear,
            format!("{} isn't connected any more.", o.repo),
        )
    })
}

fn setting(s: &Snapshot, o: &Outcome) -> Found {
    let setting = o.setting?;
    let Some(read) = s.repo_settings.iter().find(|r| same_repo(&r.repo, &o.repo)) else {
        // Not read yet: wait for the next read, or give up in time.
        return gone(s, o);
    };
    if !after(&read.read_at, &o.at) {
        return gone(s, o);
    }
    let switch = setting.is_switch();
    match read.on(setting) {
        Some(true) if switch => Some((Verdict::Held, "It reads as on.".into())),
        Some(true) => Some((Verdict::Held, "GitHub shows the change.".into())),
        Some(false) if switch => Some((
            Verdict::Missed,
            "It still reads as off, though GitHub took the change. Its plan may not offer it."
                .into(),
        )),
        Some(false) => Some((
            Verdict::Missed,
            "GitHub still shows it as before, though it took the change.".into(),
        )),
        None => Some((
            Verdict::Unclear,
            "GitHub doesn't say whether it's on.".into(),
        )),
    }
}

/// The repository's releases and tags, as Orbit read them after `o` was
/// done. A read GitHub refused (`unseen`) says nothing either way, so it
/// waits for one that worked.
fn releases_after<'a>(s: &'a Snapshot, o: &Outcome) -> Option<&'a crate::releases::RepoReleases> {
    s.releases
        .iter()
        .find(|r| same_repo(&r.repo, &o.repo))
        .filter(|r| after(&r.read_at, &o.at) && r.unseen.is_none())
}

fn tag(s: &Snapshot, o: &Outcome) -> Found {
    let Some(read) = releases_after(s, o) else {
        return gone(s, o);
    };
    match read.tags.iter().find(|t| t.name == o.tag) {
        Some(t) if t.sha == o.sha => {
            Some((Verdict::Held, format!("It points at {}.", short(&o.sha))))
        }
        Some(t) => Some((
            Verdict::Missed,
            format!("It points at {}, not {}.", short(&t.sha), short(&o.sha)),
        )),
        // GitHub may have more tags than Orbit read.
        None if read.more_tags => None,
        None => Some((Verdict::Missed, "GitHub doesn't list the tag.".into())),
    }
}

fn release(s: &Snapshot, o: &Outcome) -> Found {
    let id = o.release?;
    let Some(read) = releases_after(s, o) else {
        return gone(s, o);
    };
    let published = [&read.latest, &read.prerelease]
        .into_iter()
        .flatten()
        .any(|r| r.id == id && !r.draft);
    if published {
        Some((Verdict::Held, "It reads as published.".into()))
    } else if read.drafts.iter().any(|d| d.id == id) {
        Some((Verdict::Missed, "It's still a draft.".into()))
    } else if !read.drafts_hidden && read.drafts.len() as u32 >= read.draft_count {
        // Orbit reads only the latest release and pre-release, so one
        // published behind them looks the same as one deleted.
        Some((
            Verdict::Unclear,
            "It isn't a draft any more, but it isn't the latest release either, so Orbit can't tell whether it was published or deleted."
                .into(),
        ))
    } else {
        None
    }
}

fn rerun(s: &Snapshot, o: &Outcome) -> Found {
    let thread = s.ops.iter().find(|t| t.key == o.thread && !t.gone);
    let item = thread.and_then(|t| t.items.iter().find(|i| i.number == o.item));
    let (Some(t), Some(item)) = (thread, item) else {
        return Some((
            Verdict::Unclear,
            "Orbit doesn't follow that check any more.".into(),
        ));
    };
    if item.at != o.sha {
        return Some((
            Verdict::Unclear,
            "A newer commit came before Orbit saw how the rerun went.".into(),
        ));
    }
    let failed_again = item.reruns.iter().any(|r| r.url == o.job && r.failed_again);
    if failed_again {
        return Some((Verdict::Missed, "It failed again after the rerun.".into()));
    }
    let release = s
        .problems
        .iter()
        .any(|p| p.id == t.problem_id && p.kind == ProblemKind::ReleaseBuild);
    if release {
        return build_rerun(s, t, o);
    }
    // Told only by CI read since the rerun, with the names of all its
    // checks (not from before a restart). GitHub reruns a job as a new attempt in
    // the same workflow run, so only checks by its name in that run count:
    // another run of the workflow at the commit, such as a pull request's,
    // says nothing of it, and two jobs in its run by one name can't be told
    // apart. A job cancelled or skipped isn't failing either, so it holds
    // only when GitHub says it passed.
    let read = s.ci.iter().find(|c| same_repo(&c.repo, &t.repo))?;
    if !after(&read.checked_at, &o.at) {
        return None;
    }
    let ci = commit_ci(s, t).filter(|c| c.sha == o.sha && c.named)?;
    let run = run_of(&o.job)?;
    let same: Vec<_> = ci
        .checks
        .iter()
        .filter(|(c, _)| c.name == item.check && run_of(&c.url) == Some(run))
        .collect();
    // A check by its name failing may be the rerun, and a read that missed
    // some checks may have missed it: either way, wait.
    let failing = item.failing || ci.failing.iter().any(|c| c.name == item.check);
    if failing || ci.incomplete || ci.cursor.is_some() {
        return None;
    }
    let finished = ci.running == 0;
    match same.as_slice() {
        [(c, CiState::Passing)] if c.url != o.job => {
            Some((Verdict::Held, "It passed after the rerun.".into()))
        }
        [(c, CiState::None)] if c.url != o.job => Some((
            Verdict::Unclear,
            "It didn't pass or fail after the rerun: GitHub may have cancelled or skipped it."
                .into(),
        )),
        [] if finished => Some((
            Verdict::Unclear,
            "Orbit doesn't see the rerun among the checks at that commit.".into(),
        )),
        [_, _, ..] if finished => Some((
            Verdict::Unclear,
            "Two jobs in its run share its name, so Orbit can't tell which one was rerun.".into(),
        )),
        _ => None,
    }
}
/// The latest commit's checks, as Orbit last read them, for a thread's
/// pull request or default branch.
fn commit_ci<'a>(s: &'a Snapshot, t: &Thread) -> Option<&'a crate::ci::CommitCi> {
    let repo = s.ci.iter().find(|c| same_repo(&c.repo, &t.repo))?;
    match (&t.pr_id, t.number) {
        (Some(_), Some(n)) => repo.pull(n).map(|p| &p.ci),
        (Some(_), None) => None,
        (None, _) => Some(&repo.head),
    }
}
/// A rerun of a release build's job, by its workflow run in the build
/// Orbit read after it: GitHub reruns a job in the same run.
fn build_rerun(s: &Snapshot, t: &Thread, o: &Outcome) -> Found {
    let build = s.builds.iter().find(|b| same_repo(&b.repo, &t.repo))?;
    if build.tag != t.branch {
        return Some((
            Verdict::Unclear,
            format!("Orbit watches the build of {} now.", clean(&build.tag, 80)),
        ));
    }
    if build.sha != o.sha {
        return Some((
            Verdict::Unclear,
            "A newer commit came before Orbit saw how the rerun went.".into(),
        ));
    }
    if !after(&build.read_at, &o.at) {
        return None;
    }
    let run = run_of(&o.job).and_then(|id| build.runs.iter().find(|r| r.id == id))?;
    match run.state {
        CiState::Passing => Some((Verdict::Held, "Its run passed after the rerun.".into())),
        CiState::None if !run.conclusion.is_empty() => Some((
            Verdict::Unclear,
            format!(
                "Its run didn't pass or fail after the rerun: GitHub says it was {}.",
                clean(&run.conclusion, 40)
            ),
        )),
        _ => None,
    }
}
/// The workflow run a job's link is in: `.../actions/runs/<id>/job/<id>`.
fn run_of(job: &str) -> Option<u64> {
    job.split("/actions/runs/")
        .nth(1)?
        .split('/')
        .next()?
        .parse()
        .ok()
}

fn rebase(s: &Snapshot, o: &Outcome, now: DateTime<Utc>) -> Found {
    let number = o.number?;
    match s
        .prs
        .iter()
        .find(|p| p.number == number && same_repo(&p.repo, &o.repo))
    {
        Some(pr) if pr.head_sha != o.sha => Some((
            Verdict::Held,
            format!("Dependabot rebased it, to {}.", short(&pr.head_sha)),
        )),
        // Orbit reads pull requests on each sync: one that didn't move by
        // then wasn't rebased. A sync that failed for its repository keeps
        // what Orbit read before, so it says nothing.
        Some(_)
            if is_due(o, now)
                && s.last_poll
                    .as_deref()
                    .is_some_and(|synced| !after(o.due.as_deref().unwrap_or_default(), synced))
                && !s.sync_failures.iter().any(|f| same_repo(&f.repo, &o.repo)) =>
        {
            Some((
                Verdict::Missed,
                "Dependabot didn't rebase it within a day.".into(),
            ))
        }
        Some(_) => None,
        None => gone(s, o).or_else(|| {
            Some((
                Verdict::Unclear,
                "It closed before Orbit saw it rebased.".into(),
            ))
        }),
    }
}

fn fix(s: &Snapshot, o: &Outcome) -> Found {
    let answer = s
        .ops
        .iter()
        .find(|t| t.key == o.thread && !t.gone)
        .and_then(|t| t.items.iter().find(|i| i.number == o.item))
        .and_then(|i| i.answer.as_ref());
    let Some(a) = answer else {
        return Some((
            Verdict::Unclear,
            "Orbit doesn't follow that item any more.".into(),
        ));
    };
    if a.commit.as_deref() != Some(o.sha.as_str()) {
        return Some((
            Verdict::Unclear,
            "The agent said something else about it since.".into(),
        ));
    }
    let at = short(&o.sha);
    match a.check? {
        FixCheck::Waiting => None,
        FixCheck::Passes => Some((Verdict::Held, format!("Its checks pass at {at}."))),
        FixCheck::Fails => Some((Verdict::Missed, format!("Its checks fail at {at}."))),
        FixCheck::NotOnBranch => Some((
            Verdict::Missed,
            format!("{at} isn't on the branch Orbit reads."),
        )),
        FixCheck::NotFound => Some((
            Verdict::Missed,
            format!("GitHub doesn't know the commit {at}."),
        )),
    }
}

/// A default branch that passed stays fixed: it misses as soon as it fails
/// again, as the same problem back or a new one, and holds once Orbit read
/// its CI on or after the date.
fn holds(s: &Snapshot, o: &Outcome, now: DateTime<Utc>) -> Found {
    let Some(p) = s.problems.iter().find(|p| p.id == o.problem) else {
        return gone(s, o).or_else(|| {
            Some((
                Verdict::Unclear,
                "Orbit doesn't keep that problem any more.".into(),
            ))
        });
    };
    let passed = time(&o.at);
    // Back again: the same problem reopened, or a new one opened since.
    let back = if p.closed_sha != o.sha {
        p.timeline
            .iter()
            .find(|e| after(&e.at, &o.at) && e.text.starts_with("Failing again"))
            .map(|e| e.at.clone())
            .or_else(|| Some(p.updated_at.clone()))
    } else {
        s.problems
            .iter()
            .filter(|q| q.key == p.key && q.id != p.id && after(&q.opened_at, &o.at))
            .map(|q| q.opened_at.clone())
            .min()
    };
    if let Some(back) = back {
        let gap = match (passed, time(&back)) {
            (Some(a), Some(b)) => format!(" {} after it passed", crate::problems::how_long(b - a)),
            _ => String::new(),
        };
        return Some((Verdict::Missed, format!("It failed again{gap}.")));
    }
    // Held only by a read on or after the date that shows the branch
    // passing: one that shows it running or failing waits, as a failure
    // reopens the problem and misses.
    let due = o.due.as_deref()?;
    let read =
        s.ci.iter()
            .find(|c| same_repo(&c.repo, &o.repo))
            .is_some_and(|c| !after(due, &c.checked_at) && c.head.state == CiState::Passing);
    (is_due(o, now) && read).then(|| {
        (
            Verdict::Held,
            format!("It stayed fixed for {HOLDS_DAYS} days."),
        )
    })
}

/// A setting Orbit turned on is still on a week later: it misses as soon
/// as Orbit reads it off, and holds once Orbit read it on, on or after the
/// date.
fn still_on(s: &Snapshot, o: &Outcome, now: DateTime<Utc>) -> Found {
    let setting = o.setting?;
    let Some(read) = s
        .repo_settings
        .iter()
        .find(|r| same_repo(&r.repo, &o.repo))
        .filter(|r| after(&r.read_at, &o.at))
    else {
        return gone(s, o);
    };
    let switch = setting.is_switch();
    match read.on(setting) {
        Some(false) if switch => Some((Verdict::Missed, "It reads as off again.".into())),
        Some(false) => Some((
            Verdict::Missed,
            "GitHub shows it as it was before again.".into(),
        )),
        Some(true) if is_due(o, now) && !after(o.due.as_deref()?, &read.read_at) => Some((
            Verdict::Held,
            if switch {
                "It's still on a week later.".into()
            } else {
                "GitHub still shows the change a week later.".into()
            },
        )),
        _ => None,
    }
}

/// A tag Orbit pushed has a release build that passes within a day: it
/// holds once Orbit reads it passing, and misses if it still fails or runs
/// when the day is up.
fn build(s: &Snapshot, o: &Outcome, now: DateTime<Utc>) -> Found {
    if !s.settings.watch_ci {
        return None;
    }
    let read = s.builds.iter().find(|b| same_repo(&b.repo, &o.repo));
    let Some(b) = read.filter(|b| b.tag == o.tag && b.sha == o.sha) else {
        return match read {
            Some(b) if !b.tag.is_empty() => Some((
                Verdict::Unclear,
                format!("Orbit watches the build of {} now.", clean(&b.tag, 100)),
            )),
            _ => gone(s, o),
        };
    };
    if !after(&b.read_at, &o.at) {
        return gone(s, o);
    }
    // Passed only as Orbit would publish from it: every run a success or
    // skipped, with one success. A cancelled run isn't failing either.
    let why = match crate::builds::publishable(b) {
        Ok(()) => return Some((Verdict::Held, "Its release build passed.".into())),
        Err(why) => why,
    };
    if !is_due(o, now) {
        return None;
    }
    match b.ci.state {
        CiState::Failing => Some((
            Verdict::Missed,
            format!(
                "Its release build still fails: {}.",
                clean(&crate::problems::names(&b.ci.failing), 300)
            ),
        )),
        CiState::Running => Some((
            Verdict::Missed,
            "Its release build hadn't passed within a day.".into(),
        )),
        _ if b.runs.is_empty() => Some((
            Verdict::Unclear,
            "No release build ran for it within a day.".into(),
        )),
        _ => Some((
            Verdict::Unclear,
            format!(
                "Orbit couldn't tell whether its release build passed within a day: {}.",
                clean(&why, 200)
            ),
        )),
    }
}

/// Lets go of outcomes settled long ago, keeping misses longer and at most
/// `MAX_KEPT`, the newest. Waiting ones are kept. Says whether it let any
/// go.
pub fn prune(outcomes: &mut Vec<Outcome>, now: DateTime<Utc>) -> bool {
    let before = outcomes.len();
    outcomes.retain(|o| {
        let days = match o.verdict {
            Verdict::Waiting => return true,
            Verdict::Missed => KEEP_MISSED_DAYS,
            _ => KEEP_DAYS,
        };
        let settled = o.settled_at.as_deref().and_then(time);
        settled.is_some_and(|t| now.signed_duration_since(t) <= Duration::days(days))
    });
    if outcomes.len() > MAX_KEPT {
        outcomes.sort_by(|a, b| a.at.cmp(&b.at));
        let excess = outcomes.len() - MAX_KEPT;
        outcomes.drain(..excess);
    }
    outcomes.len() != before
}

/// Whether Bit already tells you of this kind of miss another way: from
/// the CI thread it is on (`crate::ops`), a job that failed again after a
/// rerun, and an agent's fix that still fails, isn't on the branch, or
/// names a commit GitHub doesn't know; and a default branch failing again,
/// which is a problem of its own (or the same one back, quietly at first,
/// as `crate::problems` keeps it). Those misses get no second note.
pub fn noted_elsewhere(o: &Outcome) -> bool {
    matches!(o.kind, Kind::Rerun | Kind::Fix | Kind::Holds)
}

/// What held and what missed since `since` (a brief's time), the newest
/// first.
pub fn settled_since<'a>(outcomes: &'a [Outcome], since: &str) -> Vec<&'a Outcome> {
    let mut out: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| matches!(o.verdict, Verdict::Held | Verdict::Missed))
        .filter(|o| o.settled_at.as_deref().is_some_and(|at| after(at, since)))
        .collect();
    out.sort_by(|a, b| b.settled_at.cmp(&a.settled_at));
    out
}

/// What a missed outcome says in Bit's note.
pub fn missed_text(o: &Outcome) -> String {
    format!("{} {}", o.what, o.why)
}

#[cfg(test)]
#[path = "outcomes_tests.rs"]
mod tests;
