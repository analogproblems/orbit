//! What Orbit learns from the problems it saw (see `crate::problems`).
//!
//! **Case history.** Each problem about failing checks (on a default
//! branch, a pull request or a release build) gets a short record of its
//! own, its `Case`. Orbit keeps it while the problem is open and for
//! `KEEP_DAYS` after it closes, long after the problem itself is let go.
//! The case says what failed and where, and how it ended (`End`). It is
//! made by code from the problem and its CI thread (`record`), before
//! Orbit refreshes the thread, so it still sees the reruns, the agent's
//! answers and the step that failed as they were when the problem closed.
//! The case keeps every job Orbit saw fail at that commit, each by its own
//! link (`Case.failed`), as a problem keeps each failing name once. A
//! problem that passed at the commit it failed at counts as bad luck only
//! when Orbit saw each of those jobs pass there again (`passed_again`).
//!
//! **Last time.** When a new problem opens, Orbit looks for the latest
//! case like it (`look_back`): first one with its key, and failing that,
//! one in its repository where one of the same checks failed. The new
//! problem says how that one ended (`LastTime`).
//!
//! **Lessons.** Orbit finds repeats among the cases and the missed outcome
//! checks of the last `LEARN_DAYS` (see `crate::outcomes`). A case counts
//! once its problem can no longer come back as the same one (`FLAP_QUIET`
//! after it closed), and once per lesson however many of its checks the
//! lesson's name covers. Checks are told apart by their full names, which
//! text only shortens (`luck_id`). Orbit keeps each repeat as a `Lesson`: what happened,
//! and what to do next time (`learn`). A lesson applies to the open
//! problems it is about (`apply`), which then say it to the agent and to
//! you. You can forget one, and it is learned again only from what
//! happens after that, as is one Orbit let go to keep at most
//! `MAX_LESSONS`.
//!
//! Guidance, never authority: a lesson changes what Orbit and Bit say. It
//! never changes a rule, or what Bit does on its own. Nothing here posts
//! anything or reads GitHub.
use crate::{
    baseline::Setting,
    broker::clean,
    ci::{short, Check, CiState, CommitCi, RepoCi},
    ops::{AnswerKind, Thread},
    outcomes::{Kind, Verdict},
    problems::{how_long, stamp, time, Problem, ProblemKind, FLAP_QUIET},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How long a case is kept after its problem closed, a lesson after Orbit
/// last saw it happen, and what you forgot.
pub const KEEP_DAYS: i64 = 90;
/// The most cases and lessons kept, the newest first.
pub const MAX_CASES: usize = 300;
pub const MAX_LESSONS: usize = 50;
/// How far back a repeat counts, and how many times something must happen
/// to be a lesson: a check failing by bad luck or a problem coming back 3
/// times, a fix that didn't hold twice.
pub const LEARN_DAYS: i64 = 30;
pub const REPEATS: usize = 3;
pub const MISSES: usize = 2;
/// The most checks a case names, and the most cases or outcome checks a
/// lesson names as where it came from.
pub const MAX_CHECKS: usize = 10;
pub const MAX_FROM: usize = 5;
/// The most failed jobs a case keeps to see pass again (`Case.failed`).
pub const MAX_FAILED: usize = 100;

/// How a case ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// Its problem is still open.
    #[default]
    Open,
    /// It passed at the commit it failed at, when its checks ran again.
    Again,
    /// It stopped failing at the commit it failed at, but Orbit didn't see
    /// what failed pass there: GitHub may have cancelled or skipped it, or
    /// started it over as a new run.
    Unclear,
    /// It passed after an agent said it fixed it.
    Fixed,
    /// It passed at a newer commit.
    Passed,
    /// Orbit stopped following it before it passed.
    Closed,
}

/// The short record of one problem (see the module's docs).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Case {
    /// The problem's id.
    pub id: String,
    pub key: String,
    pub kind: ProblemKind,
    pub repo: String,
    /// The default branch, the pull request's branch, or the tag.
    pub branch: String,
    /// The pull request, for a pull request's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// Every check that failed while it was open, and those failing when
    /// Orbit last saw it open, each once by its full name as GitHub sends
    /// it: Orbit tells checks apart by it, and shortens it only to show.
    pub checks: Vec<String>,
    pub last: Vec<String>,
    /// The step that failed, from the first job log Orbit read for it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub step: String,
    /// The latest commit it failed at.
    pub sha: String,
    pub opened_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    pub end: End,
    /// The commit it passed at.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub passed: String,
    /// An agent said it fixed it, and in which commit, when it named one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fix: String,
    /// Orbit reran one of its jobs at `sha`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reran: bool,
    /// Every job Orbit saw failing at `sha`, by name and link, with each of
    /// several that failed by one name (a problem names each once), and
    /// for a release build each run that failed: what has to pass again
    /// for it to be bad luck. At most `MAX_FAILED`, and `unsure` when there
    /// were more. Kept until its problem can't come back as the same one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<Check>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unsure: bool,
    /// Why Orbit stopped following it, when it closed without passing.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// What happened the last time something like a problem failed
