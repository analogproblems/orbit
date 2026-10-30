//! Telling a project's agent about failing CI, on GitHub where it already
//! listens: one Orbit comment on each pull request whose checks fail,
//! edited in place, and one issue labelled `orbit` for each default branch
//! that fails. Each failing check is an item, I1, I2 and so on, numbered
//! once in its comment or issue and never reused, so an agent can name it.
//! So is a pull request's base branch moving on since its checks began.
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
use crate::{
    ci::short,
    problems::{stamp, time, Problem, ProblemKind, FLAP_QUIET, KEEP_CLOSED},
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
    /// branch.
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
}

impl Item {
    /// What it is about, in a few words: the check, or "main moved on".
    pub fn name(&self) -> String {
        match self.kind {
            ItemKind::Check => self.check.clone(),
            ItemKind::BaseMoved => format!("{} moved on", self.check),
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
    let rule = match t.kind {
        ThreadKind::Comment => &buddy.rules.comment_on_prs,
        ThreadKind::Issue => &buddy.rules.open_issues,
    };
    rule_allows(rule, t.allowed, buddy.enabled) && watching(s, t)
}
/// Whether Orbit watches what a thread is about: Watch CI is on, its
/// repository is connected, and for a comment, Orbit still follows the pull
/// request.
pub fn watching(s: &Snapshot, t: &Thread) -> bool {
    let watched = s.settings.watch_ci
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
/// problem, and the pull request's thread for a pull request's. Returns
/// whether anything changed.
pub fn refresh(threads: &mut Vec<Thread>, problems: &[Problem], now: DateTime<Utc>) -> bool {
    let before = threads.clone();
    let mut order: Vec<&Problem> = problems.iter().collect();
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
                        ProblemKind::DefaultBranch => ThreadKind::Issue,
                        ProblemKind::PullRequest | ProblemKind::BaseMoved => ThreadKind::Comment,
                    },
                    repo: p.repo.clone(),
                    number: match p.kind {
                        ProblemKind::DefaultBranch => None,
                        ProblemKind::PullRequest | ProblemKind::BaseMoved => {
                            p.pr_id.as_deref().and_then(pr_number)
                        }
                    },
                    pr_id: p.pr_id.clone(),
                    branch: p.branch.clone(),
                    problem_id: p.id.clone(),
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
fn key_of(p: &Problem) -> String {
    match p.kind {
        ProblemKind::DefaultBranch => format!("issue:{}", p.id),
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
/// each check that fails for the first time, or for a pull request's base
/// moving on the first time.
fn items_from(t: &mut Thread, p: &Problem) {
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
    for item in &mut t.items {
        let failing = match item.kind {
            ItemKind::Check => p.failing.iter().any(|c| c.name == item.check),
            ItemKind::BaseMoved => moved,
        };
        if failing && item.kind == ItemKind::BaseMoved {
            item.check = p.base.clone();
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
    let p = problem?;
    let plan = |action: Action, body: String| Planned {
        thread: t.key.clone(),
        action,
        repo: t.repo.clone(),
        number: t.number,
        comment_id: t.comment_id,
        title: String::new(),
        marker: t.tag(),
        completed: p.passed(),
        closed_at: None,
        rerun: None,
        body,
    };
    match t.kind {
        ThreadKind::Comment => {
            if t.comment_id.is_none() {
                return (p.is_open() && may_post)
                    .then(|| plan(Action::PostComment, comment_body(t, p)));
            }
            // A closed pull request has nobody left to tell.
            if p.closed_reason.as_deref() == Some(PR_CLOSED) {
                return None;
            }
            let body = body_now(t, p);
            (body != t.posted).then(|| plan(Action::EditComment, body))
        }
        ThreadKind::Issue => {
            if t.number.is_none() {
                return (p.is_open() && may_post).then(|| Planned {
                    title: issue_title(t),
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
/// Whether a default branch's issue should be closed: its problem is over,
/// and if it passed, it has stayed green long enough not to come back as
/// the same problem.
fn closes(p: &Problem, now: DateTime<Utc>) -> bool {
    let Some(closed) = p.closed_at.as_deref().and_then(time) else {
        return false;
    };
    !p.passed() || now - closed >= CLOSE_AFTER
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
    if item.kind == ItemKind::BaseMoved {
        return base_line(item, t, p);
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
/// agents that read the raw text.
fn data(t: &Thread, p: &Problem) -> String {
    let items: Vec<_> = t
        .items
        .iter()
        .take(MAX_ITEMS)
        .map(|i| {
            let mut item = json!({
                "id": format!("I{}", i.number),
                "check": i.check,
                "failing": i.failing,
                "since": i.since,
                "at": i.at,
            });
            // Only for a base that moved on, so older bodies stay as they
            // were: its base branch, and no check.
            if i.kind == ItemKind::BaseMoved {
                item["kind"] = json!(i.kind);
                item["base"] = json!(i.check);
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
    let v = json!({
        "version": 1,
        "kind": match t.kind {
            ThreadKind::Comment => "pull_request",
            ThreadKind::Issue => "default_branch",
        },
        "repo": t.repo,
        "open": p.is_open(),
        "passed": p.passed(),
        "items": items,
    });
    let text = v
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!("{}{text} -->", t.tag())
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

/// Orbit's comment on a pull request.
pub fn comment_body(t: &Thread, p: &Problem) -> String {
    let status = if p.is_open() && p.kind == ProblemKind::BaseMoved {
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
    };
    format!(
        "## Orbit · CI\n\n{status}\n\n{}\n\nOrbit reads CI after each sync and edits this comment when it changes. It never pushes to this branch.\n\n{}",
        items_text(t, p),
        data(t, p)
    )
}
/// The text a thread's comment or issue should have now, until it closes.
fn body_now(t: &Thread, p: &Problem) -> String {
    match t.kind {
        ThreadKind::Comment => comment_body(t, p),
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
/// The title of Orbit's issue about a default branch.
pub fn issue_title(t: &Thread) -> String {
    let branch = if t.branch.is_empty() {
        "the default branch"
    } else {
        &t.branch
    };
    format!("CI is failing on {branch}")
}
/// Orbit's issue about a default branch; `closing` once Orbit closes it.
pub fn issue_body(t: &Thread, p: &Problem, closing: bool) -> String {
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
        data(t, p)
    )
}
/// The note Orbit leaves on an issue as it closes it.
pub fn closing_note(t: &Thread, p: &Problem) -> String {
    let closed_at = p.closed_at.as_deref().unwrap_or_default();
    let text = if p.passed() {
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
/// were left.
pub fn interrupted(journal: &mut [Write], threads: &mut [Thread], now: DateTime<Utc>) -> bool {
    let mut any = false;
    for w in journal
        .iter_mut()
        .filter(|w| w.state == WriteState::Sending)
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
                    t.comment_id = Some(found.id);
                    t.read_to = t.read_to.max(found.id);
                }
                ThreadKind::Issue => t.number = found.number.or(t.number),
            }
        }
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
            t.updated_at = at;
            match planned.action {
                // Never sent as a write; see `looked`.
                Action::Find => {}
                Action::PostComment | Action::EditComment => {
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
}
/// Where a thread is, in plain words: `acme/api #12` for a pull request's
/// comment, `main in acme/api` for a default branch's issue.
pub fn place(t: &Thread) -> String {
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
        Event::Posted(key) | Event::Closed(key) => key,
        Event::Heard(h) => &h.thread,
        Event::Reran { thread, .. } | Event::FailedAgain { thread, .. } => thread,
    };
    let t = threads.iter().find(|t| &t.key == key)?;
    let p = problems.iter().find(|p| p.id == t.problem_id);
    let issue = || {
        t.number
            .map_or("its issue".to_string(), |n| format!("issue #{n}"))
    };
    let text = match e {
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
