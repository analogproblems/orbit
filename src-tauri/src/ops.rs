//! Telling a project's agent about failing CI, on GitHub where it already
//! listens: one Orbit comment on each pull request whose checks fail,
//! edited in place, and one issue labelled `orbit` for each default branch
//! that fails. Each failing check is an item, I1, I2 and so on, numbered
//! once in its comment or issue and never reused, so an agent can name it.
//! So is a pull request's base branch moving on since its checks began.
//! A repository where no CI runs on pull requests gets an issue too, with
//! a workflow to start from, which Bit offers before it opens. So does each
//! kind of security alert in a private repository, but secrets (`tells_agent`):
//! an item per alert, which leaves once GitHub says it is fixed.
//!
//! Everything here is worked out from the problems Orbit keeps
//! (`crate::problems`). The engine sends each write (`engine::ops`) after
//! recording it in the journal. A write is never sent twice: before Orbit
//! posts something new, it looks on GitHub for that thing's marker, in case
//! an earlier try reached GitHub just before Orbit stopped, and a thread
//! whose post may have got there looks for it before anything else.
//!
//! Agents answer on the same comment or issue, item by item (`Taking I2`,
//! `Fixed I2 in abc1234`, `Declined I2: reason`, `Need a person on I2:
//! question`; see `hear`). A fix in a named commit is checked against CI
//! there (`to_verify`), and the comment or issue says what Orbit found.
//!
//! On a pull request Orbit reviews automatically, the comment also starts
//! with whether it is clear to merge (`crate::readiness`), with a hidden
//! line naming the head and base that answer is for (`ready`). That line
//! alone is worth a comment once it is clear or waits on someone.
use crate::{
    ci::{short, RepoCi},
    problems::{stamp, time, Problem, ProblemKind, FLAP_QUIET, KEEP_CLOSED},
    readiness::{Readiness, Turn},
    security::Source,
    types::{same_repo, trusted_source, PullRequest, Snapshot},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

/// The label on Orbit's issues.
pub const LABEL: &str = "orbit";
/// How long a default branch stays green before its issue closes. A
/// problem that comes back sooner is the same problem, so a flaky branch
/// leaves its issue open rather than closing and reopening it on every run.
pub const CLOSE_AFTER: Duration = FLAP_QUIET;
/// The most writes one pass sends, for GitHub's limits.
pub const WRITES_PER_PASS: usize = 10;
/// The most items a comment or issue lists; the rest are counted.
pub const MAX_ITEMS: usize = 50;
/// The failing items whose failed step and log lines a comment or issue
/// shows; the rest are on Operations.
pub const MAX_WHY: usize = 5;
/// The most journal entries kept; the oldest go first, and none are kept
/// longer than closed problems are.
pub const MAX_JOURNAL: usize = 200;
/// The longest wait before a failed write is tried again. The wait starts
/// at a minute and doubles with each failure in a row.
pub const MAX_RETRY: Duration = Duration::hours(1);
/// The longest reason or question kept from an answer.
pub const MAX_ANSWER: usize = 500;
/// The most fixes checked against CI in one pass, for GitHub's limits.
pub const CHECKS_PER_PASS: usize = 5;
/// How long a pull request's answer waits on its agent before Orbit
/// reminds it, once, on its comment.
pub const NUDGE_AGENT: Duration = Duration::hours(2);
/// How long it waits on a person, such as a reviewer, before it is in
/// Needs you.
pub const NUDGE_PERSON: Duration = Duration::days(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    /// Orbit's one comment on a pull request.
    #[default]
    Comment,
    /// Orbit's issue about a default branch.
    Issue,
}

/// Where Orbit tells an agent about one thing: its comment on a pull
/// request, or its issue about a default branch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thread {
    /// `pr:owner/name#12` for a pull request's comment, kept across its
    /// problems so item numbers are never reused; `issue:<problem id>` for
    /// a default branch's issue, one per problem.
    pub key: String,
    pub kind: ThreadKind,
    pub repo: String,
    /// The pull request, or the issue once it is open.
    pub number: Option<u64>,
    /// The pull request, for a comment.
    pub pr_id: Option<String>,
    /// Orbit's comment, once posted.
    pub comment_id: Option<u64>,
    /// Where it is on GitHub, once posted.
    pub url: String,
    /// The default branch, for an issue.
    pub branch: String,
    /// The problem it is about now.
    pub problem_id: String,
    pub items: Vec<Item>,
    /// The text GitHub has, as Orbit last wrote it.
    pub posted: String,
    /// Whether Orbit closed the issue.
    pub closed: bool,
    /// When the problem closed that the issue's closing note is about.
    pub noted: Option<String>,
    /// You said to post it, while Bit asks first.
    pub allowed: bool,
    /// Bit told you it can post it, for a proposal it asks about first.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub offered: bool,
    /// The comment or issue is gone from GitHub, so Orbit leaves it be.
    pub gone: bool,
    /// A try to post it may have reached GitHub without Orbit hearing
    /// back, so Orbit looks for it before anything else.
    pub unsure: bool,
    /// Why the latest write failed, how many in a row have, and when Orbit
    /// tries again.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    pub updated_at: String,
    /// The newest reply Orbit has read on its comment or issue, by GitHub
    /// comment ID, so each reply is read once.
    pub read_to: u64,
    /// Orbit reads the comments posted or edited from this time on, by
    /// GitHub's clock. GitHub lists them oldest first, by when they were
    /// posted, so it moves on only once Orbit has read them all.
    pub read_since: String,
    /// The page of that list Orbit goes on from, part way through it; 0
    /// once it has read it all.
    pub read_page: u32,
    /// The latest time a comment on that list was posted or edited, where
    /// the next list starts.
    pub read_newest: String,
    /// When GitHub last took Orbit's text for its comment or issue, by
    /// GitHub's clock.
    pub shown_at: String,
    /// Whether the pull request is clear to merge, as its comment says it,
    /// for one Orbit reviews automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready: Option<Ready>,
    /// The kind of security alert its issue is about, for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}
impl Thread {
    /// Whether it is on GitHub.
    pub fn is_posted(&self) -> bool {
        match self.kind {
            ThreadKind::Comment => self.comment_id.is_some(),
            ThreadKind::Issue => self.number.is_some(),
        }
    }
    /// The hidden line that marks Orbit's comment or issue for this
    /// thread, followed by its items as JSON.
    pub fn tag(&self) -> String {
        tag(&self.key)
    }
}
/// Whether a pull request is clear to merge, as the top of its comment says
/// it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ready {
    pub clear: bool,
    /// The head and base it was worked out for, and once it is clear, the
    /// base's latest commit.
    pub head_sha: String,
    pub base: String,
    pub base_sha: String,
    /// Whose move it is while it isn't clear; none once Orbit stopped
    /// working it out.
    pub turn: Option<Turn>,
    /// What is in the way, in Orbit's words, or once it is clear, what
    /// Orbit couldn't check.
    pub text: String,
    /// When Orbit first gave this answer, words aside, by Orbit's clock.
    /// A wait on the agent starts again once the comment is first on
    /// GitHub, when the agent can see it (`first_shown`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub since: String,
    /// When Orbit nudged whoever's move it is, once, as it had waited
    /// long enough (`nudges`): the agent on the comment, or you in Needs
    /// you.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nudged: Option<String>,
    /// When GitHub took the comment reminding the agent, which may be
    /// later than `nudged`, or never once it is gone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminded: Option<String>,
}
impl Ready {
    fn of(r: &Readiness) -> Ready {
        let first = r.waits.first();
        Ready {
            clear: r.clear,
            head_sha: r.head_sha.clone(),
            base: r.base.clone(),
            base_sha: if r.clear {
                r.base_sha.clone()
            } else {
                String::new()
            },
            turn: first.map(|w| w.turn),
            text: first
                .map(|w| w.text.clone())
                .or(r.note())
                .unwrap_or_default(),
            ..Default::default()
        }
    }
    /// What a pull request Orbit no longer works it out for says.
    fn stopped(pr: &PullRequest) -> Ready {
        let text = if pr.draft {
            "Orbit stopped working out whether it is clear to merge while it is a draft."
        } else {
            "Orbit stopped working out whether it is clear to merge, as it no longer reviews it automatically."
        };
        Ready {
            head_sha: pr.head_sha.clone(),
            base: pr.base_ref.clone(),
            text: text.into(),
            ..Default::default()
        }
    }
    /// Whether it gives the same answer, whatever its words and however
    /// long it has stood.
    fn same(&self, other: &Ready) -> bool {
        let answer = |r: &Ready| Ready {
            text: String::new(),
            since: String::new(),
            nudged: None,
            reminded: None,
            ..r.clone()
        };
        answer(self) == answer(other)
    }
    /// Whether its comment reminds the agent: it waits on them, and Orbit
    /// nudged them.
    fn reminds(&self) -> bool {
        self.turn == Some(Turn::Author) && self.nudged.is_some()
    }
    /// Whether it is worth a comment of its own: it is clear, or it waits
    /// on someone.
    fn tells(&self) -> bool {
        self.clear || matches!(self.turn, Some(Turn::Author | Turn::Person))
    }
}

/// The start of the hidden line that marks a thread's comment or issue.
pub fn tag(key: &str) -> String {
    format!("<!-- orbit-ops thread={key} ")
}
/// The hidden line that marks an issue's closing note.
pub fn note_tag(key: &str, closed_at: &str) -> String {
    format!("<!-- orbit-ops-note thread={key} closed={closed_at} -->")
}

/// What an item is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A check that fails.
    #[default]
    Check,
    /// A pull request's base branch moved on since its checks began (see
    /// `ProblemKind::BaseMoved`).
    BaseMoved,
    /// No CI runs on the repository's pull requests (see
    /// `ProblemKind::NoCi`).
    NoCi,
    /// A security alert, open while GitHub has it open (see
    /// `ProblemKind::Security`).
    Alert,
    /// A risky workflow setting, open while Orbit finds it in the files on
    /// the default branch (see `ProblemKind::WorkflowRisks`).
    Risk,
}
impl ItemKind {
    fn is_check(&self) -> bool {
        *self == ItemKind::Check
    }
}
/// One failing check, as a comment or issue lists it, or a pull request's
/// base moving on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    /// 1 for I1. Never reused within its thread.
    pub number: u32,
    #[serde(skip_serializing_if = "ItemKind::is_check")]
    pub kind: ItemKind,
    /// The check, such as "CI / Rust tests", or for `BaseMoved`, the base
    /// branch, and for `NoCi`, the default branch.
    pub check: String,
    pub url: String,
    /// Whether it failed at `at`, the latest commit Orbit saw; for
    /// `BaseMoved`, whether the base has moved on since the checks there
    /// began.
    pub failing: bool,
    /// The commit it started failing at, this time.
    pub since: String,
    pub at: String,
    /// What an agent last said about it since it started failing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// When GitHub first showed it failing this time, by GitHub's clock:
    /// only replies from then on are about this failure. Empty until
    /// Orbit's text that shows it is on GitHub.
    pub heard_from: String,
    /// Why its job failed at `at`, once Orbit has read its log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<crate::logs::Failure>,
    /// Orbit's reruns of its job at `at`, and its tries at them (see
    /// `crate::reruns`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reruns: Vec<crate::reruns::Rerun>,
    /// The job you said to rerun, by its link, while Bit asks first.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rerun_ok: String,
    /// For a security alert: its number on GitHub.
    #[serde(skip_serializing_if = "is_zero")]
    pub alert: u64,
    /// For a risky workflow setting: its number in its problem.
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub risk: u32,
}
fn is_zero(n: &u64) -> bool {
    *n == 0
}
fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