/// (`Problem.last_time`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LastTime {
    pub case: Case,
    /// When the new problem opened, which the text counts from.
    pub at: String,
    /// In a sentence, as the app shows it.
    pub text: String,
}

/// What kind of repeat a lesson is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonKind {
    /// A check that failed and then passed at the same commit.
    #[default]
    Luck,
    /// A default branch that keeps breaking after it was fixed. (A pull
    /// request's checks failing and then passing at a newer commit is how
    /// work goes, so it isn't counted.)
    Back,
    /// Agents' fixes that didn't pass their checks.
    Fix,
    /// A setting Orbit turned on that was off again a week later.
    Setting,
    /// A release build that failed after Orbit pushed its tag.
    Build,
}

/// Something Orbit learned from a repeat (see the module's docs).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Lesson {
    /// The same for the same repeat: bad luck by its check (`luck_id`),
    /// `back:<problem key>` (a default branch's),
    /// `fix:<repo>`, `setting:<repo>:<setting>` or `build:<repo>`, in lower
    /// case but for the check, as GitHub tells checks apart by case.
    pub id: String,
    pub kind: LessonKind,
    pub repo: String,
    /// The check, for bad luck, by its full name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub check: String,
    /// The default branch, for one that keeps coming back there.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting: Option<Setting>,
    /// How many times it happened in the last `LEARN_DAYS`, when Orbit last
    /// counted.
    pub count: usize,
    /// For one that keeps coming back: the commits that fixed it, the
    /// newest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<String>,
    /// The cases or outcome checks it was learned from, the newest first.
    pub from: Vec<String>,
    pub learned_at: String,
    /// When it last happened.
    pub seen_at: String,
    /// What happened, and what to do next time, as the app shows them.
    pub what: String,
    pub next: String,
}

/// A lesson you forgot, or Orbit let go to keep at most `MAX_LESSONS`
/// (`Snapshot.forgotten_lessons`): it is learned again only from what
/// happens after `at`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Forgot {
    pub id: String,
    pub at: String,
}

/// Whether a kind of problem gets a case: those about checks that fail.
pub fn tracked(kind: ProblemKind) -> bool {
    matches!(
        kind,
        ProblemKind::DefaultBranch | ProblemKind::PullRequest | ProblemKind::ReleaseBuild
    )
}

/// The id of the lesson about a check failing by bad luck, by the check's
/// full name: `luck:<repo>:<check>` when the name is short and plain
/// enough to show as it is, else `luck:<repo>#<hash>:<start>`, with a hash
/// of the whole name, so two checks are never one. A repository's name
/// holds no `#` or `:`, so the two never meet, whatever a check is named.
pub fn luck_id(repo: &str, check: &str) -> String {
    let repo = repo.to_lowercase();
    if clean(check, 160) == check {
        return format!("luck:{repo}:{check}");
    }
    format!("luck:{repo}#{}:{}", name_hash(check), clean(check, 100))
}

