//! Problems: one record per thing that is wrong, such as a default branch
//! whose CI fails, kept from when Orbit first sees it until Orbit sees it
//! fixed. Each has a short timeline of what changed. A failing default
//! branch tells you once, through `crate::alerts`; failing checks on a pull
//! request are its author's to fix, so they are only listed.
//!
//! Everything here is worked out from what Orbit read; nothing reaches
//! GitHub.
use crate::{
    alerts::{self, Raise, Standing},
    ci::{self, Check, CiState, CommitCi, RepoCi},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How long a problem that is still open stays quiet before it reminds you.
pub const REMIND_AFTER: Duration = Duration::hours(24);
/// How long after a problem's alert it stays quiet if it clears and comes
/// back, so a flaky branch doesn't notify on every run. A problem that comes
/// back later than this after it cleared is a new one.
pub const FLAP_QUIET: Duration = Duration::hours(1);
/// How long closed problems are kept, and how many at most.
pub const KEEP_CLOSED: Duration = Duration::days(14);
pub const MAX_CLOSED: usize = 100;
/// The most timeline entries a problem keeps; the oldest go first.
pub const MAX_TIMELINE: usize = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// CI fails on a repository's default branch.
    #[default]
    DefaultBranch,
    /// CI fails on a pull request's latest commit.
    PullRequest,
}
/// Something that happened to a problem.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProblemEvent {
    pub at: String,
    pub text: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Problem {
    pub id: String,
    /// What the problem is about, the same while it stays the same thing:
    /// `ci:owner/name` for a default branch, `ci:owner/name#12` for a pull
    /// request.
    pub key: String,
    pub kind: ProblemKind,
    pub repo: String,
    pub pr_id: Option<String>,
    /// The default branch, for a default branch's problem.
    pub branch: String,
    /// The latest commit seen failing, and the first.
    pub sha: String,
    pub since_sha: String,
    pub failing: Vec<Check>,
    pub opened_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    /// Why it closed, such as "Passed at 1a2b3c4."
    pub closed_reason: Option<String>,
    /// Oldest first.
    pub timeline: Vec<ProblemEvent>,
    /// How many times it has told you. Its Needs you item changes with it,
    /// so each alert is announced once.
    pub alerts: u32,
    pub last_alert_at: Option<String>,
    /// The commit and how many checks failed when it last told you, to tell
    /// whether it got worse.
    pub alerted_sha: String,
    pub alerted_failing: u32,
    /// When a problem that came back within `FLAP_QUIET` tells you, if it is
    /// still open then.
    pub alert_due: Option<String>,
}
impl Problem {
    pub fn is_open(&self) -> bool {
        self.closed_at.is_none()
    }
    /// The key of its Needs you item, which changes with each alert.
    pub fn item_key(&self) -> String {
        format!("problem:{}:{}", self.id, self.alerts)
    }
    /// Whether it is yours to know about, rather than a pull request
    /// author's to fix.
    pub fn needs_you(&self) -> bool {
        self.kind == ProblemKind::DefaultBranch
    }
    fn note(&mut self, at: &str, text: String) {
        self.timeline.push(ProblemEvent {
            at: at.into(),
            text,
        });
        let excess = self.timeline.len().saturating_sub(MAX_TIMELINE);
        self.timeline.drain(..excess);
        self.updated_at = at.into();
    }
    fn alert(&mut self, at: &str) {
        self.alerts += 1;
        self.last_alert_at = Some(at.into());
        self.alerted_sha = self.sha.clone();
        self.alerted_failing = self.failing.len() as u32;
        self.alert_due = None;
    }
    /// A newer commit fails more checks than when it last told you.
    fn worse(&self) -> bool {
        self.sha != self.alerted_sha && self.failing.len() as u32 > self.alerted_failing
    }
    fn close(&mut self, at: &str, reason: String) {
        self.note(at, reason.clone());
        self.closed_at = Some(at.into());
        self.closed_reason = Some(reason);
        self.alert_due = None;
    }
}