impl Item {
    /// What it is about, in a few words: the check, "main moved on", or
    /// the alert, such as "Dependabot alert 12, lodash".
    pub fn name(&self) -> String {
        match self.kind {
            ItemKind::Check | ItemKind::Alert | ItemKind::Risk => self.check.clone(),
            ItemKind::BaseMoved => format!("{} moved on", self.check),
            ItemKind::NoCi => "CI on pull requests".into(),
        }
    }
}

/// What an agent says about an item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerKind {
    /// `Taking I2`: someone is on it.
    #[default]
    Taking,
    /// `Fixed I2 in abc1234`, or just `Fixed I2`.
    Fixed,
    /// `Declined I2: reason`.
    Declined,
    /// `Need a person on I2: question`.
    Question,
}
/// One line of a reply about items, as `engine::claims::ci_lines` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub numbers: Vec<u32>,
    pub kind: AnswerKind,
    pub commit: Option<String>,
    /// The reason or question.
    pub text: Option<String>,
}
/// A reply on Orbit's comment or issue. `said` is empty for one from an
/// account Orbit doesn't trust, or one Orbit wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub id: u64,
    pub author: String,
    /// When it was posted and last edited, by GitHub's clock.
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
    pub said: Vec<Said>,
}
/// What CI says at the commit an agent says fixes an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixCheck {
    /// Not known yet: its checks are still running, or the commit is newer
    /// than the one Orbit last read.
    Waiting,
    Passes,
    Fails,
    /// The commit isn't part of the branch Orbit reads.
    NotOnBranch,
    /// GitHub doesn't know the commit.
    NotFound,
}
/// The latest answer about an item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Answer {
    pub kind: AnswerKind,
    pub author: String,
    /// When the reply was posted, by GitHub's clock.
    pub at: String,
    /// The reply on GitHub.
    pub url: String,
    pub reply_id: u64,
    pub commit: Option<String>,
    /// The reason or question, in the agent's words. Orbit shows it, but
    /// never posts it.
    pub text: Option<String>,
    /// For a fix in a named commit, what CI says there.
    pub check: Option<FixCheck>,
    /// The commit the branch was at when Orbit last checked.
    pub head: String,
    /// When Orbit last tried to check it, so the fixes it checks take
    /// turns.
    pub checked_at: String,
}
/// An answer Orbit just heard or checked, for Bit's notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    pub thread: String,
    pub number: u32,
    /// What the item is about (`Item::name`).
    pub check: String,
    pub answer: Answer,
}
/// A fix to check against CI: an item an agent says is fixed in `commit`,
/// still failing at `head`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verify {
    pub thread: String,
    pub number: u32,
    pub repo: String,
    pub commit: String,
    pub head: String,
    pub check: String,
}

/// What a write does on GitHub.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Looks on GitHub for what an earlier try may have posted. Only reads,
    /// so it isn't in the journal.
    Find,
    PostComment,
    EditComment,
    OpenIssue,
    EditIssue,
    /// The note that says why an issue closes.
    NoteOnIssue,
    CloseIssue,
    ReopenIssue,
    /// Reruns a failed GitHub Actions job (see `crate::reruns`).
    RerunJob,
    /// Turns on a repository's security setting, on your tap (see
    /// `crate::baseline`). It belongs to no thread.
    ChangeSetting,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteState {
    /// Recorded, and on its way to GitHub.
    #[default]
    Sending,
    Done,
    Failed,
}
/// One write Orbit makes on GitHub, recorded before it is sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Write {
    pub id: String,
    pub thread: String,
    pub action: Action,
    pub repo: String,
    /// The pull request or issue, when known.
    #[serde(default)]
    pub number: Option<u64>,
    #[serde(default)]
    pub state: WriteState,
    pub at: String,
    #[serde(default)]
    pub done_at: Option<String>,
    /// Where GitHub shows it, once done.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub error: Option<String>,
    /// For a change to a setting, which one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting: Option<crate::baseline::Setting>,
}

/// A write a thread needs, ready to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub thread: String,
    pub action: Action,
    pub repo: String,
    /// The pull request for a comment, the issue for an issue.
    pub number: Option<u64>,
    pub comment_id: Option<u64>,
    /// For a new issue.
    pub title: String,
    /// The comment's or issue's text, or the closing note.
    pub body: String,
    /// What GitHub's copy contains, to find it again: the thread's tag, or
    /// the note's.
    pub marker: String,
    /// For closing: whether it closes as done rather than not planned.
    pub completed: bool,
    /// For a closing note: when the problem it is about closed.
    pub closed_at: Option<String>,
    /// For a rerun: the item's job.
    pub rerun: Option<crate::reruns::Job>,
}
/// What GitHub said to a write: where the comment or issue is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Posted {
    pub id: u64,
    pub number: Option<u64>,
    pub url: String,
    /// When GitHub took this write, or for a copy Orbit read back, when it
    /// last took Orbit's text in it, by GitHub's clock.
    pub updated_at: String,
    /// The text GitHub has.
    pub body: String,
}

/// Whether a rule lets Bit post: it is on, and the rule is Do it, or Ask
/// first and you said yes.
pub fn rule_allows(rule: &str, allowed: bool, bit_on: bool) -> bool {
    bit_on && (rule == "do_it" || (rule == "ask_first" && allowed))
}
/// Whether Bit may post something new for a thread now: its rule allows it
/// and Orbit is `watching`. Checked again right before each post.
pub fn may_post(s: &Snapshot, t: &Thread) -> bool {
    let buddy = &s.settings.buddy;
    rule_allows(rule_of(s, t), t.allowed, buddy.enabled) && watching(s, t)
}
/// The rule for posting a thread: proposing CI for a repository with none
/// on pull requests, opening an issue about a default branch, or
/// commenting on a pull request.
pub fn rule_of<'a>(s: &'a Snapshot, t: &Thread) -> &'a str {
    let rules = &s.settings.buddy.rules;
    match t.kind {
        ThreadKind::Comment => &rules.comment_on_prs,
        ThreadKind::Issue if proposes(s, t) => &rules.propose_ci,
        ThreadKind::Issue => &rules.open_issues,
    }
}
/// Whether a thread is Orbit's proposal of CI for a repository.
pub fn proposes(s: &Snapshot, t: &Thread) -> bool {
    s.problems
        .iter()
        .any(|p| p.id == t.problem_id && p.kind == ProblemKind::NoCi)
}
/// Marks each open proposal that Bit asks about first, and hasn't posted,
/// as offered, and returns what Bit tells you about each, once.
pub fn offer(s: &mut Snapshot) -> Vec<Event> {
    let keys: Vec<String> = s
        .ops
        .iter()
        .filter(|t| !t.offered && !t.is_posted() && !t.unsure && !t.allowed)
        .filter(|t| {
            let open = s
                .problems
                .iter()
                .any(|p| p.id == t.problem_id && p.is_open());
            open && proposes(s, t)
                && s.settings.buddy.enabled
                && rule_of(s, t) == "ask_first"
                && watching(s, t)
        })
        .map(|t| t.key.clone())
        .collect();
    for t in s.ops.iter_mut().filter(|t| keys.contains(&t.key)) {
        t.offered = true;
    }
    keys.into_iter().map(Event::Offered).collect()
}
/// Whether Orbit watches what a thread is about: Watch CI is on (Watch
/// security alerts, for alerts), its repository is connected, and for a
/// comment, Orbit still follows the pull request.
pub fn watching(s: &Snapshot, t: &Thread) -> bool {
    let switch = match t.source {
        Some(_) => s.settings.watch_security,
        None => s.settings.watch_ci,
    };
    let watched = switch
        && s.settings
            .repositories
            .iter()
            .any(|r| same_repo(&r.name, &t.repo));
    let followed = match t.kind {
        ThreadKind::Issue => true,
        ThreadKind::Comment => s.prs.iter().any(|p| {
            t.pr_id.as_deref() == Some(p.id.as_str())
                && !p.draft
                && trusted_source(p, &s.settings.trusted_authors)
        }),
    };
    watched && followed
}