/// The start of a hash of a check's full name, in hex.
fn name_hash(check: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(check.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The id `luck_id` gave a check before its hashed form changed:
/// `luck:<repo>:<start> #<hash>`, which a check named like that could
/// spell too.
fn old_luck_id(repo: &str, check: &str) -> String {
    if clean(check, 160) == check {
        return luck_id(repo, check);
    }
    format!(
        "luck:{}:{} #{}",
        repo.to_lowercase(),
        clean(check, 100),
        name_hash(check)
    )
}

/// Brings bad luck's lessons saved before their ids changed (`luck_id`) up
/// to date as Orbit loads its state, with what you forgot, so a lesson
/// Orbit has isn't learned again beside it and one you forgot stays
/// forgotten. A lesson knows its check's full name. What you forgot keeps
/// only an id, so Orbit matches it by the checks the cases and lessons
/// name, and forgets each check it could be, keeping the old id too, as a
/// check could be named like it. Says whether anything changed.
pub fn upgrade(s: &mut Snapshot) -> bool {
    let mut moved: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let lessons = s.lessons.iter().map(|l| (&l.repo, &l.check));
    let cases = s.cases.iter().flat_map(|c| {
        let failed = c.failed.iter().map(|f| &f.name);
        c.checks
            .iter()
            .chain(&c.last)
            .chain(failed)
            .map(move |n| (&c.repo, n))
    });
    for (repo, check) in lessons.chain(cases).filter(|(_, c)| !c.is_empty()) {
        let (old, new) = (old_luck_id(repo, check), luck_id(repo, check));
        let ids = moved.entry(old.clone()).or_default();
        if old != new && !ids.contains(&new) {
            ids.push(new);
        }
    }
    let mut changed = false;
    let renamed = |l: &mut Lesson| {
        let id = luck_id(&l.repo, &l.check);
        let old = l.kind == LessonKind::Luck && !l.check.is_empty() && l.id != id;
        if old {
            l.id = id;
        }
        old
    };
    for l in s
        .lessons
        .iter_mut()
        .chain(s.problems.iter_mut().flat_map(|p| p.lessons.iter_mut()))
    {
        changed |= renamed(l);
    }
    let mut forgot = s.forgotten_lessons.clone();
    for f in &s.forgotten_lessons {
        for id in moved.get(&f.id).into_iter().flatten() {
            match forgot.iter_mut().find(|g| &g.id == id) {
                Some(g) if g.at >= f.at => continue,
                Some(g) => g.at = f.at.clone(),
                None => forgot.push(Forgot {
                    id: id.clone(),
                    at: f.at.clone(),
                }),
            }
            changed = true;
        }
    }
    s.forgotten_lessons = forgot;
    changed
}

/// Keeps the cases up to date, says what happened last time on each new
/// problem, learns from repeats and gives each open problem the lessons
/// about it. Returns whether anything changed, and the lessons learned just
/// now, for Bit's notes.
pub fn follow(s: &mut Snapshot, now: DateTime<Utc>) -> (bool, Vec<Lesson>) {
    let mut changed = record(s);
    changed |= look_back(s);
    let learned = learn(s, now);
    changed |= !learned.is_empty();
    changed |= apply(s);
    (changed, learned)
}

fn pr_number(pr_id: Option<&str>) -> Option<u64> {
    pr_id?.rsplit_once('#')?.1.parse().ok()
}

/// What the problem's thread says about it now: the step that failed
/// first, whether Orbit itself reran a job at `sha`, and whether an agent said it
/// fixed it, with the newest commit an agent named.
fn from_thread(t: Option<&Thread>, sha: &str) -> (String, bool, bool, String) {
    let Some(t) = t else {
        return Default::default();
    };
    let failing = || t.items.iter().filter(|i| i.failing);
    let step = failing()
        .filter_map(|i| i.why.as_ref())
        .map(|w| w.step.trim())
        .find(|s| !s.is_empty())
        .map(|s| clean(s, 120))
        .unwrap_or_default();
    // Orbit's own reruns only: one it found already done may have been
    // someone else's.
    let reran = failing().any(|i| i.at == sha && i.reruns.iter().any(|r| r.done && !r.already));
    let answers: Vec<_> = failing()
        .filter_map(|i| i.answer.as_ref())
        .filter(|a| a.kind == AnswerKind::Fixed)
        .collect();
    let fix = answers
        .iter()
        .filter(|a| a.commit.as_deref().is_some_and(valid_sha))
        .max_by(|a, b| a.at.cmp(&b.at))
        .and_then(|a| a.commit.clone())
        .unwrap_or_default();
    (step, reran, !answers.is_empty(), fix)
}

/// The commit a problem failed at, as Orbit last read it with every check
/// by name and link (`CommitCi.named`): the pull request's, or the default
/// branch's.
fn commit_at<'a>(ci: &'a [RepoCi], p: &Problem, number: Option<u64>) -> Option<&'a CommitCi> {
    let read = ci.iter().find(|r| same_repo(&r.repo, &p.repo))?;
    let commit = match number {
        Some(n) => &read.pull(n)?.ci,
        None => &read.head,
    };
    Some(commit).filter(|ci| ci.sha == p.sha && ci.named)
}

/// The release build a problem is about, as Orbit last read it.
fn build_of<'a>(
    builds: &'a [crate::builds::Build],
    p: &Problem,
) -> Option<&'a crate::builds::Build> {
    builds
        .iter()
        .find(|b| same_repo(&b.repo, &p.repo) && b.tag == p.branch && b.sha == p.sha)
}

/// Every job failing at a problem's commit as Orbit reads it now, by name
/// and link, before failures by one name are kept once; for a release
/// build, each run that failed. None from a read that doesn't name them,
/// as after a restart until Orbit reads again.
fn failing_now(
    ci: &[RepoCi],
    builds: &[crate::builds::Build],
    p: &Problem,
    number: Option<u64>,
) -> Vec<Check> {
    if p.kind == ProblemKind::ReleaseBuild {
        return build_of(builds, p)
            .into_iter()
            .flat_map(|b| &b.runs)
            .filter(|run| run.state == CiState::Failing)
            .map(|run| Check {
                name: run.name.clone(),
                url: run.url.clone(),
            })
            .collect();
    }
    commit_at(ci, p, number)
        .into_iter()
        .flat_map(|commit| &commit.checks)
        .filter(|(_, state)| *state == CiState::Failing)
        .map(|(check, _)| check.clone())
        .collect()
}