fn time(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}
fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
/// Check names in a sentence: "A", "A and B", "A, B and 2 more".
pub fn names(checks: &[Check]) -> String {
    let all: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
    match all.as_slice() {
        [] => "no checks".into(),
        [one] => (*one).into(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}

/// The key of a repository's default branch problem.
pub fn branch_key(repo: &str) -> String {
    format!("ci:{}", repo.to_ascii_lowercase())
}
/// The key of a pull request's problem.
pub fn pull_key(pr_id: &str) -> String {
    format!("ci:{pr_id}")
}

/// What Orbit saw of one thing that can fail.
struct Seen<'a> {
    key: String,
    kind: ProblemKind,
    repo: &'a str,
    pr_id: Option<String>,
    branch: &'a str,
    ci: &'a CommitCi,
}

/// Updates the problems of one repository from its CI as just read.
/// `followed` are the pull requests whose CI Orbit follows: open, not
/// drafts, and from a source Orbit trusts. A pull request problem closes
/// when the pull request is no longer followed, but only when `ci` lists
/// every open pull request.
pub fn observe(
    problems: &mut Vec<Problem>,
    ci: &RepoCi,
    followed: &[&PullRequest],
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    if !ci.head.sha.is_empty() {
        see(
            problems,
            Seen {
                key: branch_key(&ci.repo),
                kind: ProblemKind::DefaultBranch,
                repo: &ci.repo,
                pr_id: None,
                branch: &ci.branch,
                ci: &ci.head,
            },
            now,
        );
    }
    for pr in followed {
        let Some(pull) = ci.pull(pr.number).filter(|p| p.ci.sha == pr.head_sha) else {
            continue;
        };
        see(
            problems,
            Seen {
                key: pull_key(&pr.id),
                kind: ProblemKind::PullRequest,
                repo: &ci.repo,
                pr_id: Some(pr.id.clone()),
                branch: &pr.head_ref,
                ci: &pull.ci,
            },
            now,
        );
    }
    if !ci.complete {
        return;
    }
    for p in problems.iter_mut().filter(|p| {
        p.is_open() && p.kind == ProblemKind::PullRequest && same_repo(&p.repo, &ci.repo)
    }) {
        let number = p
            .pr_id
            .as_deref()
            .and_then(|id| id.rsplit_once('#'))
            .and_then(|(_, n)| n.parse::<u64>().ok());
        let open = number.and_then(|n| ci.pull(n));
        let reason = match open {
            None => "The pull request closed.",
            Some(pull) if pull.draft => "The pull request is a draft again.",
            Some(_) if !followed.iter().any(|f| Some(&f.id) == p.pr_id.as_ref()) => {
                "Orbit no longer follows this pull request."
            }
            Some(_) => continue,
        };
        p.close(&at, reason.into());
    }
}

/// Opens, updates or closes the problem `seen` is about.
fn see(problems: &mut Vec<Problem>, seen: Seen, now: DateTime<Utc>) {
    let at = stamp(now);
    let short = ci::short(&seen.ci.sha).to_string();
    let open = problems
        .iter_mut()
        .find(|p| p.key == seen.key && p.is_open());
    match (seen.ci.state, open) {
        (CiState::Passing, Some(p)) => p.close(&at, format!("Passed at {short}.")),
        (CiState::Failing, Some(p)) => {
            if p.sha != seen.ci.sha {
                p.note(&at, format!("Still failing at {short}."));
            } else if p.failing != seen.ci.failing {
                p.note(&at, format!("Now failing: {}.", names(&seen.ci.failing)));
            }
            p.sha = seen.ci.sha.clone();
            p.failing = seen.ci.failing.clone();
            if p.needs_you() {
                let due = p.alert_due.as_deref().and_then(time);
                let worse = p.worse();
                let standing = match p.last_alert_at.as_deref().and_then(time) {
                    Some(last_alert) => Standing::Alerted { last_alert },
                    None => Standing::New,
                };
                if due.is_some_and(|due| now >= due)
                    || raise(standing, worse, now, REMIND_AFTER) == Raise::Alert
                {
                    p.alert(&at);
                }
            }
        }
        (CiState::Failing, None) => {
            // One that cleared a moment ago is the same problem back again.
            let back = problems
                .iter_mut()
                .filter(|p| {
                    p.key == seen.key
                        && p.closed_at
                            .as_deref()
                            .and_then(time)
                            .is_some_and(|closed| now - closed < FLAP_QUIET)
                })
                .max_by(|a, b| a.opened_at.cmp(&b.opened_at));
            match back {
                Some(p) => {
                    p.closed_at = None;
                    p.closed_reason = None;
                    p.sha = seen.ci.sha.clone();
                    p.failing = seen.ci.failing.clone();
                    p.note(&at, format!("Failing again at {short}."));
                    if p.needs_you() {
                        let standing = match p.last_alert_at.as_deref().and_then(time) {
                            Some(last_alert) => Standing::Returned { last_alert },
                            None => Standing::New,
                        };
                        // Back worse tells you now, as it would while open.
                        match raise(standing, p.worse(), now, FLAP_QUIET) {
                            Raise::Alert => p.alert(&at),
                            Raise::AlertAfter(due) => p.alert_due = Some(stamp(due)),
                            Raise::Quiet => {}
                        }
                    }
                }
                None => {
                    let mut p = Problem {
                        id: id(),
                        key: seen.key,
                        kind: seen.kind,
                        repo: seen.repo.to_string(),
                        pr_id: seen.pr_id,
                        branch: seen.branch.to_string(),
                        sha: seen.ci.sha.clone(),
                        since_sha: seen.ci.sha.clone(),
                        failing: seen.ci.failing.clone(),
                        opened_at: at.clone(),
                        ..Default::default()
                    };
                    let on = if seen.branch.is_empty() {
                        String::new()
                    } else {
                        format!(" on {}", seen.branch)
                    };
                    p.note(
                        &at,
                        format!("Failing{on} at {short}: {}.", names(&seen.ci.failing)),
                    );
                    if p.needs_you() {
                        p.alert(&at);
                    }
                    problems.push(p);
                }
            }
        }
        // Running, or no checks: wait for an answer either way.
        _ => {}
    }
}

fn raise(standing: Standing, worse: bool, now: DateTime<Utc>, cooldown: Duration) -> Raise {
    alerts::raise(standing, worse, now, cooldown)
}

/// Closes the open problems of a repository that is no longer connected,
/// and every open problem when `everything` is set, such as when you turn
/// Watch CI off.
pub fn close_unwatched(
    problems: &mut [Problem],
    connected: &[String],
    everything: bool,
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    for p in problems.iter_mut().filter(|p| p.is_open()) {
        if everything {
            p.close(&at, "Orbit stopped watching CI.".into());
        } else if !connected.iter().any(|r| same_repo(r, &p.repo)) {
            p.close(&at, "The repository was disconnected.".into());
        }
    }
}

/// The Needs you keys of problems that closed within `FLAP_QUIET`, so one
/// that comes back in that time isn't announced again.
pub fn quiet_keys(problems: &[Problem], now: DateTime<Utc>) -> Vec<String> {
    problems
        .iter()
        .filter(|p| {
            p.needs_you()
                && p.alerts > 0
                && p.closed_at
                    .as_deref()
                    .and_then(time)
                    .is_some_and(|closed| now - closed < FLAP_QUIET)
        })
        .map(Problem::item_key)
        .collect()
}

/// Drops closed problems older than `KEEP_CLOSED`, keeping the newest
/// `MAX_CLOSED` of the rest; problems that closed together are kept by ID.
/// Returns whether any went.
pub fn prune(problems: &mut Vec<Problem>, now: DateTime<Utc>) -> bool {
    let before = problems.len();
    problems.retain(|p| {
        p.closed_at
            .as_deref()
            .and_then(time)
            .is_none_or(|closed| now - closed < KEEP_CLOSED)
    });
    let mut closed: Vec<(String, String)> = problems
        .iter()
        .filter_map(|p| p.closed_at.clone().map(|at| (at, p.id.clone())))
        .collect();
    if closed.len() > MAX_CLOSED {
        closed.sort_by(|a, b| b.cmp(a));
        let keep: std::collections::HashSet<String> = closed
            .into_iter()
            .take(MAX_CLOSED)
            .map(|(_, id)| id)
            .collect();
        problems.retain(|p| p.closed_at.is_none() || keep.contains(&p.id));
    }
    problems.len() != before
}

#[cfg(test)]
#[path = "problems_tests.rs"]
mod tests;