/// Brings each thread up to date with its problem, and starts one for each
/// open problem without one: an issue's thread for a default branch's
/// problem or a kind of security alert, and the pull request's thread for a
/// pull request's. Runs waiting for a runner get none, as only a person can
/// add or fix one, and security alerts only when `tells_agent`, though a
/// thread already on GitHub is kept true. Returns whether anything changed.
pub fn refresh(threads: &mut Vec<Thread>, problems: &[Problem], now: DateTime<Utc>) -> bool {
    let before = threads.clone();
    let mut order: Vec<&Problem> = problems
        .iter()
        .filter(|p| match p.kind {
            ProblemKind::Waiting => false,
            ProblemKind::Security | ProblemKind::WorkflowRisks => {
                tells_agent(p) || threads.iter().any(|t| t.key == key_of(p))
            }
            _ => true,
        })
        .collect();
    // Older first, so a pull request's newest problem is the one it keeps.
    order.sort_by(|a, b| a.opened_at.cmp(&b.opened_at));
    for p in order {
        let key = key_of(p);
        let i = match threads.iter().position(|t| t.key == key) {
            Some(i) => i,
            None if p.is_open() => {
                threads.push(Thread {
                    key,
                    kind: match p.kind {
                        ProblemKind::DefaultBranch
                        | ProblemKind::NoCi
                        | ProblemKind::Waiting
                        | ProblemKind::Security
                        | ProblemKind::WorkflowRisks => ThreadKind::Issue,
                        ProblemKind::PullRequest | ProblemKind::BaseMoved => ThreadKind::Comment,
                    },
                    repo: p.repo.clone(),
                    number: match p.kind {
                        ProblemKind::DefaultBranch
                        | ProblemKind::NoCi
                        | ProblemKind::Waiting
                        | ProblemKind::Security
                        | ProblemKind::WorkflowRisks => None,
                        ProblemKind::PullRequest | ProblemKind::BaseMoved => {
                            p.pr_id.as_deref().and_then(pr_number)
                        }
                    },
                    pr_id: p.pr_id.clone(),
                    branch: p.branch.clone(),
                    problem_id: p.id.clone(),
                    source: p.source,
                    ..Default::default()
                });
                threads.len() - 1
            }
            None => continue,
        };
        let t = &mut threads[i];
        // A pull request's thread follows its open problem, or the one it
        // last followed.
        if t.problem_id != p.id && !p.is_open() {
            continue;
        }
        t.problem_id = p.id.clone();
        t.branch = p.branch.clone();
        items_from(t, p);
        // A failure that came back without changing Orbit's text has been
        // on GitHub since Orbit last wrote it.
        let unshown = t.items.iter().any(|i| i.failing && i.heard_from.is_empty());
        if unshown && t.is_posted() && !t.shown_at.is_empty() && body_now(t, p) == t.posted {
            let at = t.shown_at.clone();
            shown(t, &at);
        }
    }
    let at = stamp(now);
    let mut changed = false;
    for (i, t) in threads.iter_mut().enumerate() {
        if before.get(i) != Some(t) {
            t.updated_at = at.clone();
            changed = true;
        }
    }
    changed || threads.len() != before.len()
}
/// Puts each pull request's answer to whether it is clear to merge on its
/// comment's thread, starting a thread for each one while Orbit watches
/// CI. The answer a comment gives changes only when it turns clear or not,
/// its head or base moves, or it becomes someone else's move, so each
/// change wakes the agent once; its words catch up whenever the comment
/// changes anyway. An answer that has stood long enough nudges whoever's
/// move it is, once (`nudges`). A pull request still open with no answer
/// any more says Orbit stopped; a closed one is left as it was. Returns
/// whether anything changed.
pub fn ready(
    threads: &mut Vec<Thread>,
    answers: &[Readiness],
    prs: &[PullRequest],
    problems: &[Problem],
    ci: &[RepoCi],
    watch: bool,
    now: DateTime<Utc>,
) -> bool {
    let before = threads.clone();
    for a in answers.iter().filter(|_| watch) {
        let key = format!("pr:{}", a.pr_id);
        if !threads.iter().any(|t| t.key == key) {
            threads.push(Thread {
                key,
                repo: a.repo.clone(),
                number: Some(a.number),
                pr_id: Some(a.pr_id.clone()),
                ..Default::default()
            });
        }
    }
    let at = stamp(now);
    for t in threads.iter_mut() {
        let pr = prs
            .iter()
            .find(|p| t.pr_id.as_deref() == Some(p.id.as_str()));
        let Some(pr) = pr else {
            continue;
        };
        let a = answers.iter().find(|a| a.pr_id == pr.id);
        let mut answer = match a {
            Some(a) => Ready::of(a),
            None if t.ready.is_some() => Ready::stopped(pr),
            None => continue,
        };
        let Some(kept) = t.ready.as_ref().filter(|r| r.same(&answer)) else {
            answer.since = at.clone();
            t.ready = Some(answer);
            continue;
        };
        // An answer from before Orbit kept its time starts now.
        answer.since = match kept.since.as_str() {
            "" => at.clone(),
            since => since.into(),
        };
        answer.nudged = kept.nudged.clone();
        answer.reminded = kept.reminded.clone();
        let due = a.is_some_and(|a| nudges(t, a, &answer.since, now));
        if answer.nudged.is_none() && watch && due {
            answer.nudged = Some(at.clone());
        }
        if let Some(r) = t.ready.as_mut() {
            r.since = answer.since.clone();
            r.nudged = answer.nudged.clone();
        }
        // The same answer in new words waits for the comment to change,
        // which a nudge to the agent does.
        if comment_body(t, problem_of(t, problems, ci)) != t.posted {
            t.ready = Some(answer);
        }
    }
    let mut changed = false;
    for (i, t) in threads.iter_mut().enumerate() {
        if before.get(i) != Some(t) {
            t.updated_at = at.clone();
            changed = true;
        }
    }
    changed
}
/// Whether an answer that hasn't changed has waited long enough to nudge
/// whoever's move it is: the agent, `NUDGE_AGENT` after its comment shows
/// it, unless it answered each failing check or base item it is about
/// (`Declined` or `Need a person`); or a person, `NUDGE_PERSON` after a
/// reviewer or a rule on GitHub started holding it back. Orbit's loop, its
/// checks while Watch CI is off and a base it can't judge are in Needs you
/// or on screen already.
fn nudges(t: &Thread, a: &Readiness, since: &str, now: DateTime<Utc>) -> bool {
    use crate::readiness::WaitKind;
    let Some(w) = a.waits.first() else {
        return false;
    };
    let after = match (w.turn, w.kind) {
        (Turn::Author, kind) => {
            let answered = matches!(kind, WaitKind::Checks | WaitKind::Base) && answered(t);
            if !t.is_posted() || t.gone || answered {
                return false;
            }
            NUDGE_AGENT
        }
        (Turn::Person, WaitKind::Merge | WaitKind::Reviews) => NUDGE_PERSON,
        _ => return false,
    };
    time(since).is_some_and(|since| now - since >= after)
}
/// Whether the agent answered every item failing on its comment in a way
/// that leaves the next step to a person: it declined it, or asked for a
/// person.
fn answered(t: &Thread) -> bool {
    let mut failing = t.items.iter().filter(|i| i.failing).peekable();
    failing.peek().is_some()
        && failing.all(|i| {
            i.answer
                .as_ref()
                .is_some_and(|a| matches!(a.kind, AnswerKind::Declined | AnswerKind::Question))
        })
}
/// Whether a problem's agent hears of it: always but for security alerts
/// and risky workflow settings (`Problem::sensitive`), which it hears of
/// only in a private repository, as an issue in a public one would show
/// everyone where the weakness is before it is fixed, and never for a
/// secret, as only a person can revoke one.
pub fn tells_agent(p: &Problem) -> bool {
    match p.kind {
        ProblemKind::Waiting => false,
        ProblemKind::Security => p.private && p.source != Some(Source::SecretScanning),
        ProblemKind::WorkflowRisks => p.private,
        _ => true,
    }
}
/// Whether a thread about security alerts or risky workflow settings
/// takes nothing now: GitHub last said its repository isn't private, so
/// anything Orbit writes there could show more people where a weakness is
/// than when Orbit told the agent, even closing, whose text may hold one
/// GitHub never had from Orbit. Its issue stays as it is on GitHub until
/// GitHub says the repository is private again. Each write is also held
/// until GitHub says so just before it (`Engine::tell_agents`).
pub fn withheld(t: &Thread, p: &Problem) -> bool {
    (t.source.is_some() || p.sensitive()) && !tells_agent(p)
}
/// Whether a thread is about something only the repository's own people
/// may hear of: security alerts or risky workflow settings.
pub fn sensitive(s: &Snapshot, key: &str) -> bool {
    s.ops.iter().any(|t| {
        t.key == key
            && (t.source.is_some()
                || s.problems
                    .iter()
                    .any(|p| p.id == t.problem_id && p.sensitive()))
    })
}
/// Whether only a person may leave an item open when its agent declines
/// it: a high or critical security alert, or a high risky workflow
/// setting.
pub fn yours_to_decide(i: &Item, p: &Problem) -> bool {
    match i.kind {
        ItemKind::Alert => p
            .security_alerts
            .iter()
            .any(|a| a.number == i.alert && a.needs_you()),
        ItemKind::Risk => p
            .risks
            .iter()
            .any(|r| r.number == i.risk && r.severity >= crate::security::Severity::High),
        _ => false,
    }
}
fn key_of(p: &Problem) -> String {
    match p.kind {
        ProblemKind::DefaultBranch
        | ProblemKind::NoCi
        | ProblemKind::Waiting
        | ProblemKind::Security
        | ProblemKind::WorkflowRisks => format!("issue:{}", p.id),
        // A pull request's problems share its one comment.
        ProblemKind::PullRequest | ProblemKind::BaseMoved => {
            format!("pr:{}", p.pr_id.as_deref().unwrap_or(&p.repo))
        }
    }
}
fn pr_number(id: &str) -> Option<u64> {
    id.rsplit_once('#').and_then(|(_, n)| n.parse().ok())
}
/// Marks each item failing or not as the problem stands now, adding one for
/// each check that fails for the first time, for a pull request's base
/// moving on the first time, or for a repository with no CI on pull
/// requests.
fn items_from(t: &mut Thread, p: &Problem) {
    if p.kind == ProblemKind::Security {
        return alert_items(t, p);
    }
    if p.kind == ProblemKind::WorkflowRisks {
        return risk_items(t, p);
    }
    // Only a passing commit says a check stopped failing; when Orbit stops
    // watching for another reason, items stay as it last saw them.
    if !p.is_open() {
        if p.passed() {
            for item in &mut t.items {
                item.failing = false;
                at(item, &p.closed_sha);
            }
        }
        return;
    }
    let moved = p.kind == ProblemKind::BaseMoved;
    let no_ci = p.kind == ProblemKind::NoCi;
    for item in &mut t.items {
        let failing = match item.kind {
            ItemKind::Check => p.failing.iter().any(|c| c.name == item.check),
            ItemKind::BaseMoved => moved,
            ItemKind::NoCi => no_ci,
            // Only a security problem has alerts (`alert_items`), and only
            // a workflows problem risks (`risk_items`).
            ItemKind::Alert | ItemKind::Risk => false,
        };
        // Its branch follows a rename.
        if failing && item.kind == ItemKind::BaseMoved {
            item.check = p.base.clone();
        }
        if failing && item.kind == ItemKind::NoCi {
            item.check = p.branch.clone();
        }
        if failing && !item.failing {
            // Failing again: what was said about it before is over.
            item.since = p.sha.clone();
            item.answer = None;
            item.heard_from.clear();
        }
        item.failing = failing;
        at(item, &p.sha);
    }
    let new = |t: &Thread, kind: ItemKind, check: &str, url: &str| Item {
        number: t.items.iter().map(|i| i.number).max().unwrap_or(0) + 1,
        kind,
        check: check.to_string(),
        url: url.to_string(),
        failing: true,
        since: p.sha.clone(),
        at: p.sha.clone(),
        ..Default::default()
    };
    for check in &p.failing {
        let found = t
            .items
            .iter_mut()
            .find(|i| i.kind == ItemKind::Check && i.check == check.name);
        match found {
            Some(item) => item.url = check.url.clone(),
            None => {
                let item = new(t, ItemKind::Check, &check.name, &check.url);
                t.items.push(item);
            }
        }
    }
    if moved && !t.items.iter().any(|i| i.kind == ItemKind::BaseMoved) {
        let item = new(t, ItemKind::BaseMoved, &p.base, "");
        t.items.push(item);
    }
    if no_ci && !t.items.iter().any(|i| i.kind == ItemKind::NoCi) {
        let item = new(t, ItemKind::NoCi, &p.branch, "");
        t.items.push(item);
    }
}
/// Marks each alert's item open while its problem lists it, adding one for
/// each new alert, the most severe first. An alert leaves its problem only
/// once GitHub says it is no longer open, so its item stops being open
/// then; when Orbit stopped watching instead, items stay as it last saw
/// them.
fn alert_items(t: &mut Thread, p: &Problem) {
    if (!p.is_open() && !p.cleared()) || withheld(t, p) {
        return;
    }
    for item in t.items.iter_mut().filter(|i| i.kind == ItemKind::Alert) {
        let open = p.security_alerts.iter().find(|a| a.number == item.alert);
        if let Some(a) = open {
            if !item.failing {
                // Open again: what was said about it before is over.
                item.answer = None;
                item.heard_from.clear();
            }
            item.check = a.label();
            item.url = a.url.clone();
            item.since = a.created_at.clone();
            item.at = a.created_at.clone();
        }
        item.failing = open.is_some();
    }
    for a in &p.security_alerts {
        let known = t
            .items
            .iter()
            .any(|i| i.kind == ItemKind::Alert && i.alert == a.number);
        if !known {
            let number = t.items.iter().map(|i| i.number).max().unwrap_or(0) + 1;
            t.items.push(Item {
                number,
                kind: ItemKind::Alert,
                alert: a.number,
                check: a.label(),
                url: a.url.clone(),
                failing: true,
                since: a.created_at.clone(),
                at: a.created_at.clone(),
                ..Default::default()
            });
        }
    }
}
/// Marks each risky setting's item open while its problem lists it, at the
/// commit Orbit last read, adding one for each new one, the most severe
/// first. A risk that comes back after it went has a new number, so a new
/// item. When Orbit stopped watching instead of finding them fixed, items
/// stay as it last saw them.
fn risk_items(t: &mut Thread, p: &Problem) {
    if (!p.is_open() && !p.passed()) || withheld(t, p) {
        return;
    }
    let sha = if p.is_open() { &p.sha } else { &p.closed_sha };
    for item in t.items.iter_mut().filter(|i| i.kind == ItemKind::Risk) {
        let open = p.risks.iter().find(|r| r.number == item.risk);
        if let Some(r) = open {
            item.check = r.name();
            item.since = r.since.clone();
        }
        item.failing = open.is_some();
        item.at = sha.clone();
    }
    for r in &p.risks {
        let known = t
            .items
            .iter()
            .any(|i| i.kind == ItemKind::Risk && i.risk == r.number);
        if !known {
            let number = t.items.iter().map(|i| i.number).max().unwrap_or(0) + 1;
            t.items.push(Item {
                number,
                kind: ItemKind::Risk,
                risk: r.number,
                check: r.name(),
                failing: true,
                since: r.since.clone(),
                at: sha.clone(),
                ..Default::default()
            });
        }
    }
}
/// Moves an item to a commit. Reruns are of one commit's jobs, so they
/// start again at a new one.
fn at(item: &mut Item, sha: &str) {
    if item.at != sha {
        item.reruns.clear();
        item.rerun_ok.clear();
        item.at = sha.to_string();
    }
}