/// Keeps a job that failed on a case, once by its name and run (a rerun is
/// a new attempt in the same run), at most `MAX_FAILED`.
fn keep_failed(c: &mut Case, f: Check) {
    use crate::outcomes::run_of;
    let run = |k: &Check| {
        run_of(&k.url)
            .map(|id| id.to_string())
            .unwrap_or(k.url.clone())
    };
    if c.failed
        .iter()
        .any(|k| k.name == f.name && run(k) == run(&f))
    {
        return;
    }
    if c.failed.len() >= MAX_FAILED {
        c.unsure = true;
        return;
    }
    c.failed.push(f);
}

/// Whether what failed passed again at the commit it failed at, as Orbit
/// last read it, by every job the case saw fail there (`Case.failed`) and
/// those its problem names. GitHub reruns a job as a new attempt in the
/// same workflow run, so each failed job counts by its own run, from its
/// link. One that isn't there any more, or whose run a new one replaced,
/// didn't pass again, and neither did one GitHub cancelled or skipped,
/// though its problem closes as it no longer fails. With no failed job
/// kept, or more than it keeps, it can't tell.
///
/// For a default branch or a pull request, from a read that saw every
/// check: each failed job's own run has exactly one check by its name
/// (with no run in its link, there is exactly one by its name at all),
/// and every check by its name passed. For a release build: every run is
/// a success or skipped (`builds::publishable`), each run that failed is
/// still the newest of its workflow and succeeded, and so did every run
/// that could own a job by a name the problem names (`Run / job`, or the
/// run's own name).
fn passed_again(ci: &[RepoCi], builds: &[crate::builds::Build], p: &Problem, c: &Case) -> bool {
    use crate::outcomes::run_of;
    if p.failing.is_empty() || c.failed.is_empty() || c.unsure {
        return false;
    }
    let failed = || c.failed.iter().chain(&p.failing);
    if p.kind == ProblemKind::ReleaseBuild {
        let owns = |name: &str, run: &crate::builds::Run| {
            run.name.is_empty() || name == run.name || name.starts_with(&format!("{} / ", run.name))
        };
        let success = |run: &crate::builds::Run| run.conclusion == "success";
        return build_of(builds, p).is_some_and(|b| {
            crate::builds::publishable(b).is_ok()
                && failed().all(|f| {
                    run_of(&f.url)
                        .is_some_and(|id| b.runs.iter().any(|run| run.id == id && success(run)))
                })
                && p.failing
                    .iter()
                    .all(|f| b.runs.iter().filter(|run| owns(&f.name, run)).all(success))
        });
    }
    let Some(commit) =
        commit_at(ci, p, c.number).filter(|ci| !ci.incomplete && ci.cursor.is_none())
    else {
        return false;
    };
    failed().all(|failed| {
        let same: Vec<&(Check, CiState)> = commit
            .checks
            .iter()
            .filter(|(check, _)| check.name == failed.name)
            .collect();
        let own = match run_of(&failed.url) {
            Some(run) => same
                .iter()
                .filter(|(check, _)| run_of(&check.url) == Some(run))
                .count(),
            None => same.len(),
        };
        own == 1 && same.iter().all(|(_, s)| *s == CiState::Passing)
    })
}

/// Adds a case for each new problem about failing checks, follows each
/// open one, and says how each one that closed since ended. A problem that
/// closed before Orbit kept cases gets none. Returns whether any changed.
pub fn record(s: &mut Snapshot) -> bool {
    let mut changed = false;
    for p in s.problems.iter().filter(|p| tracked(p.kind)) {
        let thread = s.ops.iter().find(|t| t.problem_id == p.id);
        let i = match s.cases.iter().position(|c| c.id == p.id) {
            Some(i) => i,
            None if p.is_open() => {
                s.cases.push(Case {
                    id: p.id.clone(),
                    key: p.key.clone(),
                    kind: p.kind,
                    repo: p.repo.clone(),
                    branch: p.branch.clone(),
                    number: pr_number(p.pr_id.as_deref()),
                    opened_at: p.opened_at.clone(),
                    ..Default::default()
                });
                changed = true;
                s.cases.len() - 1
            }
            None => continue,
        };
        let failing = if p.is_open() {
            failing_now(&s.ci, &s.builds, p, s.cases[i].number)
        } else {
            vec![]
        };
        let again = !p.is_open()
            && s.cases[i].end == End::Open
            && p.passed()
            && p.closed_sha == p.sha
            && passed_again(&s.ci, &s.builds, p, &s.cases[i]);
        let c = &mut s.cases[i];
        let before = c.clone();
        if p.is_open() {
            // Open, or back again within the hour as the same problem.
            c.end = End::Open;
            c.closed_at = None;
            c.passed.clear();
            c.reason.clear();
            if c.sha != p.sha {
                c.reran = false;
                c.failed.clear();
                c.unsure = false;
            }
            c.sha = p.sha.clone();
            c.branch = p.branch.clone();
            for f in failing {
                keep_failed(c, f);
            }
            c.last.clear();
            for f in &p.failing {
                if !c.last.contains(&f.name) {
                    c.last.push(f.name.clone());
                }
            }
            for name in &c.last {
                if !c.checks.contains(name) && c.checks.len() < MAX_CHECKS {
                    c.checks.push(name.clone());
                }
            }
            let (step, reran, fixed, fix) = from_thread(thread, &p.sha);
            if c.step.is_empty() {
                c.step = step;
            }
            c.reran |= reran;
            c.fixed = fixed;
            c.fix = fix;
        } else if c.end == End::Open {
            // Closed since the last pass: the thread is still as it was.
            let (_, reran, fixed, fix) = from_thread(thread, &p.sha);
            c.reran |= reran;
            c.fixed |= fixed;
            if !fix.is_empty() {
                c.fix = fix;
            }
            c.closed_at = p.closed_at.clone();
            c.end = if !p.passed() {
                End::Closed
            } else if again {
                End::Again
            } else if p.closed_sha == p.sha {
                End::Unclear
            } else if c.fixed {
                End::Fixed
            } else {
                End::Passed
            };
            c.passed = p.closed_sha.clone();
            if c.end == End::Closed {
                c.reason = clean(p.closed_reason.as_deref().unwrap_or_default(), 200);
            }
        }
        changed |= *c != before;
    }
    changed
}

