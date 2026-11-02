//! Problems: one record per thing that is wrong, such as a default branch
//! whose CI fails, kept from when Orbit first sees it until Orbit sees it
//! fixed. Each has a short timeline of what changed. A failing default
//! branch tells you once, through `crate::alerts`; failing checks on a pull
//! request are its author's to fix, so they are only listed, as is a pull
//! request whose checks passed before its base branch moved on, and a
//! repository where no CI runs on pull requests. Runs that wait a long time
//! for a runner tell you like a failing default branch, as only a person
//! can add or fix a runner. A repository's open security alerts of each
//! kind are a problem too (`see_security`); those only a person should see
//! to tell you once each. So are risky settings in a repository's workflow
//! files (`see_risks`), which are only listed.
//!
//! Everything here is worked out from what Orbit read; nothing reaches
//! GitHub.
use crate::{
    alerts::{self, Raise, Standing},
    ci::{self, Check, CiState, CommitCi, RepoCi},
    runners::{self, Waiting},
    types::*,
    workflows::Setup,
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
    /// A pull request's checks pass, but they began before its base
    /// branch's latest commit, so they ran without it.
    BaseMoved,
    /// No CI runs on a repository's pull requests (see `crate::workflows`).
    NoCi,
    /// A repository's runs have waited a long time for a runner (see
    /// `crate::runners`).
    Waiting,
    /// A repository's open security alerts of one kind (see
    /// `crate::security`).
    Security,
    /// Risky settings in a repository's workflow files (see
    /// `crate::risks`).
    WorkflowRisks,
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
    /// request, `base:owner/name#12` for its base moving on,
    /// `setup:owner/name` for a repository with no CI on pull requests,
    /// `wait:owner/name` for its runs waiting for a runner,
    /// `security:owner/name:dependabot` for its security alerts of a kind,
    /// and `workflows:owner/name` for risky settings in its workflows.
    pub key: String,
    pub kind: ProblemKind,
    pub repo: String,
    pub pr_id: Option<String>,
    /// The default branch, for a default branch's problem; the pull
    /// request's branch for a pull request's.
    pub branch: String,
    /// The latest commit seen failing, and the first since it last passed.
    pub sha: String,
    pub since_sha: String,
    pub failing: Vec<Check>,
    pub opened_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    /// Why it closed, such as "Passed at 1a2b3c4."
    pub closed_reason: Option<String>,
    /// The commit it passed at, when it closed by passing.
    pub closed_sha: String,
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
    /// Where a default branch broke, once Orbit has read its history (see
    /// `crate::broke`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broke: Option<crate::broke::Broke>,
    /// For a pull request whose base moved: the base branch, and its latest
    /// commit, which the checks ran without.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base_sha: String,
    /// For a repository with no CI on pull requests: what Orbit found at
    /// `sha` on `branch`, and the workflow it proposes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<crate::workflows::Found>,
    /// For runs waiting for a runner: those waiting when Orbit last read
    /// them, the longest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waiting: Vec<Waiting>,
    /// For security alerts: which kind, those open when Orbit last read
    /// them, the most severe first, and those that need you it told you
    /// about, by number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::security::Source>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub security_alerts: Vec<crate::security::Alert>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub told_alerts: Vec<u64>,
    /// For security alerts: GitHub said the repository is private when
    /// Orbit last read them, so its agent may hear of them in an issue.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    /// For security alerts: those that left GitHub's list without GitHub
    /// saying they were fixed, dismissed or resolved, as it no longer had
    /// them, by number.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vanished: Vec<u64>,
    /// For risky workflow settings: those Orbit found when it last read the
    /// files at `sha`, the most severe first, and how many it has numbered,
    /// so a number is never used twice.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<crate::risks::Risk>,
    #[serde(skip_serializing_if = "is_zero")]
    pub risk_count: u32,
}
pub(crate) fn is_zero(n: &u32) -> bool {
    *n == 0
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
    /// author's to fix: security alerts only while one of them needs you.
    pub fn needs_you(&self) -> bool {
        match self.kind {
            ProblemKind::DefaultBranch | ProblemKind::Waiting => true,
            ProblemKind::Security => self
                .security_alerts
                .iter()
                .any(crate::security::Alert::needs_you),
            _ => false,
        }
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
        self.closed_sha.clear();
        self.alert_due = None;
    }
    /// Whether it closed because its commit passed.
    pub fn passed(&self) -> bool {
        !self.is_open() && !self.closed_sha.is_empty()
    }
    /// Whether only the repository's own people may hear of it, as it
    /// shows where a weakness is: security alerts and risky workflow
    /// settings.
    pub fn sensitive(&self) -> bool {
        matches!(
            self.kind,
            ProblemKind::Security | ProblemKind::WorkflowRisks
        )
    }
    /// Whether it closed with none of its security alerts left open,
    /// rather than as Orbit stopped watching them.
    pub fn cleared(&self) -> bool {
        self.kind == ProblemKind::Security && !self.is_open() && self.security_alerts.is_empty()
    }
    /// Whether it closed as GitHub said each of its security alerts was
    /// fixed, dismissed or resolved: cleared, with none of them gone from
    /// GitHub without a word.
    pub fn resolved(&self) -> bool {
        self.cleared() && self.vanished.is_empty()
    }
}