/// Gives each failing item what Orbit read about why its job failed.
/// Returns whether any changed.
pub fn explain(threads: &mut [Thread], failures: &[crate::logs::Failure]) -> bool {
    let mut changed = false;
    for item in threads.iter_mut().flat_map(|t| t.items.iter_mut()) {
        let why = failures
            .iter()
            .find(|f| item.failing && !item.url.is_empty() && f.url == item.url)
            .cloned();
        if item.why != why {
            item.why = why;
            changed = true;
        }
    }
    changed
}
/// Records that GitHub has Orbit's text for a thread from `at` on, by
/// GitHub's clock. Each failing item it shows for the first time this time
/// hears replies from then on, and as no reply from before the earliest of
/// those is about a failing item, Orbit reads on from there.
fn shown(t: &mut Thread, at: &str) {
    if at.is_empty() {
        return;
    }
    t.shown_at = at.to_string();
    let failing = t.items.iter_mut().filter(|i| i.failing);
    for item in failing.filter(|i| i.heard_from.is_empty()) {
        item.heard_from = at.to_string();
    }
    read_on(t);
}
/// Takes from Orbit's copy on GitHub of a comment or issue it found again,
/// or posted before it read answers, when each failing item was on GitHub:
/// one the copy shows failing since the same commit has been there since
/// GitHub last took that text. It may have been there longer, but the
/// copy can't say since when, so a reply from before then doesn't count:
/// it may be about an earlier failure of the same check.
pub fn recover(t: &mut Thread, copy: &Posted) {
    if copy.updated_at.is_empty() {
        return;
    }
    let on_github = failing_in(&copy.body, &t.key);
    let failing = t.items.iter_mut().filter(|i| i.failing);
    for item in failing.filter(|i| i.heard_from.is_empty()) {
        if on_github.contains(&(item.number, item.since.clone())) {
            item.heard_from = copy.updated_at.clone();
        }
    }
    if t.shown_at.is_empty() {
        t.shown_at = copy.updated_at.clone();
    }
    read_on(t);
}
/// The items a copy of Orbit's text shows failing, from its hidden line:
/// each one's number and the commit it started failing at.
fn failing_in(body: &str, key: &str) -> Vec<(u32, String)> {
    let Some((_, rest)) = body.split_once(&tag(key)) else {
        return vec![];
    };
    let json = rest.split_once(" -->").map_or(rest, |(json, _)| json);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec![];
    };
    let items = v["items"].as_array().into_iter().flatten();
    items
        .filter(|i| i["failing"] == true)
        .filter_map(|i| {
            let number = i["id"].as_str()?.strip_prefix('I')?.parse().ok()?;
            Some((number, i["since"].as_str()?.to_string()))
        })
        .collect()
}
/// Reads on from the earliest time GitHub first showed a failing item, as
/// no reply from before it is about one. One GitHub doesn't show yet hears
/// only replies from later still.
fn read_on(t: &mut Thread) {
    let floor = t
        .items
        .iter()
        .filter(|i| i.failing && !i.heard_from.is_empty())
        .map(|i| &i.heard_from)
        .min();
    if let Some(floor) = floor.filter(|f| **f > t.read_since) {
        t.read_since = floor.clone();
        t.read_page = 0;
        t.read_newest.clear();
    }
}
/// Reads comments Orbit asked GitHub for: each line about a failing item
/// in a reply Orbit hasn't read yet, posted since GitHub first showed the
/// item failing this time, becomes its answer, the latest counting. Other
/// items are left alone. `more` is the page to go on from next time, when
/// GitHub has more of the list. Returns the answers that changed.
pub fn hear(t: &mut Thread, replies: &[Reply], more: Option<u32>) -> Vec<Heard> {
    if let Some(newest) = replies.iter().map(|r| &r.updated_at).max() {
        if *newest > t.read_newest {
            t.read_newest = newest.clone();
        }
    }
    match more {
        Some(page) => t.read_page = page,
        // Read to the end, so every comment from before the latest one on
        // the list was on it. Moving on any sooner would skip those GitHub
        // lists after the ones read so far.
        None => {
            let newest = std::mem::take(&mut t.read_newest);
            if newest > t.read_since {
                t.read_since = newest;
            }
            t.read_page = 0;
        }
    }
    let mut replies: Vec<&Reply> = replies.iter().filter(|r| r.id > t.read_to).collect();
    replies.sort_by_key(|r| r.id);
    let mut heard: Vec<Heard> = vec![];
    for r in replies {
        t.read_to = r.id;
        for said in &r.said {
            for &number in &said.numbers {
                let about = |i: &&mut Item| {
                    i.number == number
                        && i.failing
                        && !i.heard_from.is_empty()
                        && r.created_at >= i.heard_from
                };
                let Some(item) = t.items.iter_mut().find(about) else {
                    continue;
                };
                let answer = Answer {
                    kind: said.kind,
                    author: r.author.clone(),
                    at: r.created_at.clone(),
                    url: r.url.clone(),
                    reply_id: r.id,
                    commit: said.commit.clone(),
                    text: said
                        .text
                        .as_ref()
                        .map(|t| t.chars().take(MAX_ANSWER).collect()),
                    check: said.commit.as_ref().map(|_| FixCheck::Waiting),
                    head: String::new(),
                    checked_at: String::new(),
                };
                item.answer = Some(answer.clone());
                heard.retain(|h| h.number != number);
                heard.push(Heard {
                    thread: t.key.clone(),
                    number,
                    check: item.name(),
                    answer,
                });
            }
        }
    }
    heard
}
/// The fixes to check against CI now: failing items an agent says a commit
/// fixes, on open problems, whose check hasn't settled, the one Orbit
/// tried longest ago first. One that wasn't on the branch is checked again
/// once the branch moves.
pub fn to_verify(threads: &[Thread], problems: &[Problem]) -> Vec<Verify> {
    let mut out = vec![];
    for t in threads.iter().filter(|t| !t.gone) {
        let Some(p) = problems
            .iter()
            .find(|p| p.id == t.problem_id && p.is_open() && !p.sha.is_empty())
        else {
            continue;
        };
        // A base that moved on settles itself once CI runs again.
        let checks = t.items.iter().filter(|i| i.kind == ItemKind::Check);
        for item in checks.filter(|i| i.failing) {
            let Some(a) = &item.answer else { continue };
            let (Some(commit), Some(check)) = (&a.commit, a.check) else {
                continue;
            };
            let again = match check {
                FixCheck::Waiting => true,
                FixCheck::NotOnBranch => a.head != p.sha,
                _ => false,
            };
            if again {
                out.push((
                    a.checked_at.clone(),
                    Verify {
                        thread: t.key.clone(),
                        number: item.number,
                        repo: t.repo.clone(),
                        commit: commit.clone(),
                        head: p.sha.clone(),
                        check: item.check.clone(),
                    },
                ));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().map(|(_, v)| v).collect()
}
/// Records a try to check a fix's commit and, when GitHub answered, what
/// CI says there, if the answer is still the one checked. Returns it when
/// it has just settled, for Bit's notes.
pub fn verified(
    threads: &mut [Thread],
    v: &Verify,
    result: Result<FixCheck, String>,
    now: DateTime<Utc>,
) -> Option<Heard> {
    let t = threads.iter_mut().find(|t| t.key == v.thread)?;
    let item = t.items.iter_mut().find(|i| i.number == v.number)?;
    let name = item.name();
    let a = item
        .answer
        .as_mut()
        .filter(|a| a.commit.as_deref() == Some(v.commit.as_str()))?;
    a.checked_at = stamp(now);
    let check = result.ok()?;
    let was = a.check;
    a.check = Some(check);
    a.head = v.head.clone();
    (was != Some(check) && check != FixCheck::Waiting).then(|| Heard {
        thread: t.key.clone(),
        number: v.number,
        check: name,
        answer: a.clone(),
    })
}

/// The next write a thread needs, if any: post it, keep it up to date, and
/// for an issue, close it with a note once its problem is over. Something
/// new is posted only while `may_post`; what is already on GitHub is kept
/// true whatever the rule, until it is gone.
pub fn next(
    t: &Thread,
    problem: Option<&Problem>,
    may_post: bool,
    now: DateTime<Utc>,
) -> Option<Planned> {
    if t.gone
        || t.retry_at
            .as_deref()
            .and_then(time)
            .is_some_and(|at| now < at)
    {
        return None;
    }
    if t.unsure && !t.is_posted() {
        return Some(Planned {
            thread: t.key.clone(),
            action: Action::Find,
            repo: t.repo.clone(),
            number: t.number,
            comment_id: None,
            title: String::new(),
            body: String::new(),
            marker: t.tag(),
            completed: false,
            closed_at: None,
            rerun: None,
        });
    }
    let plan = |action: Action, body: String| Planned {
        thread: t.key.clone(),
        action,
        repo: t.repo.clone(),
        number: t.number,
        comment_id: t.comment_id,
        title: String::new(),
        marker: t.tag(),
        completed: problem.is_some_and(|p| p.passed() || p.resolved()),
        closed_at: None,
        rerun: None,
        body,
    };
    match t.kind {
        ThreadKind::Comment => {
            let p = problem;
            if p.is_none() && t.ready.is_none() {
                return None;
            }
            if t.comment_id.is_none() {
                let tells =
                    p.is_some_and(Problem::is_open) || t.ready.as_ref().is_some_and(Ready::tells);
                return (tells && may_post).then(|| plan(Action::PostComment, comment_body(t, p)));
            }
            // A closed pull request has nobody left to tell.
            if p.and_then(|p| p.closed_reason.as_deref()) == Some(PR_CLOSED) {
                return None;
            }
            let body = comment_body(t, p);
            (body != t.posted).then(|| plan(Action::EditComment, body))
        }
        ThreadKind::Issue => {
            let p = problem?;
            // Nothing about security alerts where more people may read it
            // than when Orbit told the agent: no issue opens, reopens,
            // takes new details or closes.
            if withheld(t, p) {
                return None;
            }
            if t.number.is_none() {
                return (p.is_open() && may_post).then(|| Planned {
                    title: issue_title(t, p),
                    ..plan(Action::OpenIssue, issue_body(t, p, false))
                });
            }
            let closing = closes(p, now);
            if closing && !t.closed {
                let closed_at = p.closed_at.clone().unwrap_or_default();
                if t.noted.as_deref() != Some(closed_at.as_str()) {
                    return Some(Planned {
                        marker: note_tag(&t.key, &closed_at),
                        closed_at: Some(closed_at),
                        ..plan(Action::NoteOnIssue, closing_note(t, p))
                    });
                }
                return Some(plan(Action::CloseIssue, issue_body(t, p, true)));
            }
            if p.is_open() && t.closed {
                return Some(plan(Action::ReopenIssue, issue_body(t, p, false)));
            }
            let body = body_now(t, p);
            (body != t.posted).then(|| plan(Action::EditIssue, body))
        }
    }
}
/// Why `problems` closes a pull request's problem when it closes.
const PR_CLOSED: &str = "The pull request closed.";
/// The problem a thread's comment or issue is about: the one it follows,
/// unless that closed with its pull request and the pull request has been
/// reopened since, as what Orbit saw before then is over. Its comment then
/// says what is true now, not that it closed.
///
/// Reopened means the latest CI read of its repository, `ci`, lists it:
/// CI lists only open pull requests, and a read is kept only once applied,
/// after the one that found it closed. Times can't tell, as GitHub's for
/// the reopening may come before Orbit's for the closing.
pub fn problem_of<'a>(t: &Thread, problems: &'a [Problem], ci: &[RepoCi]) -> Option<&'a Problem> {
    let p = problems.iter().find(|p| p.id == t.problem_id)?;
    let listed = |n| {
        ci.iter()
            .any(|c| same_repo(&c.repo, &t.repo) && c.pull(n).is_some())
    };
    let reopened = p.closed_reason.as_deref() == Some(PR_CLOSED) && t.number.is_some_and(listed);
    (!reopened).then_some(p)
}
/// Whether a default branch's issue should be closed: its problem is over,
/// and if it passed, it has stayed green long enough not to come back as
/// the same problem.
fn closes(p: &Problem, now: DateTime<Utc>) -> bool {
    let Some(closed) = p.closed_at.as_deref().and_then(time) else {
        return false;
    };
    // A workflow file once changed doesn't flap like a failing check.
    !p.passed()
        || matches!(p.kind, ProblemKind::NoCi | ProblemKind::WorkflowRisks)
        || now - closed >= CLOSE_AFTER
}

/// A check's name as code, so nothing in it is read as Markdown or
/// mentions anyone.
fn code(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| match c {
            '`' => '\'',
            c if c.is_control() => ' ',
            c => c,
        })
        .take(200)
        .collect();
    // Nothing in it can start a hidden line.
    format!("`{}`", text.replace("<!--", "<! --"))
}
/// A link's address, only to GitHub and only when it is safe in Markdown.
fn safe(url: &str) -> Option<&str> {
    let safe = url.starts_with("https://github.com/")
        && !url
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '"'));
    safe.then_some(url)
}
/// A link to a check's page.
fn details(url: &str) -> String {
    safe(url).map_or_else(String::new, |url| format!(" · [details]({url})"))
}
/// Orbit's words with others' in them, such as a check's name or GitHub's
/// error, as they are: each word Markdown could read as more than text, or
/// that could mention someone or link somewhere, is code.
fn plain(text: &str) -> String {
    let words: Vec<String> = text
        .split(' ')
        .map(|w| {
            let plain = w
                .chars()
                .all(|c| c.is_alphanumeric() || ".,;:-'()+/=?\"%".contains(c))
                && !w.contains("://")
                && !w.to_ascii_lowercase().starts_with("www.");
            if plain {
                w.to_string()
            } else {
                code(w)
            }
        })
        .collect();
    words.join(" ")
}
/// What an agent said about a failing item, and for a fix, what Orbit
/// found. A reason or question is linked, never repeated.
fn answer_text(item: &Item, t: &Thread) -> String {
    let Some(a) = &item.answer else {
        return String::new();
    };
    let reply = |what: String| match safe(&a.url) {
        Some(url) => format!(" · {what} ([reply]({url}))"),
        None => format!(" · {what}"),
    };
    let what = match (a.kind, &a.commit) {
        // Orbit doesn't check a commit for a proposal: it closes once a
        // workflow on the default branch runs on pull requests.
        (AnswerKind::Fixed, commit) if item.kind == ItemKind::NoCi => {
            let at = commit.as_ref().map(|c| format!(" in {}", code(short(c))));
            format!(
                "said to be added{}; Orbit is waiting to see a workflow that runs on pull requests on {}",
                at.unwrap_or_default(),
                branch(t)
            )
        }
        // A risk is fixed once Orbit no longer finds it on the default
        // branch.
        (AnswerKind::Fixed, commit) if item.kind == ItemKind::Risk => {
            let at = commit.as_ref().map(|c| format!(" in {}", code(short(c))));
            format!(
                "said to be fixed{}; Orbit checks again at the next commit on {}",
                at.unwrap_or_default(),
                branch(t)
            )
        }
        // An alert is fixed once GitHub says so, whatever the commit.
        (AnswerKind::Fixed, commit) if item.kind == ItemKind::Alert => {
            let at = commit.as_ref().map(|c| format!(" in {}", code(short(c))));
            format!(
                "said to be fixed{}; Orbit is waiting for GitHub to mark the alert fixed",
                at.unwrap_or_default()
            )
        }
        (AnswerKind::Taking, _) => "someone is on it".to_string(),
        (AnswerKind::Declined, _) => "declined".to_string(),
        (AnswerKind::Question, _) => "waiting for a person".to_string(),
        (AnswerKind::Fixed, None) => {
            "said to be fixed; Orbit is waiting to see it pass".to_string()
        }
        (AnswerKind::Fixed, Some(commit)) => {
            let c = code(short(commit));
            match a.check.unwrap_or(FixCheck::Waiting) {
                FixCheck::Waiting => {
                    format!("said to be fixed in {c}; Orbit is waiting for its checks there")
                }
                FixCheck::Passes => format!("said to be fixed in {c}, and it passes there"),
                FixCheck::Fails => format!("said to be fixed in {c}, but it still fails there"),
                FixCheck::NotOnBranch => {
                    format!("said to be fixed in {c}, which isn't on {}", branch(t))
                }
                FixCheck::NotFound => format!("said to be fixed in {c}, which Orbit can't find"),
            }
        }
    };
    reply(what)
}
fn item_line(item: &Item, t: &Thread, p: &Problem) -> String {
    match item.kind {
        ItemKind::BaseMoved => return base_line(item, t, p),
        ItemKind::NoCi => return setup_line(item, t, p),
        ItemKind::Alert => return alert_line(item, t, p),
        ItemKind::Risk => return risk_line(item, t, p),
        ItemKind::Check => {}
    }
    let state = if item.failing && !p.is_open() {
        format!(
            "failing at {} when Orbit stopped watching",
            code(short(&item.at))
        )
    } else if item.failing {
        if item.since == item.at {
            format!("failing at {}", code(short(&item.at)))
        } else {
            format!(
                "failing since {}, still at {}",
                code(short(&item.since)),
                code(short(&item.at))
            )
        }
    } else {
        let rerun = if crate::reruns::done(item) > 0 {
            " after a rerun"
        } else {
            ""
        };
        // A base that moved on is only ever about checks that pass.
        let passes = p.passed() || p.kind == ProblemKind::BaseMoved;
        let state = if passes { "passes" } else { "not failing" };
        format!("{state} at {}{rerun}", code(short(&item.at)))
    };
    let (link, answer) = if item.failing && p.is_open() {
        (details(&item.url), answer_text(item, t))
    } else if item.failing {
        (details(&item.url), String::new())
    } else {
        (String::new(), String::new())
    };
    format!(
        "- **I{}** · {} · {state}{link}{answer}",
        item.number,
        code(&item.check)
    )
}
/// The line about a pull request's base moving on since its checks began.
fn base_line(item: &Item, t: &Thread, p: &Problem) -> String {
    let base = code(&item.check);
    let ran = code(short(&item.at));
    let state = if item.failing && p.is_open() {
        format!(
            "{base} moved on since the checks at {ran} began · merge it in and let CI run again{}",
            answer_text(item, t)
        )
    } else if item.failing {
        format!("{base} moved on since the checks at {ran} began, when Orbit stopped watching")
    } else {
        format!("{base} moved on · done at {ran}")
    };
    format!("- **I{}** · {state}", item.number)
}
/// The line about adding CI that runs on pull requests.
fn setup_line(item: &Item, t: &Thread, p: &Problem) -> String {
    let state = if item.failing && p.is_open() {
        format!(
            "add a workflow that runs on pull requests{}",
            answer_text(item, t)
        )
    } else if item.failing {
        "add a workflow that runs on pull requests, when Orbit stopped watching".to_string()
    } else {
        format!(
            "a workflow runs on pull requests · done at {}",
            code(short(&item.at))
        )
    };
    format!("- **I{}** · {state}", item.number)
}
/// The line about a security alert: what and where it is, and what to
/// change, while it is open. What GitHub says about it, such as an
/// advisory's summary, is plain text.
fn alert_line(item: &Item, t: &Thread, p: &Problem) -> String {
    let open = p
        .security_alerts
        .iter()
        .find(|a| a.number == item.alert)
        .filter(|_| item.failing && p.is_open());
    let Some(a) = open else {
        let state = if item.failing {
            "open when Orbit stopped watching"
        } else if p.vanished.contains(&item.alert) {
            "GitHub no longer has it, so Orbit can't say it was fixed or dismissed"
        } else {
            "no longer open on GitHub"
        };
        return format!("- **I{}** · {} · {state}", item.number, plain(&item.check));
    };
    let mut parts = vec![format!("{} {}", a.source.one(), a.number)];
    let at = match (a.path.is_empty(), a.line) {
        (true, _) => String::new(),
        (false, 0) => format!(" in {}", code(&a.path)),
        (false, line) => format!(" in {} line {line}", code(&a.path)),
    };
    parts.push(format!("{} ({}){at}", code(&a.subject), a.severity.word()));
    match a.source {
        Source::Dependabot if a.fixed_in.is_empty() => parts.push(
            "no version fixes it yet: replace it, or ask a person whether to dismiss the alert"
                .into(),
        ),
        Source::Dependabot => parts.push(format!(
            "update {} to {} or later",
            code(&a.subject),
            code(&a.fixed_in)
        )),
        _ if !a.title.is_empty() => parts.push(plain(&a.title)),
        _ => {}
    }
    let link = safe(&a.url).map_or_else(String::new, |url| format!(" · [alert]({url})"));
    let answer = if p.is_open() {
        answer_text(item, t)
    } else {
        String::new()
    };
    format!(
        "- **I{}** · {}{link}{answer}",
        item.number,
        parts.join(" · ")
    )
}
/// The line about a risky workflow setting: what and where it is, and
/// what to change, while Orbit still finds it. The file's words are only
/// ever code.
fn risk_line(item: &Item, t: &Thread, p: &Problem) -> String {
    let open = p
        .risks
        .iter()
        .find(|r| r.number == item.risk)
        .filter(|_| item.failing && p.is_open());
    let Some(r) = open else {
        let state = if item.failing {
            "still there when Orbit stopped watching".to_string()
        } else {
            format!("fixed at {}", code(short(&item.at)))
        };
        return format!("- **I{}** · {} · {state}", item.number, plain(&item.check));
    };
    use crate::risks::RiskKind;
    let what = match r.kind {
        RiskKind::Unpinned => format!("{} isn't pinned", code(&r.subject)),
        RiskKind::Permissions if r.subject == "missing" => {
            "no `permissions`, so jobs get the token's default".to_string()
        }
        RiskKind::Permissions => format!("{} permissions", code(&r.subject)),
        RiskKind::PullCheckout => format!(
            "checks out the pull request's code ({}) where secrets are exposed",
            code(&r.subject)
        ),
        RiskKind::Injection => format!("{} goes straight into a script", code(&r.subject)),
        RiskKind::Actionlint | RiskKind::Zizmor => format!(
            "{} found: {} ({})",
            r.kind.key(),
            plain(&r.detail),
            code(&r.subject)
        ),
    };
    let job = if r.job.is_empty() {
        String::new()
    } else {
        format!(", job {}", code(&r.job))
    };
    let link = file_link(&t.repo, &p.sha, &r.path, r.line);
    format!(
        "- **I{}** · {what} ({}) · {} line {}{job}{link} · {}{}",
        item.number,
        r.severity.word(),
        code(&r.path),
        r.line,
        r.advice(),
        answer_text(item, t)
    )
}
/// A link to a line in a file at a commit, when the repository's name and
/// commit are safe in a link.
fn file_link(repo: &str, sha: &str, path: &str, line: usize) -> String {
    let safe_part = |s: &str, extra: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c) || extra.contains(c))
    };
    if !safe_part(repo, "/") || !safe_part(path, "/") || !sha.chars().all(|c| c.is_ascii_hexdigit())
    {
        return String::new();
    }
    format!(" · [file](https://github.com/{repo}/blob/{sha}/{path}#L{line})")
}
/// Why an item's job failed, under its line: the step, and the end of its
/// log in a code block that nothing in it can close. The log is someone
/// else's words, so it is only ever code, and anything that reads like
/// one of Orbit's hidden lines is broken up.
fn why_text(item: &Item) -> String {
    let Some(why) = &item.why else {
        return String::new();
    };
    if why.held {
        return "\n  Orbit held back this job's log: it looks like it holds a secret.".into();
    }
    if why.lines.is_empty() {
        return String::new();
    }
    let step = if why.step.is_empty() {
        "Its log ends:".to_string()
    } else {
        format!("Failed at step {}:", code(&why.step))
    };
    let lines: Vec<String> = why
        .lines
        .iter()
        .map(|l| l.replace("<!--", "<! --"))
        .collect();
    let ticks = lines
        .iter()
        .flat_map(|l| l.split(|c| c != '`').map(str::len))
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((ticks + 1).max(3));
    let body: Vec<String> = lines.iter().map(|l| format!("  {l}")).collect();
    format!("\n  {step}\n  {fence}text\n{}\n  {fence}", body.join("\n"))
}
fn items_text(t: &Thread, p: &Problem) -> String {
    // Failing first, then by number.
    let mut items: Vec<&Item> = t.items.iter().collect();
    items.sort_by_key(|i| (!i.failing, i.number));
    let mut lines: Vec<String> = items
        .iter()
        .take(MAX_ITEMS)
        .enumerate()
        .map(|(n, i)| {
            let mut line = item_line(i, t, p);
            if i.failing && p.is_open() {
                if let Some(rerun) = crate::reruns::text(i) {
                    line = format!("{line}\n  {rerun}");
                }
                if n < MAX_WHY {
                    line += &why_text(i);
                }
            }
            line
        })
        .collect();
    if items.len() > MAX_ITEMS {
        lines.push(format!("- and {} more", items.len() - MAX_ITEMS));
    }
    lines.join("\n")
}
/// The hidden line at the end of a comment or issue, with its items for
/// agents that read the raw text, and the problem's state when there is
/// one.
fn data(t: &Thread, p: Option<&Problem>) -> String {
    // A comment only about merging has no problem to list items for.
    let items: Vec<_> = t
        .items
        .iter()
        .filter(|_| p.is_some())
        .take(MAX_ITEMS)
        .map(|i| {
            let mut item = json!({
                "id": format!("I{}", i.number),
                "check": i.check,
                "failing": i.failing,
                "since": i.since,
                "at": i.at,
            });
            // An alert's number and severity, rather than a check.
            if i.kind == ItemKind::Alert {
                item["kind"] = json!(i.kind);
                item["alert"] = json!(i.alert);
                let open = p.and_then(|p| p.security_alerts.iter().find(|a| a.number == i.alert));
                if let Some(a) = open {
                    item["severity"] = json!(a.severity);
                }
                if let Some(o) = item.as_object_mut() {
                    o.remove("check");
                }
            }
            // A risk's number, kind, severity and place, rather than a
            // check.
            if i.kind == ItemKind::Risk {
                item["kind"] = json!(i.kind);
                item["risk"] = json!(i.risk);
                let open = p.and_then(|p| p.risks.iter().find(|r| r.number == i.risk));
                if let Some(r) = open {
                    item["severity"] = json!(r.severity);
                    item["rule"] = json!(r.kind.key());
                    item["path"] = json!(r.path);
                    item["line"] = json!(r.line);
                }
                if let Some(o) = item.as_object_mut() {
                    o.remove("check");
                }
            }
            // Only for a base that moved on or a repository with no CI, so
            // older bodies stay as they were: the branch, and no check.
            let branch = match i.kind {
                ItemKind::Check | ItemKind::Alert | ItemKind::Risk => None,
                ItemKind::BaseMoved => Some("base"),
                ItemKind::NoCi => Some("branch"),
            };
            if let Some(name) = branch {
                item["kind"] = json!(i.kind);
                item[name] = json!(i.check);
                if let Some(o) = item.as_object_mut() {
                    o.remove("check");
                }
            }
            // Only with an answer, so older bodies stay as they were.
            if let Some(a) = &i.answer {
                item["answer"] = json!({
                    "kind": a.kind,
                    "commit": a.commit,
                    "check": a.check,
                });
            }
            // The same for reruns.
            let reruns = crate::reruns::done(i);
            if reruns > 0 {
                item["reruns"] = json!(reruns);
            }
            item
        })
        .collect();
    let mut v = json!({
        "version": 1,
        "kind": match (t.kind, p.map(|p| p.kind)) {
            (_, Some(ProblemKind::NoCi)) => "no_ci",
            (_, Some(ProblemKind::Security)) => "security",
            (_, Some(ProblemKind::WorkflowRisks)) => "workflows",
            (ThreadKind::Comment, _) => "pull_request",
            (ThreadKind::Issue, _) => "default_branch",
        },
        "repo": t.repo,
        "open": p.is_some_and(Problem::is_open),
        "passed": p.is_some_and(Problem::passed),
        "items": items,
    });
    // A comment only about merging has no problem to have passed.
    if let (None, Some(o)) = (p, v.as_object_mut()) {
        o.remove("passed");
    }
    if let Some(source) = t.source {
        v["source"] = json!(source);
    }
    format!("{}{} -->", t.tag(), hidden(&v))
}
/// JSON for a hidden line, which nothing in it can end.
fn hidden(v: &serde_json::Value) -> String {
    v.to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}