/// Code formatting of names and commits for the agent, or none for the app.
type Show<'a> = &'a dyn Fn(&str) -> String;

fn list(names: &[String], show: Show) -> String {
    let shown: Vec<String> = names.iter().take(3).map(|n| show(&clean(n, 160))).collect();
    let more = names.len().saturating_sub(shown.len());
    match (shown.as_slice(), more) {
        ([], _) => "Its checks".into(),
        ([one], 0) => one.clone(),
        ([a, b], 0) => format!("{a} and {b}"),
        (_, 0) => {
            let (last, rest) = shown.split_last().expect("three names");
            format!("{} and {last}", rest.join(", "))
        }
        _ => format!("{} and {more} more", shown.join(", ")),
    }
}

/// Where a case's checks failed, as a sentence's end: "on `main`", "on
/// pull request #12", "in the release build for `v1.2.0`".
fn place(c: &Case, show: Show) -> String {
    match c.kind {
        ProblemKind::PullRequest => match c.number {
            Some(n) => format!("on pull request #{n}"),
            None => "on a pull request".into(),
        },
        ProblemKind::ReleaseBuild => format!("in the release build for {}", show(&c.branch)),
        _ if c.branch.is_empty() => "on the default branch".into(),
        _ => format!("on {}", show(&c.branch)),
    }
}

/// How a case ended, in a sentence.
fn ended(c: &Case, show: Show) -> String {
    let took = match (time(&c.opened_at), c.closed_at.as_deref().and_then(time)) {
        (Some(a), Some(b)) => format!(", {} later", how_long(b - a)),
        _ => String::new(),
    };
    let at = show(short(&c.passed));
    match c.end {
        End::Open => "It hasn't ended yet.".into(),
        End::Again if c.reran => {
            format!("It passed when Orbit reran it at the same commit{took}.")
        }
        End::Again => format!("It passed when it ran again at the same commit{took}."),
        End::Unclear => format!(
            "It stopped failing at the same commit{took}, but Orbit didn't see what failed pass there: GitHub may have cancelled or skipped it, or started it over as a new run."
        ),
        End::Fixed if !c.fix.is_empty() && c.fix != c.passed => format!(
            "It passed again at {at} after the agent's fix in {}{took}.",
            show(short(&c.fix))
        ),
        End::Fixed => format!("It passed again at {at} after the agent's fix{took}."),
        End::Passed => format!("It passed again at {at}{took}."),
        End::Closed if c.reason.is_empty() => {
            format!("Orbit stopped following it{took}.")
        }
        End::Closed => format!("Orbit stopped following it{took}: {}", c.reason),
    }
}

/// What happened last time, in a sentence or two: "Last time, 9 days
/// earlier: `CI / test` failed on `main`. It passed again at `1a2b3c4`
/// after the agent's fix, 2 hours later."
pub fn last_time_text(l: &LastTime, show: Show) -> String {
    let c = &l.case;
    let gap = match (c.closed_at.as_deref().and_then(time), time(&l.at)) {
        (Some(closed), Some(at)) => format!(", {} earlier", how_long(at - closed)),
        _ => String::new(),
    };
    let checks = if c.last.is_empty() {
        &c.checks
    } else {
        &c.last
    };
    format!(
        "Last time{gap}: {} failed {}. {}",
        list(checks, show),
        place(c, show),
        ended(c, show)
    )
}