pub(crate) fn time(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}
pub(crate) fn stamp(t: DateTime<Utc>) -> String {
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
/// The key of a pull request's problem about its base moving on.
pub fn base_key(pr_id: &str) -> String {
    format!("base:{pr_id}")
}
/// The key of a repository's problem about no CI running on its pull
/// requests.
pub fn setup_key(repo: &str) -> String {
    format!("setup:{}", repo.to_ascii_lowercase())
}
/// The key of a repository's problem about runs waiting for a runner.
pub fn waiting_key(repo: &str) -> String {
    format!("wait:{}", repo.to_ascii_lowercase())
}
/// The key of a repository's problem about its security alerts of a kind.
pub fn security_key(repo: &str, source: crate::security::Source) -> String {
    format!("security:{}:{}", repo.to_ascii_lowercase(), source.key())
}
/// The key of a repository's problem about risky settings in its workflow
/// files.
pub fn workflows_key(repo: &str) -> String {
    format!("workflows:{}", repo.to_ascii_lowercase())
}
/// Runs waiting for a runner in a sentence: "A run of CI in acme/api has
/// waited more than 30 minutes for a runner."
pub fn waiting_text(repo: &str, waiting: &[Waiting]) -> String {
    let minutes = runners::STUCK.num_minutes();
    let mut names: Vec<&str> = vec![];
    for w in waiting.iter().filter(|w| !w.name.is_empty()) {
        if !names.contains(&w.name.as_str()) {
            names.push(&w.name);
        }
    }
    let names = match names.as_slice() {
        [] => String::new(),
        [one] => (*one).into(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    };
    match (waiting.len(), names.is_empty()) {
        (1, false) => format!(
            "A run of {names} in {repo} has waited more than {minutes} minutes for a runner."
        ),
        (1, true) => {
            format!("A run in {repo} has waited more than {minutes} minutes for a runner.")
        }
        (n, false) => format!(
            "{n} runs in {repo} have waited more than {minutes} minutes for a runner: {names}."
        ),
        (n, true) => {
            format!("{n} runs in {repo} have waited more than {minutes} minutes for a runner.")
        }
    }
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
        see_base(problems, ci, pr, &pull.ci, now);
    }
    if !ci.complete {
        return;
    }
    let pulls = [ProblemKind::PullRequest, ProblemKind::BaseMoved];
    for p in problems
        .iter_mut()
        .filter(|p| p.is_open() && pulls.contains(&p.kind) && same_repo(&p.repo, &ci.repo))
    {
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

/// Whether a pull request's checks that passed began before its base
/// branch's latest commit was made, so they ran without it, or None when
/// Orbit can't tell: its base isn't the default branch, or GitHub didn't
/// say when.
pub(crate) fn behind(ci: &RepoCi, pr: &PullRequest, pull: &CommitCi) -> Option<bool> {
    if ci.branch.is_empty() || pr.base_ref != ci.branch {
        return None;
    }
    let ran = time(&pull.ran_at)?;
    let made = time(&ci.head.committed_at)?;
    Some(made > ran)
}

/// Opens, updates or closes the problem about a pull request's base moving
/// on since its checks began. It closes once they run again with the
/// base's latest, or fail, or when its base is another branch now, such as
/// after the default branch was renamed; then one about the new base may
/// open at once.
fn see_base(
    problems: &mut Vec<Problem>,
    ci: &RepoCi,
    pr: &PullRequest,
    pull: &CommitCi,
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    let key = base_key(&pr.id);
    let short = ci::short(&pull.sha).to_string();
    let base = ci.branch.as_str();
    let renamed = |p: &Problem| p.base != base || pr.base_ref != p.base;
    if let Some(p) = problems
        .iter_mut()
        .find(|p| p.key == key && p.is_open() && renamed(p))
    {
        p.close(&at, format!("Its base is no longer {}.", p.base));
    }
    let open = problems.iter_mut().find(|p| p.key == key && p.is_open());
    match (pull.state, behind(ci, pr, pull), open) {
        (CiState::Passing, Some(true), Some(p)) => {
            if p.sha != pull.sha {
                p.note(
                    &at,
                    format!("Passed again at {short}, still without {base}'s latest."),
                );
            } else if p.base_sha != ci.head.sha {
                let moved = ci::short(&ci.head.sha);
                p.note(&at, format!("{base} moved on again, to {moved}."));
            }
            p.sha = pull.sha.clone();
            p.base_sha = ci.head.sha.clone();
        }
        (CiState::Passing, Some(true), None) => {
            let mut p = Problem {
                id: id(),
                key,
                kind: ProblemKind::BaseMoved,
                repo: ci.repo.clone(),
                pr_id: Some(pr.id.clone()),
                branch: pr.head_ref.clone(),
                sha: pull.sha.clone(),
                since_sha: pull.sha.clone(),
                opened_at: at.clone(),
                base: base.to_string(),
                base_sha: ci.head.sha.clone(),
                ..Default::default()
            };
            p.note(
                &at,
                format!(
                    "Passed at {short}, but {base} moved on to {} after its checks began.",
                    ci::short(&ci.head.sha)
                ),
            );
            problems.push(p);
        }
        (CiState::Passing, Some(false), Some(p)) => {
            p.close(&at, format!("Passed at {short} with {base}'s latest."));
            p.closed_sha = pull.sha.clone();
        }
        (CiState::Failing, _, Some(p)) => p.close(&at, format!("Its checks fail at {short} now.")),
        // Running, or Orbit can't tell yet: wait.
        _ => {}
    }
}

/// Opens, updates or closes the problem about no CI running on a
/// repository's pull requests, from what Orbit read of its files at the
/// default branch's latest commit (`setup`) and the checks it just read.
/// It opens only when the files say so for sure, nothing ran checks on a
/// commit Orbit follows (the default branch's, when there are no
/// workflows at all, and each followed pull request's), and `QUIET` has
/// passed since Orbit read them, so another service's checks have time to
/// show up. It closes once the files or the checks say otherwise. Checks
/// Orbit saw are kept on `setup` (`Setup::ran`), so the problem doesn't
/// open again once that pull request closes or the branch moves on.
pub fn see_setup(
    problems: &mut Vec<Problem>,
    setup: Option<&mut Setup>,
    ci: &RepoCi,
    followed: &[&PullRequest],
    now: DateTime<Utc>,
) {
    use crate::workflows::{Verdict, QUIET};
    // Only what was read at the commit and branch Orbit sees now counts.
    let now_read = |s: &&mut Setup| s.sha == ci.head.sha && s.branch == ci.branch;
    let Some(setup) = setup.filter(|s| now_read(s) && !s.sha.is_empty()) else {
        return;
    };
    let Some(verdict) = setup.verdict() else {
        return;
    };
    let at = stamp(now);
    let short = ci::short(&setup.sha).to_string();
    let branch = if setup.branch.is_empty() {
        "the default branch".to_string()
    } else {
        setup.branch.clone()
    };
    // A followed pull request whose latest commit has checks.
    let checked = followed.iter().find_map(|pr| {
        ci.pull(pr.number)
            .filter(|p| p.ci.sha == pr.head_sha && p.ci.state != CiState::None)
            .map(|_| pr.number)
    });
    let on_branch = ci.head.state != CiState::None;
    // Checks show something runs them, at this commit or a later one.
    let ran = match (&verdict, checked) {
        (_, Some(n)) => Some(format!("Checks ran on #{n}, so something runs them.")),
        (Verdict::NoWorkflows, None) if on_branch => Some(format!(
            "Checks ran on the default branch at {short}, so something runs them."
        )),
        _ => None,
    };
    if setup.ran.is_none() {
        setup.ran = ran;
    }
    let key = setup_key(&ci.repo);
    let open = problems.iter_mut().find(|p| p.key == key && p.is_open());
    // Why it closes: a note that names the file, and a reason in Orbit's
    // own words, as the reason goes on GitHub.
    let missing = match (&verdict, &setup.ran) {
        (Verdict::Covered(path), _) => Err((
            if path.starts_with(crate::workflows::FOLDER) {
                format!("{path} runs on pull requests.")
            } else {
                format!("{path} sets up another CI service.")
            },
            format!("CI runs on pull requests at {short}."),
        )),
        (Verdict::Unknown(path), _) => Err((
            format!("Orbit can't tell whether {path} runs on pull requests."),
            format!("Orbit can't tell any more that nothing runs on pull requests, at {short}."),
        )),
        (_, Some(ran)) => Err((String::new(), ran.clone())),
        _ => Ok(()),
    };
    match (missing, open) {
        (Ok(()), Some(p)) => {
            let found = setup.found();
            if p.found.as_ref() != Some(&found) {
                p.note(&at, format!("What Orbit found changed at {short}."));
            }
            p.sha = setup.sha.clone();
            p.branch = setup.branch.clone();
            p.found = Some(found);
        }
        (Ok(()), None) => {
            let read = time(&setup.read_at);
            if read.is_none_or(|read| now - read < QUIET) {
                return;
            }
            let text = match verdict {
                Verdict::NoWorkflows => format!("No workflows on {branch} at {short}."),
                _ => format!("No workflow on {branch} runs on pull requests, at {short}."),
            };
            let mut p = Problem {
                id: id(),
                key,
                kind: ProblemKind::NoCi,
                repo: ci.repo.clone(),
                branch: setup.branch.clone(),
                sha: setup.sha.clone(),
                since_sha: setup.sha.clone(),
                opened_at: at.clone(),
                found: Some(setup.found()),
                ..Default::default()
            };
            p.note(&at, text);
            problems.push(p);
        }
        (Err((why, reason)), Some(p)) => {
            // The files say CI runs now; checks Orbit saw only say
            // something ran them.
            let covered = matches!(verdict, Verdict::Covered(_));
            if !why.is_empty() {
                p.note(&at, why);
            }
            p.close(&at, reason);
            if covered {
                p.closed_sha = setup.sha.clone();
            }
        }
        (Err(_), None) => {}
    }
}

/// A workflows problem in a sentence: "The workflows in acme/api have 3
/// risky settings, 1 of them high."
pub fn risks_text(p: &Problem) -> String {
    let (many, high) = crate::risks::count(&p.risks);
    format!("The workflows in {} have {many}{high}.", p.repo)
}
/// Opens, updates or closes the problem about risky settings in a
/// repository's workflow files, from what Orbit just read of them at its
/// default branch's latest commit (see `crate::risks`). A risk is the same
/// one wherever it moves in its file, and keeps its number; one that comes
/// back after it went is new. What Orbit found before in a file it
/// couldn't check now, or in one with more risks than it keeps
/// (`crate::risks::MAX_PER_FILE`), stays, as Orbit can't say it was
/// fixed; so does what a tool found in a file still there when the tool
/// didn't run or couldn't check that file (`crate::lint`). A file that went
/// takes its risks with it. It closes,
/// as passed at that commit, once nothing is left.
pub fn see_risks(problems: &mut Vec<Problem>, setup: &Setup, now: DateTime<Utc>) {
    use crate::risks::{names, sort};
    if setup.error.is_some() || setup.sha.is_empty() || setup.checks == 0 {
        return;
    }
    let at = stamp(now);
    let short = ci::short(&setup.sha).to_string();
    let key = workflows_key(&setup.repo);
    // Files where a risk Orbit found before but not now may still be
    // there: ones it couldn't check, and ones with more than it keeps.
    let unseen: Vec<&str> = setup
        .workflows
        .iter()
        .filter(|w| !w.checked || w.more > 0)
        .map(|w| w.path.as_str())
        .collect();
    // Nor is what a tool found before in a file that is still there, when
    // it didn't run this time or couldn't check that file.
    let ran: Vec<crate::risks::RiskKind> = setup
        .tools
        .iter()
        .filter(|t| t.error.is_empty())
        .map(|t| t.tool)
        .collect();
    let unchecked_by = |r: &crate::risks::Risk| {
        let file = setup.workflows.iter().find(|w| w.path == r.path);
        r.kind.tool()
            && file.is_some_and(|w| {
                !ran.contains(&r.kind) || w.missed.iter().any(|m| m.tool == r.kind)
            })
    };
    let found: Vec<&crate::risks::Risk> = setup.workflows.iter().flat_map(|w| &w.risks).collect();
    let fresh = |p: &mut Problem, r: &crate::risks::Risk| {
        p.risk_count += 1;
        let mut r = r.clone();
        r.number = p.risk_count;
        r.since = setup.sha.clone();
        r
    };
    let Some(p) = problems.iter_mut().find(|p| p.key == key && p.is_open()) else {
        if found.is_empty() {
            return;
        }
        let mut p = Problem {
            id: id(),
            key,
            kind: ProblemKind::WorkflowRisks,
            repo: setup.repo.clone(),
            branch: setup.branch.clone(),
            sha: setup.sha.clone(),
            since_sha: setup.sha.clone(),
            opened_at: at.clone(),
            private: setup.private,
            ..Default::default()
        };
        for r in found {
            let r = fresh(&mut p, r);
            p.risks.push(r);
        }
        sort(&mut p.risks);
        let many = match p.risks.len() {
            1 => "A risky workflow setting".to_string(),
            n => format!("{n} risky workflow settings"),
        };
        let text = format!("{many} at {short}: {}.", names(&p.risks));
        p.note(&at, text);
        problems.push(p);
        return;
    };
    p.sha = setup.sha.clone();
    p.branch = setup.branch.clone();
    p.private = setup.private;
    let mut gone = vec![];
    p.risks.retain(|r| {
        let stays =
            unseen.contains(&r.path.as_str()) || unchecked_by(r) || found.iter().any(|f| f.same(r));
        if !stays {
            gone.push(r.clone());
        }
        stays
    });
    let mut new = vec![];
    for f in found {
        match p.risks.iter_mut().find(|r| r.same(f)) {
            Some(r) => {
                r.line = f.line;
                r.severity = f.severity;
            }
            None => new.push(fresh(p, f)),
        }
    }
    if !gone.is_empty() {
        let text = format!("Fixed at {short}: {}.", names(&gone));
        if p.risks.is_empty() && new.is_empty() {
            p.note(&at, text);
            p.close(&at, format!("No risky workflow settings at {short}."));
            p.closed_sha = setup.sha.clone();
            return;
        }
        p.note(&at, text);
    }
    if !new.is_empty() {
        sort(&mut new);
        p.note(&at, format!("New at {short}: {}.", names(&new)));
        p.risks.extend(new);
    }
    sort(&mut p.risks);
}

/// Opens, updates or closes the problem about a repository's runs waiting
/// for a runner, from what Orbit just read of them (see `crate::runners`):
/// `all` says whether it read every queued run, as it closes the problem
/// only then. It tells you when it opens and reminds you after
/// `REMIND_AFTER`; one that clears and comes back within `FLAP_QUIET` stays
/// quiet then, as a failing default branch does.
pub fn see_waiting(
    problems: &mut Vec<Problem>,
    repo: &str,
    waiting: &[Waiting],
    all: bool,
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    let key = waiting_key(repo);
    let open = problems.iter_mut().find(|p| p.key == key && p.is_open());
    match (waiting.is_empty(), open) {
        (true, Some(p)) if all => p.close(&at, "No runs wait for a runner now.".into()),
        // A run Orbit didn't read may still wait.
        (true, _) => {}
        (false, Some(p)) => {
            if p.waiting.len() != waiting.len() {
                let now_waiting = match waiting.len() {
                    1 => "Now 1 run waits for a runner.".to_string(),
                    n => format!("Now {n} runs wait for a runner."),
                };
                p.note(&at, now_waiting);
            }
            p.waiting = waiting.to_vec();
            let due = p.alert_due.as_deref().and_then(time);
            let standing = match p.last_alert_at.as_deref().and_then(time) {
                Some(last_alert) => Standing::Alerted { last_alert },
                None => Standing::New,
            };
            if due.is_some_and(|due| now >= due)
                || raise(standing, false, now, REMIND_AFTER) == Raise::Alert
            {
                p.alert(&at);
            }
        }
        (false, None) => {
            // One that cleared a moment ago is the same problem back again.
            let back = problems
                .iter_mut()
                .filter(|p| {
                    p.key == key
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
                    p.waiting = waiting.to_vec();
                    p.note(&at, format!("Again: {}", waiting_text(repo, waiting)));
                    let standing = match p.last_alert_at.as_deref().and_then(time) {
                        Some(last_alert) => Standing::Returned { last_alert },
                        None => Standing::New,
                    };
                    match raise(standing, false, now, FLAP_QUIET) {
                        Raise::Alert => p.alert(&at),
                        Raise::AlertAfter(due) => p.alert_due = Some(stamp(due)),
                        Raise::Quiet => {}
                    }
                }
                None => {
                    let mut p = Problem {
                        id: id(),
                        key,
                        kind: ProblemKind::Waiting,
                        repo: repo.to_string(),
                        opened_at: at.clone(),
                        waiting: waiting.to_vec(),
                        ..Default::default()
                    };
                    p.note(&at, waiting_text(repo, waiting));
                    p.alert(&at);
                    problems.push(p);
                }
            }
        }
    }
}

/// Opens, updates or closes the problems about a repository's security
/// alerts, one for each kind, from what Orbit just read (`read`) and what
/// it looked up of the alerts that left a list (`left`). An alert leaves
/// its problem only once GitHub says it's no longer open, and the timeline
/// says what became of it; the problem closes once none is left. It tells
/// you once for each alert that needs you (`security::Alert::needs_you`),
/// including one that came to need you. A kind Orbit can't see now closes
/// its problem, saying so, as Orbit can no longer tell what became of it.
pub fn see_security(
    problems: &mut Vec<Problem>,
    read: &crate::security::RepoSecurity,
    left: &[crate::security::Left],
    now: DateTime<Utc>,
) {
    use crate::security::{open_text, outcome_text, sort, Alert, Outcome};
    let at = stamp(now);
    for kind in &read.kinds {
        let key = security_key(&read.repo, kind.source);
        let open = problems.iter_mut().find(|p| p.key == key && p.is_open());
        if let Some(why) = &kind.unseen {
            if let Some(p) = open {
                p.close(
                    &at,
                    format!("{why} Orbit can't see what became of its alerts now."),
                );
            }
            continue;
        }
        let Some(p) = open else {
            if kind.alerts.is_empty() {
                continue;
            }
            let mut p = Problem {
                id: id(),
                key,
                kind: ProblemKind::Security,
                repo: read.repo.clone(),
                opened_at: at.clone(),
                source: Some(kind.source),
                security_alerts: kind.alerts.clone(),
                private: read.private,
                ..Default::default()
            };
            sort(&mut p.security_alerts);
            p.note(&at, open_text(kind.source, &p.security_alerts));
            tell(&mut p, &at);
            problems.push(p);
            continue;
        };
        p.private = read.private;
        // Those no longer listed go only once GitHub says what became of
        // them; until then they stay as they were.
        let mut gone = vec![];
        let mut vanished = vec![];
        p.security_alerts.retain(|a| {
            if kind.alerts.iter().any(|b| b.number == a.number) {
                return true;
            }
            match left
                .iter()
                .find(|l| l.source == kind.source && l.number == a.number)
            {
                Some(l) if l.outcome != Outcome::Open => {
                    gone.push(outcome_text(a, &l.outcome));
                    if l.outcome == Outcome::Gone {
                        vanished.push(a.number);
                    }
                    false
                }
                _ => true,
            }
        });
        p.vanished.extend(vanished);
        // One listed again is open, whatever GitHub said before.
        p.vanished
            .retain(|n| !kind.alerts.iter().any(|a| a.number == *n));
        let mut new: Vec<Alert> = vec![];
        for a in &kind.alerts {
            match p.security_alerts.iter_mut().find(|b| b.number == a.number) {
                Some(b) => *b = a.clone(),
                None => new.push(a.clone()),
            }
        }
        if p.security_alerts.is_empty() && new.is_empty() {
            let reason = match gone.as_slice() {
                [one] => one.clone(),
                _ => {
                    for text in &gone {
                        p.note(&at, text.clone());
                    }
                    format!("No {} are open on GitHub now.", kind.source.many())
                }
            };
            p.close(&at, reason);
            continue;
        }
        for text in gone {
            p.note(&at, text);
        }
        if !new.is_empty() {
            sort(&mut new);
            let names: Vec<String> = new.iter().map(Alert::name).collect();
            let text = match names.as_slice() {
                [one] => format!("New: {one}."),
                [a, b] => format!("New: {a} and {b}."),
                [a, b, rest @ ..] => format!("New: {a}, {b} and {} more.", rest.len()),
                [] => unreachable!(),
            };
            p.note(&at, text);
            p.security_alerts.extend(new);
        }
        sort(&mut p.security_alerts);
        tell(p, &at);
    }
}
/// Records what GitHub said just now about whether a repository is
/// private, on each of its problems that only its own people may hear of
/// (`Problem::sensitive`), open or closed, and on what Orbit read of its
/// alerts and workflows.
pub fn see_private(
    problems: &mut [Problem],
    security: &mut [crate::security::RepoSecurity],
    setups: &mut [Setup],
    repo: &str,
    private: bool,
) {
    let theirs = problems
        .iter_mut()
        .filter(|p| p.sensitive() && same_repo(&p.repo, repo));
    for p in theirs {
        p.private = private;
    }
    for r in security.iter_mut().filter(|r| same_repo(&r.repo, repo)) {
        r.private = private;
    }
    for s in setups.iter_mut().filter(|s| same_repo(&s.repo, repo)) {
        s.private = private;
    }
}
/// Notes when Orbit asked GitHub about each of a repository's alerts in
/// `asked`, whatever it answered, so the next pass asks about the others
/// first (`security::to_look_up`).
pub fn looked_up(
    problems: &mut [Problem],
    repo: &str,
    asked: &[(crate::security::Source, u64)],
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    for &(source, number) in asked {
        let key = security_key(repo, source);
        let alert = problems
            .iter_mut()
            .filter(|p| p.key == key && p.is_open())
            .flat_map(|p| p.security_alerts.iter_mut())
            .find(|a| a.number == number);
        if let Some(a) = alert {
            a.looked_at = at.clone();
        }
    }
}
/// Tells you about a security problem's alerts that need you and that it
/// hasn't told you about, and forgets those no longer open, so one that
/// opens again tells you again.
fn tell(p: &mut Problem, at: &str) {
    let open: Vec<u64> = p.security_alerts.iter().map(|a| a.number).collect();
    p.told_alerts.retain(|n| open.contains(n));
    let fresh: Vec<u64> = p
        .security_alerts
        .iter()
        .filter(|a| a.needs_you() && !p.told_alerts.contains(&a.number))
        .map(|a| a.number)
        .collect();
    if !fresh.is_empty() {
        p.told_alerts.extend(fresh);
        p.alert(at);
    }
}
/// Closes every open security problem, once you turn Watch security alerts
/// off.
pub fn close_security(problems: &mut [Problem], now: DateTime<Utc>) {
    let at = stamp(now);
    for p in problems
        .iter_mut()
        .filter(|p| p.is_open() && p.kind == ProblemKind::Security)
    {
        p.close(&at, "Orbit stopped watching security alerts.".into());
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
        (CiState::Passing, Some(p)) => {
            p.close(&at, format!("Passed at {short}."));
            p.closed_sha = seen.ci.sha.clone();
        }
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
                    p.closed_sha.clear();
                    p.sha = seen.ci.sha.clone();
                    // It passed in between, so whatever broke it now came
                    // after that.
                    p.since_sha = seen.ci.sha.clone();
                    p.broke = None;
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
/// and every open problem about CI when `everything` is set, such as when
/// you turn Watch CI off. Security alerts have a switch of their own
/// (`close_security`).
pub fn close_unwatched(
    problems: &mut [Problem],
    connected: &[String],
    everything: bool,
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    for p in problems.iter_mut().filter(|p| p.is_open()) {
        let security = p.kind == ProblemKind::Security;
        if everything && !security {
            p.close(&at, "Orbit stopped watching CI.".into());
        } else if !everything && !connected.iter().any(|r| same_repo(r, &p.repo)) {
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
/// Problems in `pinned` stay whatever their age, as Orbit still has a
/// comment or issue to finish for them (see `crate::ops::pinned`). Returns
/// whether any went.
pub fn prune(
    problems: &mut Vec<Problem>,
    pinned: &std::collections::HashSet<String>,
    now: DateTime<Utc>,
) -> bool {
    let before = problems.len();
    problems.retain(|p| {
        pinned.contains(&p.id)
            || p.closed_at
                .as_deref()
                .and_then(time)
                .is_none_or(|closed| now - closed < KEEP_CLOSED)
    });
    let mut closed: Vec<(String, String)> = problems
        .iter()
        .filter(|p| !pinned.contains(&p.id))
        .filter_map(|p| p.closed_at.clone().map(|at| (at, p.id.clone())))
        .collect();
    if closed.len() > MAX_CLOSED {
        closed.sort_by(|a, b| b.cmp(a));
        let keep: std::collections::HashSet<String> = closed
            .into_iter()
            .take(MAX_CLOSED)
            .map(|(_, id)| id)
            .collect();
        problems
            .retain(|p| p.closed_at.is_none() || pinned.contains(&p.id) || keep.contains(&p.id));
    }
    problems.len() != before
}

#[cfg(test)]
#[path = "problems_tests.rs"]
mod tests;