/// The top of a pull request's comment: whether it is clear to merge, and
/// a hidden line naming the head and base that is for, so an agent can
/// tell a stale answer from a fresh one.
fn ready_text(r: &Ready) -> String {
    let line = if r.clear {
        let line = format!(
            "**Clear to merge** at {} against {} at {}.",
            code(short(&r.head_sha)),
            code(&r.base),
            code(short(&r.base_sha))
        );
        // What Orbit couldn't check.
        match r.text.as_str() {
            "" => line,
            note => format!("{line} {}", plain(note)),
        }
    } else if r.turn.is_none() {
        format!("**Not clear to merge.** {}", plain(&r.text))
    } else if r.reminds() {
        format!(
            "**Not clear to merge yet.** {} **Reminder:** it has waited on the author for more than {} hours.",
            plain(&r.text),
            NUDGE_AGENT.num_hours()
        )
    } else {
        format!("**Not clear to merge yet.** {}", plain(&r.text))
    };
    let v = json!({
        "version": 1,
        "clear": r.clear,
        "head": r.head_sha,
        "base": r.base,
        "baseSha": r.base_sha,
        "turn": r.turn,
    });
    format!("{line}\n\n<!-- orbit-ready {} -->", hidden(&v))
}
fn failing_count(t: &Thread) -> usize {
    t.items.iter().filter(|i| i.failing).count()
}
fn checks(n: usize) -> String {
    if n == 1 {
        "1 check fails".into()
    } else {
        format!("{n} checks fail")
    }
}