/// The latest case like a new problem, closed before it opened: one with
/// its key, or else one in its repository of the same group (checks, or
/// release builds) where one of the same checks failed.
fn like<'a>(cases: &'a [Case], p: &Problem) -> Option<&'a Case> {
    let names: Vec<&str> = p.failing.iter().map(|f| f.name.as_str()).collect();
    let build = |k: ProblemKind| k == ProblemKind::ReleaseBuild;
    let closed = cases.iter().filter(|c| {
        c.id != p.id
            && c.end != End::Open
            && c.closed_at
                .as_deref()
                .is_some_and(|at| at <= p.opened_at.as_str())
    });
    let newest = |a: &&Case, b: &&Case| a.closed_at.cmp(&b.closed_at);
    closed
        .clone()
        .filter(|c| c.key == p.key)
        .max_by(newest)
        .or_else(|| {
            closed
                .filter(|c| {
                    same_repo(&c.repo, &p.repo)
                        && build(c.kind) == build(p.kind)
                        && c.checks.iter().any(|n| names.contains(&n.as_str()))
                })
                .max_by(newest)
        })
}

/// Says on each problem that opened since the last pass what happened the
/// last time something like it failed, once. Its timeline says it too,
/// unless the problem already says when it was fixed before. Returns
/// whether any changed.
pub fn look_back(s: &mut Snapshot) -> bool {
    let mut changed = false;
    for p in s
        .problems
        .iter_mut()
        .filter(|p| tracked(p.kind) && p.is_open() && !p.looked_back)
    {
        p.looked_back = true;
        changed = true;
        let Some(c) = like(&s.cases, p) else {
            continue;
        };
        // What failed there is only for telling how that case ended.
        let case = Case {
            failed: vec![],
            unsure: false,
            ..c.clone()
        };
        let mut l = LastTime {
            case,
            at: p.opened_at.clone(),
            text: String::new(),
        };
        l.text = last_time_text(&l, &str::to_string);
        if p.fixed_before.is_none() {
            let at = p.opened_at.clone();
            p.note(&at, l.text.clone());
        }
        p.last_time = Some(l);
    }
    changed
}

/// What a lesson says happened, and what to do next time.
pub fn lesson_text(l: &Lesson, show: Show) -> (String, String) {
    let n = l.count;
    let days = LEARN_DAYS;
    match l.kind {
        LessonKind::Luck => (
            format!(
                "{} in {} failed and then passed when it ran again at the same commit {n} times in {days} days.",
                show(&clean(&l.check, 160)),
                l.repo
            ),
            "When it fails again, running it again will likely pass, but it may need fixing so it stops failing by chance.".into(),
        ),
        LessonKind::Back => {
            let place = match l.branch.as_str() {
                "" => "the default branch".into(),
                branch => show(branch),
            };
            let last = l
                .fixes
                .first()
                .map(|sha| format!(", most recently at {}", show(short(sha))))
                .unwrap_or_default();
            (
                format!(
                    "CI on {place} in {} broke {n} times in {days} days and was fixed each time{last}.",
                    l.repo
                ),
                "Look for what keeps breaking it, not just the check that fails now.".into(),
            )
        }
        LessonKind::Fix => (
            format!(
                "Fixes an agent named in {} didn't pass their checks {n} times in {days} days.",
                l.repo
            ),
            "Before saying a check is fixed, see it pass at the fix's commit.".into(),
        ),
        LessonKind::Setting => (
            format!(
                "{} in {} was off again a week after Orbit turned it on, {n} times in {days} days.",
                l.setting.map(Setting::name).unwrap_or("A setting"),
                l.repo
            ),
            "Something may be turning it off, such as an organization's policy or another tool. Look there before turning it on again.".into(),
        ),
        LessonKind::Build => (
            format!(
                "Release builds in {} failed after Orbit pushed their tag {n} times in {days} days.",
                l.repo
            ),
            "Run the release build before tagging, so the tag builds the first time.".into(),
        ),
    }
}

/// One time something happened that a lesson counts.
struct Seen {
    at: String,
    from: String,
    fix: String,
}