/// Orbit's comment on a pull request: whether it is clear to merge, for
/// one Orbit reviews automatically, then its CI problem, if any.
pub fn comment_body(t: &Thread, p: Option<&Problem>) -> String {
    let ci = p.map(|p| format!("{}\n\n{}", ci_status(t, p), items_text(t, p)));
    let Some(r) = &t.ready else {
        return format!(
            "## Orbit · CI\n\n{}\n\nOrbit reads CI after each sync and edits this comment when it changes. It never pushes to this branch.\n\n{}",
            ci.unwrap_or_default(),
            data(t, p)
        );
    };
    let ci = ci
        .map(|ci| format!("### CI\n\n{ci}\n\n"))
        .unwrap_or_default();
    format!(
        "## Orbit\n\n{}\n\n{ci}Orbit works out after each sync whether this is clear to merge, and edits this comment when that or CI changes. It never pushes to this branch.\n\n{}",
        ready_text(r),
        data(t, p)
    )
}
/// What a pull request's CI problem is, in a sentence.
fn ci_status(t: &Thread, p: &Problem) -> String {
    if p.is_open() && p.kind == ProblemKind::BaseMoved {
        let base = code(&p.base);
        format!(
            "Every check passes at {}, but {base} has moved on since they began. Merge {base} in and let CI run again, so they run with its latest changes.",
            code(short(&p.sha))
        )
    } else if p.is_open() {
        format!(
            "{} on this pull request at {}.",
            checks(failing_count(t)),
            code(short(&p.sha))
        )
    } else if p.passed() {
        format!(
            "Every check passes at {}. Nothing is left to fix here.",
            code(short(&p.closed_sha))
        )
    } else {
        format!(
            "Orbit has stopped watching CI here for now. {}",
            p.closed_reason.as_deref().unwrap_or_default()
        )
    }
}
/// The text a thread's comment or issue should have now, until it closes.
fn body_now(t: &Thread, p: &Problem) -> String {
    match t.kind {
        ThreadKind::Comment => comment_body(t, Some(p)),
        ThreadKind::Issue => issue_body(t, p, t.closed),
    }
}
fn branch(t: &Thread) -> String {
    if t.branch.is_empty() {
        "the default branch".into()
    } else {
        code(&t.branch)
    }
}
/// The title of Orbit's issue about a default branch, or its proposal.
pub fn issue_title(t: &Thread, p: &Problem) -> String {
    if p.kind == ProblemKind::NoCi {
        return "Add CI that runs on pull requests".into();
    }
    if p.kind == ProblemKind::WorkflowRisks {
        return "Fix the risky workflow settings".into();
    }
    if let Some(source) = p.source {
        return format!("Fix the open {}", source.many());
    }
    let branch = if t.branch.is_empty() {
        "the default branch"
    } else {
        &t.branch
    };
    format!("CI is failing on {branch}")
}
/// Orbit's issue about a default branch; `closing` once Orbit closes it.
pub fn issue_body(t: &Thread, p: &Problem, closing: bool) -> String {
    if p.kind == ProblemKind::NoCi {
        return proposal_body(t, p);
    }
    if p.kind == ProblemKind::Security {
        return security_body(t, p);
    }
    if p.kind == ProblemKind::WorkflowRisks {
        return workflows_body(t, p);
    }
    let on = branch(t);
    let status = if p.is_open() {
        let since = if p.since_sha == p.sha {
            String::new()
        } else {
            format!(", since {}", code(short(&p.since_sha)))
        };
        format!(
            "{} on {on} at {}{since}.",
            checks(failing_count(t)),
            code(short(&p.sha))
        )
    } else if p.passed() && closing {
        format!(
            "Every check passed on {on} at {} and stayed green for an hour.",
            code(short(&p.closed_sha))
        )
    } else if p.passed() {
        format!(
            "Every check passes on {on} at {}. Orbit closes this issue once it stays green for an hour.",
            code(short(&p.closed_sha))
        )
    } else {
        format!(
            "Orbit has stopped watching CI here. {}",
            p.closed_reason.as_deref().unwrap_or_default()
        )
    };
    let broke = match p.broke.as_ref().and_then(crate::broke::markdown) {
        Some(found) => format!("{found}\n\n"),
        None => String::new(),
    };
    format!(
        "## Orbit · CI\n\n{status}\n\n{broke}{}\n\nOrbit opened this issue because CI fails on {on}. It edits this issue as CI changes, and closes it once every check has passed for an hour.\n\n{}",
        items_text(t, p),
        data(t, Some(p))
    )
}
/// Orbit's issue proposing CI for a repository where none runs on pull
/// requests, with the workflow Orbit built from its files. The paths and
/// branch are the repository's words, so they are only ever code, and the
/// workflow holds only Orbit's own text.
fn proposal_body(t: &Thread, p: &Problem) -> String {
    let on = branch(t);
    let found = p.found.clone().unwrap_or_default();
    let status = if p.is_open() {
        let why = if found.workflows.is_empty() {
            format!("This repository has no workflows on {on}")
        } else {
            let paths: Vec<String> = found.workflows.iter().map(|w| code(w)).collect();
            format!(
                "None of the workflows on {on} runs on pull requests ({})",
                paths.join(", ")
            )
        };
        let proposal = if found.proposal.is_empty() {
            "Orbit couldn't tell from the files at the top of this repository how it builds and tests, so it has no workflow to suggest. One in `.github/workflows` that runs the project's tests on `pull_request` is enough.".to_string()
        } else {
            let from: Vec<String> = found.from.iter().map(|f| code(f)).collect();
            format!(
                "Here is a workflow to start from, as `.github/workflows/ci.yml`. Orbit built it from {} at {}, so check its commands are the ones this project uses.\n\n```yaml\n{}```",
                and(&from),
                code(short(&p.sha)),
                found.proposal
            )
        };
        format!("{why}, so nothing checks a change before it merges.\n\n{proposal}")
    } else if p.passed() {
        format!(
            "A workflow on {on} runs on pull requests at {}. Nothing is left to do here.",
            code(short(&p.closed_sha))
        )
    } else {
        format!(
            "Orbit has stopped proposing CI here. {}",
            p.closed_reason.as_deref().unwrap_or_default()
        )
    };
    format!(
        "## Orbit · CI\n\n{status}\n\n{}\n\nOrbit opened this issue because no CI runs on pull requests here. It edits this issue as it reads new commits on {on}, and closes it once a workflow there runs on pull requests.\n\n{}",
        items_text(t, p),
        data(t, Some(p))
    )
}
/// Orbit's issue about a kind of security alert in a private repository.
/// A secret's value is never in it, as Orbit never has one, and secrets
/// never get an issue (`tells_agent`).
fn security_body(t: &Thread, p: &Problem) -> String {
    let source = p.source.unwrap_or_default();
    let status = if p.is_open() {
        let n = p.security_alerts.len();
        let urgent = p.security_alerts.iter().filter(|a| a.needs_you()).count();
        let open = if n == 1 {
            format!("GitHub has 1 open {} here", source.one())
        } else {
            format!("GitHub has {n} open {} here", source.many())
        };
        let urgent = match (n, urgent) {
            (_, 0) => String::new(),
            (1, _) => ", and it is high or critical".to_string(),
            (_, u) => format!(", {u} of them high or critical"),
        };
        format!("{open}{urgent}. Each item says what to change.")
    } else if p.resolved() {
        format!(
            "GitHub says every {} here is fixed or dismissed. Nothing is left to do here.",
            source.one()
        )
    } else if p.cleared() {
        format!(
            "No {} are open here now. {}",
            source.many(),
            vanished_text(p)
        )
    } else {
        format!(
            "Orbit has stopped watching these alerts. {}",
            p.closed_reason.as_deref().unwrap_or_default()
        )
    };
    format!(
        "## Orbit · Security\n\n{status}\n\n{}\n\nOrbit opened this issue because GitHub found these alerts in this private repository. It edits this issue as they change, and closes it once none of them is open on GitHub. An item counts as fixed only once GitHub marks its alert fixed, which can wait until GitHub checks the default branch again. Dismissing an alert is a person's call.\n\n{}",
        items_text(t, p),
        data(t, Some(p))
    )
}
/// Orbit's issue about risky settings in a private repository's workflow
/// files.
fn workflows_body(t: &Thread, p: &Problem) -> String {
    let on = branch(t);
    let status = if p.is_open() {
        let (many, high) = crate::risks::count(&p.risks);
        format!(
            "Orbit found {many} in the workflows on {on} at {}{high}. Each item says what to change.",
            code(short(&p.sha))
        )
    } else if p.passed() {
        format!(
            "Orbit found no risky settings in the workflows on {on} at {}. Nothing is left to do here.",
            code(short(&p.closed_sha))
        )
    } else {
        format!(
            "Orbit has stopped checking the workflows here. {}",
            p.closed_reason.as_deref().unwrap_or_default()
        )
    };
    format!(
        "## Orbit · Workflows\n\n{status}\n\n{}\n\nOrbit opened this issue because it found these settings in `.github/workflows` in this private repository. Each lets someone else's code or words run with this repository's token or secrets. Orbit checks the files again at each new commit on {on}, edits this issue as they change, and closes it once none is left.\n\n{}",
        items_text(t, p),
        data(t, Some(p))
    )
}
/// "A", "A and B", "A, B and C".
fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
/// How many of a problem's alerts GitHub no longer has, without saying
/// what became of them, in a sentence.
fn vanished_text(p: &Problem) -> String {
    match p.vanished.len() {
        1 => "GitHub no longer has 1 of them, so Orbit can't say it was fixed or dismissed.".into(),
        n => format!(
            "GitHub no longer has {n} of them, so Orbit can't say they were fixed or dismissed."
        ),
    }
}
/// The note Orbit leaves on an issue as it closes it.
pub fn closing_note(t: &Thread, p: &Problem) -> String {
    let closed_at = p.closed_at.as_deref().unwrap_or_default();
    let text = if p.resolved() {
        format!(
            "GitHub says every {} here is fixed or dismissed, so Orbit is closing this issue.",
            p.source.unwrap_or_default().one()
        )
    } else if p.cleared() {
        format!(
            "No {} are open here now, so Orbit is closing this issue. {}",
            p.source.unwrap_or_default().many(),
            vanished_text(p)
        )
    } else if p.passed() && p.kind == ProblemKind::WorkflowRisks {
        format!(
            "Orbit found no risky settings in the workflows on {} at {}, so it is closing this issue.",
            branch(t),
            code(short(&p.closed_sha))
        )
    } else if p.passed() && p.kind == ProblemKind::NoCi {
        format!(
            "A workflow on {} runs on pull requests at {}, so Orbit is closing this issue.",
            branch(t),
            code(short(&p.closed_sha))
        )
    } else if p.passed() {
        format!(
            "Every check passed on {} at {} and stayed green for an hour, so Orbit is closing this issue.",
            branch(t),
            code(short(&p.closed_sha))
        )
    } else {
        format!(
            "Orbit is closing this issue. {}",
            p.closed_reason.as_deref().unwrap_or_default()
        )
    };
    format!("{text}\n\n{}", note_tag(&t.key, closed_at))
}

/// Writes left on their way to GitHub when Orbit stopped: GitHub may or may
/// not have them. Run before a pass sends anything. Returns whether any
/// were left. A change to a setting is sent apart from these, so
/// `crate::baseline::interrupted` looks after its own.
pub fn interrupted(journal: &mut [Write], threads: &mut [Thread], now: DateTime<Utc>) -> bool {
    let mut any = false;
    for w in journal
        .iter_mut()
        .filter(|w| w.state == WriteState::Sending && w.action != Action::ChangeSetting)
    {
        w.state = WriteState::Failed;
        w.done_at = Some(stamp(now));
        w.error = Some(
            "Orbit stopped before GitHub answered. It looks on GitHub before trying again, so nothing is posted twice.".into(),
        );
        if creates(w.action) {
            if let Some(t) = threads.iter_mut().find(|t| t.key == w.thread) {
                t.unsure = true;
            }
        }
        any = true;
    }
    any
}
/// Whether an action makes the thread's comment or issue.
fn creates(action: Action) -> bool {
    matches!(action, Action::PostComment | Action::OpenIssue)
}
/// Whether an action makes something new on GitHub, so Orbit looks for it
/// first.
pub fn looks_first(action: Action) -> bool {
    matches!(
        action,
        Action::Find | Action::PostComment | Action::OpenIssue | Action::NoteOnIssue
    )
}
/// Records what Orbit found on GitHub before making something: a comment
/// or issue an earlier try posted becomes the thread's, to be brought up to
/// date, and a closing note already there counts as posted. Found nothing
/// means nothing earlier got there; it keeps the count of failed tries, so
/// a post that keeps failing still waits longer each time. A search that
/// failed waits like a failed write.
pub fn looked(
    threads: &mut [Thread],
    planned: &Planned,
    result: Result<Option<Posted>, String>,
    now: DateTime<Utc>,
) {
    let Some(t) = threads.iter_mut().find(|t| t.key == planned.thread) else {
        return;
    };
    t.updated_at = stamp(now);
    let found = match result {
        Ok(found) => found,
        Err(error) => {
            wait(t, error, now);
            return;
        }
    };
    t.error = None;
    t.retry_at = None;
    let Some(found) = found else {
        t.unsure = false;
        return;
    };
    t.tries = 0;
    match planned.action {
        Action::NoteOnIssue => t.noted = planned.closed_at.clone(),
        _ => {
            t.unsure = false;
            // The items it shows have been on GitHub since it was written.
            recover(t, &found);
            t.url = found.url;
            // What it says now is unknown, so it is brought up to date.
            t.posted.clear();
            match t.kind {
                ThreadKind::Comment => {
                    // When it got there is unknown, so the agent's wait
                    // starts now.
                    first_shown(t, &stamp(now));
                    t.comment_id = Some(found.id);
                    t.read_to = t.read_to.max(found.id);
                }
                ThreadKind::Issue => t.number = found.number.or(t.number),
            }
        }
    }
}
/// A wait on the agent starts once its comment is first on GitHub, as the
/// agent can only see it from then on. A person's doesn't: their wait,
/// and the Needs you item it makes, are the same whether Orbit posted.
fn first_shown(t: &mut Thread, at: &str) {
    if t.comment_id.is_some() {
        return;
    }
    if let Some(r) = t.ready.as_mut().filter(|r| r.turn == Some(Turn::Author)) {
        r.since = at.into();
    }
}
fn wait(t: &mut Thread, error: String, now: DateTime<Utc>) {
    t.tries += 1;
    let wait = Duration::minutes(1i64 << (t.tries - 1).min(6)).min(MAX_RETRY);
    t.retry_at = Some(stamp(now + wait));
    t.error = Some(error);
}
/// Records a write before it is sent.
pub fn record(journal: &mut Vec<Write>, planned: &Planned, now: DateTime<Utc>) -> String {
    let id = crate::types::id();
    journal.push(Write {
        id: id.clone(),
        thread: planned.thread.clone(),
        action: planned.action,
        repo: planned.repo.clone(),
        number: planned.number,
        state: WriteState::Sending,
        at: stamp(now),
        done_at: None,
        url: String::new(),
        error: None,
        setting: None,
    });
    id
}
/// Whether GitHub said the comment or issue is gone.
pub fn is_gone(error: &str) -> bool {
    error.contains("HTTP 404") || error.contains("HTTP 410")
}
/// Records what GitHub said to a write, on its journal entry and its thread.
pub fn settle(
    threads: &mut [Thread],
    journal: &mut [Write],
    id: &str,
    planned: &Planned,
    result: Result<Posted, String>,
    now: DateTime<Utc>,
) {
    let at = stamp(now);
    let thread = threads.iter_mut().find(|t| t.key == planned.thread);
    let entry = journal.iter_mut().find(|w| w.id == id);
    match result {
        Ok(posted) => {
            if let Some(w) = entry {
                w.state = WriteState::Done;
                w.done_at = Some(at.clone());
                w.url = posted.url.clone();
                w.number = posted.number.or(w.number);
            }
            let Some(t) = thread else { return };
            t.error = None;
            t.tries = 0;
            t.retry_at = None;
            t.unsure = false;
            t.updated_at = at.clone();
            match planned.action {
                // Never sent as a write; see `looked`.
                Action::Find => {}
                Action::PostComment | Action::EditComment => {
                    first_shown(t, &at);
                    // GitHub has the reminder once it took a body giving it.
                    if let Some(r) = t.ready.as_mut().filter(|r| r.reminds()) {
                        if r.reminded.is_none() && planned.body.contains(&ready_text(r)) {
                            r.reminded = Some(at.clone());
                        }
                    }
                    t.comment_id = Some(posted.id);
                    t.url = posted.url;
                    t.posted = planned.body.clone();
                    // Replies before Orbit's comment aren't about its items.
                    t.read_to = t.read_to.max(posted.id);
                    shown(t, &posted.updated_at);
                }
                Action::OpenIssue | Action::EditIssue | Action::ReopenIssue => {
                    t.number = posted.number.or(t.number);
                    t.url = posted.url;
                    t.posted = planned.body.clone();
                    t.closed = false;
                    shown(t, &posted.updated_at);
                }
                Action::CloseIssue => {
                    t.posted = planned.body.clone();
                    t.closed = true;
                    shown(t, &posted.updated_at);
                }
                Action::NoteOnIssue => t.noted = planned.closed_at.clone(),
                // Settled by `crate::reruns::settle`.
                Action::RerunJob => {}
                // Settled by `crate::baseline::settle`.
                Action::ChangeSetting => {}
            }
        }
        Err(error) => {
            if let Some(w) = entry {
                w.state = WriteState::Failed;
                w.done_at = Some(at.clone());
                w.error = Some(error.clone());
            }
            let Some(t) = thread else { return };
            t.updated_at = at;
            // GitHub may have taken a post it didn't answer for.
            if creates(planned.action) {
                t.unsure = true;
            }
            // Something already posted that GitHub no longer has was
            // deleted by someone; Orbit doesn't post it again.
            if t.is_posted() && is_gone(&error) {
                t.gone = true;
                t.error = Some(match t.kind {
                    ThreadKind::Comment => {
                        "Orbit's comment is gone from GitHub, so Orbit stopped updating it.".into()
                    }
                    ThreadKind::Issue => {
                        "Orbit's issue is gone from GitHub, so Orbit stopped updating it.".into()
                    }
                });
                return;
            }
            wait(t, error, now);
        }
    }
}