/// What each lesson would be learned from now: the repeats among the
/// cases and the missed outcome checks of the last `LEARN_DAYS`, and since
/// you forgot it, by lesson.
fn repeats(s: &Snapshot, now: DateTime<Utc>) -> BTreeMap<String, (Lesson, Vec<Seen>)> {
    let since = now - Duration::days(LEARN_DAYS);
    let counts = |id: &str, at: &str| {
        let Some(at) = time(at) else {
            return false;
        };
        let forgot = s
            .forgotten_lessons
            .iter()
            .find(|f| f.id == id)
            .and_then(|f| time(&f.at));
        at > since && forgot.is_none_or(|f| at > f)
    };
    let mut found: BTreeMap<String, (Lesson, Vec<Seen>)> = BTreeMap::new();
    // Each case or outcome check counts once for a lesson, however many of
    // its checks the lesson's name covers.
    let mut add = |lesson: Lesson, seen: Seen| {
        if !counts(&lesson.id, &seen.at) {
            return;
        }
        let (_, all) = found
            .entry(lesson.id.clone())
            .or_insert_with(|| (lesson, vec![]));
        if !all.iter().any(|x| x.from == seen.from) {
            all.push(seen);
        }
    };
    for c in &s.cases {
        // Only once it can't come back as the same problem, which would
        // undo how it ended.
        let Some(at) = c
            .closed_at
            .clone()
            .filter(|at| time(at).is_some_and(|t| now - t >= FLAP_QUIET))
        else {
            continue;
        };
        match c.end {
            End::Again => {
                for check in &c.last {
                    add(
                        Lesson {
                            id: luck_id(&c.repo, check),
                            kind: LessonKind::Luck,
                            repo: c.repo.clone(),
                            check: check.clone(),
                            ..Default::default()
                        },
                        Seen {
                            at: at.clone(),
                            from: c.id.clone(),
                            fix: String::new(),
                        },
                    );
                }
            }
            End::Fixed | End::Passed if c.kind == ProblemKind::DefaultBranch => add(
                Lesson {
                    id: format!("back:{}", c.key).to_lowercase(),
                    kind: LessonKind::Back,
                    repo: c.repo.clone(),
                    branch: c.branch.clone(),
                    ..Default::default()
                },
                Seen {
                    at,
                    from: c.id.clone(),
                    fix: c.passed.clone(),
                },
            ),
            _ => {}
        }
    }
    for o in s.outcomes.iter().filter(|o| o.verdict == Verdict::Missed) {
        let at = o.settled_at.clone().unwrap_or_else(|| o.at.clone());
        let lesson = match o.kind {
            Kind::Fix => Lesson {
                id: format!("fix:{}", o.repo).to_lowercase(),
                kind: LessonKind::Fix,
                ..Default::default()
            },
            Kind::StillOn if o.setting.is_some() => Lesson {
                id: format!(
                    "setting:{}:{}",
                    o.repo,
                    o.setting.map(Setting::key).unwrap_or_default()
                )
                .to_lowercase(),
                kind: LessonKind::Setting,
                setting: o.setting,
                ..Default::default()
            },
            Kind::Build => Lesson {
                id: format!("build:{}", o.repo).to_lowercase(),
                kind: LessonKind::Build,
                ..Default::default()
            },
            _ => continue,
        };
        add(
            Lesson {
                repo: o.repo.clone(),
                ..lesson
            },
            Seen {
                at,
                from: o.id.clone(),
                fix: String::new(),
            },
        );
    }
    found
}

/// Learns each repeat that happened often enough, and brings each lesson
/// it already has up to date while it does. A lesson that happens less
/// often now is kept as it was, until `prune` lets it go. Returns the
/// lessons learned just now.
pub fn learn(s: &mut Snapshot, now: DateTime<Utc>) -> Vec<Lesson> {
    let mut learned = vec![];
    for (id, (mut lesson, mut seen)) in repeats(s, now) {
        let enough = match lesson.kind {
            LessonKind::Luck | LessonKind::Back => REPEATS,
            LessonKind::Fix | LessonKind::Setting | LessonKind::Build => MISSES,
        };
        if seen.len() < enough {
            continue;
        }
        seen.sort_by(|a, b| b.at.cmp(&a.at));
        lesson.count = seen.len();
        lesson.from = seen.iter().take(MAX_FROM).map(|x| x.from.clone()).collect();
        lesson.fixes = seen
            .iter()
            .filter(|x| valid_sha(&x.fix))
            .take(MAX_FROM)
            .map(|x| x.fix.clone())
            .collect();
        lesson.seen_at = seen[0].at.clone();
        let (what, next) = lesson_text(&lesson, &str::to_string);
        lesson.what = what;
        lesson.next = next;
        match s.lessons.iter_mut().find(|l| l.id == id) {
            Some(known) => {
                lesson.learned_at = known.learned_at.clone();
                if *known != lesson {
                    *known = lesson;
                }
            }
            None => {
                lesson.learned_at = stamp(now);
                s.lessons.push(lesson.clone());
                learned.push(lesson);
            }
        }
    }
    // One with no room is never told of.
    cap(s, now);
    learned.retain(|l| s.lessons.iter().any(|k| k.id == l.id));
    learned
}