/// Something Bit tells you about telling agents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Orbit posted a thread's comment or opened its issue.
    Posted(String),
    /// Orbit closed a thread's issue.
    Closed(String),
    /// An agent answered, or Orbit checked a fix.
    Heard(Box<Heard>),
    /// Orbit reran an item's job.
    Reran { thread: String, number: u32 },
    /// An item's job failed again after a rerun, and `ask` when Bit asks
    /// before rerunning it again.
    FailedAgain {
        thread: String,
        number: u32,
        ask: bool,
    },
    /// Bit can propose CI for a repository with none on pull requests, once
    /// you tap.
    Offered(String),
}
/// Where a thread is, in plain words: `acme/api #12` for a pull request's
/// comment, `main in acme/api` for a default branch's issue.
pub fn place(t: &Thread) -> String {
    if t.source.is_some() {
        return t.repo.clone();
    }
    match t.kind {
        ThreadKind::Comment => match t.number {
            Some(n) => format!("{} #{n}", t.repo),
            None => t.repo.clone(),
        },
        ThreadKind::Issue if t.branch.is_empty() => format!("the default branch in {}", t.repo),
        ThreadKind::Issue => format!("{} in {}", t.branch, t.repo),
    }
}
/// What Bit says about an event, and the problem and repository it is
/// about. Only answers worth your attention are told: a declined item, or
/// a fix CI doesn't bear out.
pub fn event_text(
    threads: &[Thread],
    problems: &[Problem],
    e: &Event,
) -> Option<(String, String, String)> {
    let key = match e {
        Event::Posted(key) | Event::Closed(key) | Event::Offered(key) => key,
        Event::Heard(h) => &h.thread,
        Event::Reran { thread, .. } | Event::FailedAgain { thread, .. } => thread,
    };
    let t = threads.iter().find(|t| &t.key == key)?;
    let p = problems.iter().find(|p| p.id == t.problem_id);
    let issue = || {
        t.number
            .map_or("its issue".to_string(), |n| format!("issue #{n}"))
    };
    let proposal = p.is_some_and(|p| p.kind == ProblemKind::NoCi);
    let text = match e {
        Event::Offered(_) => format!(
            "No CI runs on {}'s pull requests. Tap Propose CI on Operations and Orbit opens an issue for its agent, with a workflow to start from.",
            t.repo
        ),
        Event::Posted(_) if proposal => format!(
            "Orbit opened {} for the agent on {}, with a workflow to start from, as no CI runs on its pull requests.",
            issue(),
            t.repo
        ),
        Event::Closed(_) if proposal && p.is_some_and(|p| p.passed()) => format!(
            "Orbit closed {}: a workflow runs on {}'s pull requests now.",
            issue(),
            t.repo
        ),
        Event::Closed(_) if proposal => format!(
            "Orbit closed {} on {}. {}",
            issue(),
            t.repo,
            p?.closed_reason.as_deref().unwrap_or_default()
        ),
        Event::Posted(_) if p?.kind == ProblemKind::Security => {
            let p = p?;
            let source = p.source.unwrap_or_default();
            let n = p.security_alerts.len();
            let what = if n == 1 {
                format!("1 {}", source.one())
            } else {
                format!("{n} {}", source.many())
            };
            format!(
                "Orbit opened {} to tell the agent on {} about {what}.",
                issue(),
                t.repo
            )
        }
        Event::Posted(_) if p?.kind == ProblemKind::WorkflowRisks => {
            let what = match p?.risks.len() {
                1 => "1 risky workflow setting".to_string(),
                n => format!("{n} risky workflow settings"),
            };
            format!(
                "Orbit opened {} to tell the agent on {} about {what}.",
                issue(),
                t.repo
            )
        }
        Event::Closed(_) if p.is_some_and(|p| p.kind == ProblemKind::WorkflowRisks) => {
            match p.filter(|p| p.passed()) {
                Some(_) => format!(
                    "Orbit closed {}: {}'s workflows have no risky settings now.",
                    issue(),
                    t.repo
                ),
                None => format!(
                    "Orbit closed {} on {}. {}",
                    issue(),
                    t.repo,
                    p?.closed_reason.as_deref().unwrap_or_default()
                ),
            }
        }
        Event::Closed(_) if p.is_some_and(|p| p.resolved()) => format!(
            "Orbit closed {}: GitHub says {}'s {} are fixed or dismissed.",
            issue(),
            t.repo,
            p?.source.unwrap_or_default().many()
        ),
        Event::Closed(_) if p.is_some_and(|p| p.kind == ProblemKind::Security) => format!(
            "Orbit closed {} on {}. {}",
            issue(),
            t.repo,
            p?.closed_reason.as_deref().unwrap_or_default()
        ),
        Event::Posted(_) if p?.kind == ProblemKind::BaseMoved => format!(
            "Orbit told the agent on {} that {} moved on since its checks began.",
            place(t),
            p?.base
        ),
        Event::Posted(_) => {
            let failing = &p?.failing;
            let names = crate::problems::names(failing);
            let fail = if failing.len() == 1 { "fails" } else { "fail" };
            match t.kind {
                ThreadKind::Comment => {
                    format!("Orbit told the agent on {} that {names} {fail}.", place(t))
                }
                ThreadKind::Issue => format!(
                    "Orbit opened {} to tell the agent that {names} {fail} on {}.",
                    issue(),
                    place(t)
                ),
            }
        }
        Event::Closed(_) => {
            if p.is_some_and(|p| p.passed()) {
                format!(
                    "Orbit closed {}: {} stayed green for an hour.",
                    issue(),
                    place(t)
                )
            } else {
                format!(
                    "Orbit closed {}, as it stopped watching {}.",
                    issue(),
                    place(t)
                )
            }
        }
        Event::Heard(h) => {
            let a = &h.answer;
            // A high or critical alert declined is in Needs you, which
            // tells you of it.
            let urgent = t
                .items
                .iter()
                .any(|i| i.number == h.number && p.is_some_and(|p| yours_to_decide(i, p)));
            if a.kind == AnswerKind::Declined && urgent {
                return None;
            }
            let what = format!("I{} ({})", h.number, h.check);
            let commit = a.commit.as_deref().map(short).unwrap_or_default();
            let said = format!("The agent on {} said {what} is fixed in {commit}", place(t));
            match (a.kind, a.check) {
                (AnswerKind::Declined, _) => match &a.text {
                    Some(reason) => {
                        format!("The agent on {} declined {what}: {reason}", place(t))
                    }
                    None => format!("The agent on {} declined {what}.", place(t)),
                },
                (AnswerKind::Fixed, Some(FixCheck::Fails)) => {
                    format!("{said}, but it still fails there.")
                }
                (AnswerKind::Fixed, Some(FixCheck::NotOnBranch)) => {
                    format!("{said}, but that commit isn't on {}.", t.branch)
                }
                (AnswerKind::Fixed, Some(FixCheck::NotFound)) => {
                    format!("{said}, but Orbit can't find that commit.")
                }
                _ => return None,
            }
        }
        Event::Reran { number, .. } => {
            let item = t.items.iter().find(|i| i.number == *number)?;
            let r = item.reruns.iter().rfind(|r| r.done)?;
            let what = format!("I{} ({})", item.number, item.name());
            if crate::reruns::done(item) > 1 {
                format!(
                    "Orbit reran {what} on {} a second time, as it still looks like bad luck: {}",
                    place(t),
                    r.reason
                )
            } else {
                format!(
                    "Orbit reran {what} on {}, as it looks like bad luck: {}",
                    place(t),
                    r.reason
                )
            }
        }
        Event::FailedAgain { number, ask, .. } => {
            let item = t.items.iter().find(|i| i.number == *number)?;
            let what = format!("I{} ({})", item.number, item.name());
            let flaky = item.why.as_ref().and_then(|w| w.flaky.as_deref());
            match flaky.filter(|_| *ask) {
                Some(reason) => format!(
                    "{what} failed again on {} after a rerun. It still looks like bad luck: {reason} Tap Rerun again on Operations to try once more.",
                    place(t)
                ),
                None => format!(
                    "{what} failed again on {} after a rerun, so it's likely real.",
                    place(t)
                ),
            }
        }
    };
    Some((t.problem_id.clone(), t.repo.clone(), text))
}

/// Whether Orbit still has work for a thread: an issue it hasn't closed, a
/// post it may have to find, or a comment on a pull request that is still
/// open and may fail again, which keeps its item numbers. A comment someone
/// deleted stays too while its pull request is open, so a later failure
/// doesn't post it again.
fn in_use(t: &Thread, prs: &[PullRequest]) -> bool {
    let pr_open = t.kind == ThreadKind::Comment
        && prs
            .iter()
            .any(|p| t.pr_id.as_deref() == Some(p.id.as_str()));
    pr_open
        || (!t.gone && (t.unsure || (t.kind == ThreadKind::Issue && t.is_posted() && !t.closed)))
}
/// The problems that threads still need, which pruning keeps.
pub fn pinned(threads: &[Thread], prs: &[PullRequest]) -> HashSet<String> {
    threads
        .iter()
        .filter(|t| in_use(t, prs))
        .map(|t| t.problem_id.clone())
        .collect()
}

/// Drops threads whose problem is gone and that Orbit has no more work
/// for, and journal entries older than closed problems are kept or past the
/// newest `MAX_JOURNAL`. Returns whether any went.
pub fn prune(
    threads: &mut Vec<Thread>,
    journal: &mut Vec<Write>,
    problems: &[Problem],
    prs: &[PullRequest],
    now: DateTime<Utc>,
) -> bool {
    let before = (threads.len(), journal.len());
    threads.retain(|t| problems.iter().any(|p| p.id == t.problem_id) || in_use(t, prs));
    journal.retain(|w| time(&w.at).is_none_or(|at| now - at < KEEP_CLOSED));
    let excess = journal.len().saturating_sub(MAX_JOURNAL);
    journal.drain(..excess);
    before != (threads.len(), journal.len())
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