/// Keeps at most `MAX_LESSONS`, the latest seen first. One let go for room
/// is learned again only from what happens after now, as one you forgot
/// is, so the same repeats don't teach it again. Says whether it let any
/// go.
fn cap(s: &mut Snapshot, now: DateTime<Utc>) -> bool {
    s.lessons
        .sort_by(|a, b| b.seen_at.cmp(&a.seen_at).then_with(|| a.id.cmp(&b.id)));
    if s.lessons.len() <= MAX_LESSONS {
        return false;
    }
    for l in s.lessons.split_off(MAX_LESSONS) {
        s.forgotten_lessons.retain(|f| f.id != l.id);
        s.forgotten_lessons.push(Forgot {
            id: l.id,
            at: stamp(now),
        });
    }
    true
}

/// Whether a lesson is about a problem: bad luck in one of its failing
/// checks, its default branch breaking again, or, in its repository, fixes that didn't
/// hold, or for a release build's, builds that failed after their tag.
pub fn about(l: &Lesson, p: &Problem) -> bool {
    if !same_repo(&l.repo, &p.repo) || !tracked(p.kind) {
        return false;
    }
    match l.kind {
        LessonKind::Luck => p.failing.iter().any(|f| l.id == luck_id(&p.repo, &f.name)),
        LessonKind::Back => l.id == format!("back:{}", p.key).to_lowercase(),
        LessonKind::Fix => true,
        LessonKind::Build => p.kind == ProblemKind::ReleaseBuild,
        LessonKind::Setting => false,
    }
}

/// Gives each open problem about failing checks the lessons about it.
/// Returns whether any changed.
pub fn apply(s: &mut Snapshot) -> bool {
    let mut changed = false;
    for p in s.problems.iter_mut().filter(|p| p.is_open()) {
        let mut lessons: Vec<Lesson> = s.lessons.iter().filter(|l| about(l, p)).cloned().collect();
        lessons.sort_by(|a, b| a.id.cmp(&b.id));
        if p.lessons != lessons {
            p.lessons = lessons;
            changed = true;
        }
    }
    changed
}

/// What an open problem has seen before, one line each, for the agent:
/// what happened last time and the lessons about it.
pub fn seen_before(p: &Problem, show: Show) -> Vec<String> {
    if !p.is_open() {
        return vec![];
    }
    let last = p.last_time.as_ref().map(|l| last_time_text(l, show));
    let lessons = p.lessons.iter().map(|l| {
        let (what, next) = lesson_text(l, show);
        format!("{what} {next}")
    });
    last.into_iter().chain(lessons).collect()
}

/// Forgets a lesson, so it is learned again only from what happens after
/// now, and takes it off the problems it was about. Says whether Orbit had
/// it.
pub fn forget(s: &mut Snapshot, id: &str, now: DateTime<Utc>) -> bool {
    let before = s.lessons.len();
    s.lessons.retain(|l| l.id != id);
    if s.lessons.len() == before {
        return false;
    }
    s.forgotten_lessons.retain(|f| f.id != id);
    s.forgotten_lessons.push(Forgot {
        id: id.to_string(),
        at: stamp(now),
    });
    for p in &mut s.problems {
        p.lessons.retain(|l| l.id != id);
    }
    true
}

/// Lets go of cases whose problem closed more than `KEEP_DAYS` ago, or
/// that are still open with their problem gone, and of lessons not seen
/// for `KEEP_DAYS`, keeping at most `MAX_CASES` and `MAX_LESSONS`, the
/// newest. What you forgot is let go after `KEEP_DAYS`, as what it was
/// learned from is gone by then, and the jobs a case saw fail once its
/// problem can't come back as the same one. Says whether it let any go.
pub fn prune(s: &mut Snapshot, now: DateTime<Utc>) -> bool {
    let old = |at: &str| time(at).is_none_or(|at| now - at > Duration::days(KEEP_DAYS));
    let mut settled = false;
    for c in &mut s.cases {
        let done = c
            .closed_at
            .as_deref()
            .and_then(time)
            .is_some_and(|t| now - t >= FLAP_QUIET);
        if done && (!c.failed.is_empty() || c.unsure) {
            c.failed.clear();
            c.unsure = false;
            settled = true;
        }
    }
    let before = (s.cases.len(), s.lessons.len(), s.forgotten_lessons.len());
    let problems = &s.problems;
    s.cases.retain(|c| match &c.closed_at {
        Some(at) => !old(at),
        None => problems.iter().any(|p| p.id == c.id),
    });
    s.cases.sort_by(|a, b| {
        let at = |c: &Case| c.closed_at.clone().unwrap_or_else(|| "~".into());
        at(b).cmp(&at(a))
    });
    s.cases.truncate(MAX_CASES);
    s.lessons.retain(|l| !old(&l.seen_at));
    s.forgotten_lessons.retain(|f| !old(&f.at));
    let capped = cap(s, now);
    settled || capped || before != (s.cases.len(), s.lessons.len(), s.forgotten_lessons.len())
}

#[cfg(test)]
#[path = "cases_tests.rs"]
mod tests;
