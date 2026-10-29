//! Telling a project's agent about failing CI, on GitHub where it already
//! listens: one Orbit comment on each pull request whose checks fail,
//! edited in place, and one issue labelled `orbit` for each default branch
//! that fails. Each failing check is an item, I1, I2 and so on, numbered
//! once in its comment or issue and never reused, so an agent can name it.
//!
//! Everything here is worked out from the problems Orbit keeps
//! (`crate::problems`). The engine sends each write (`engine::ops`) after
//! recording it in the journal. A write is never sent twice: before Orbit
//! posts something new, it looks on GitHub for that thing's marker, in case
//! an earlier try reached GitHub just before Orbit stopped, and a thread
//! whose post may have got there looks for it before anything else.
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
/// The most journal entries kept; the oldest go first, and none are kept
/// longer than closed problems are.
pub const MAX_JOURNAL: usize = 200;
/// The longest wait before a failed write is tried again. The wait starts
/// at a minute and doubles with each failure in a row.
pub const MAX_RETRY: Duration = Duration::hours(1);

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

/// One failing check, as a comment or issue lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Item {
    /// 1 for I1. Never reused within its thread.
    pub number: u32,
    /// The check, such as "CI / Rust tests".
    pub check: String,
    pub url: String,
    /// Whether it failed at `at`, the latest commit Orbit saw.
    pub failing: bool,
    /// The commit it started failing at, this time.
    pub since: String,
    pub at: String,
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
}
/// What GitHub said to a write: where the comment or issue is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Posted {
    pub id: u64,
    pub number: Option<u64>,
    pub url: String,
}

/// Whether a rule lets Bit post: it is on, and the rule is Do it, or Ask
/// first and you said yes.
pub fn rule_allows(rule: &str, allowed: bool, bit_on: bool) -> bool {
    bit_on && (rule == "do_it" || (rule == "ask_first" && allowed))
}
/// Whether Bit may post something new for a thread now: its rule allows it,
/// Watch CI is on, its repository is connected, and for a comment, Orbit
/// still follows the pull request. Checked again right before each post.
pub fn may_post(s: &Snapshot, t: &Thread) -> bool {
    let buddy = &s.settings.buddy;
    let rule = match t.kind {
        ThreadKind::Comment => &buddy.rules.comment_on_prs,
        ThreadKind::Issue => &buddy.rules.open_issues,
    };
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
    rule_allows(rule, t.allowed, buddy.enabled) && watched && followed
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
                        ProblemKind::PullRequest => ThreadKind::Comment,
                    },
                    repo: p.repo.clone(),
                    number: match p.kind {
                        ProblemKind::DefaultBranch => None,
                        ProblemKind::PullRequest => p.pr_id.as_deref().and_then(pr_number),
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
        ProblemKind::PullRequest => format!("pr:{}", p.pr_id.as_deref().unwrap_or(&p.repo)),
    }
}
fn pr_number(id: &str) -> Option<u64> {
    id.rsplit_once('#').and_then(|(_, n)| n.parse().ok())
}
/// Marks each item failing or not as the problem stands now, adding one for
/// each check that fails for the first time.
fn items_from(t: &mut Thread, p: &Problem) {
    // Only a passing commit says a check stopped failing; when Orbit stops
    // watching for another reason, items stay as it last saw them.
    if !p.is_open() {
        if p.passed() {
            for item in &mut t.items {
                item.failing = false;
                item.at = p.closed_sha.clone();
            }
        }
        return;
    }
    for item in &mut t.items {
        let failing = p.failing.iter().any(|c| c.name == item.check);
        if failing && !item.failing {
            item.since = p.sha.clone();
        }
        item.failing = failing;
        item.at = p.sha.clone();
    }
    for check in &p.failing {
        match t.items.iter_mut().find(|i| i.check == check.name) {
            Some(item) => item.url = check.url.clone(),
            None => {
                let number = t.items.iter().map(|i| i.number).max().unwrap_or(0) + 1;
                t.items.push(Item {
                    number,
                    check: check.name.clone(),
                    url: check.url.clone(),
                    failing: true,
                    since: p.sha.clone(),
                    at: p.sha.clone(),
                });
            }
        }
    }
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
            let body = comment_body(t, p);
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
            let body = issue_body(t, p, t.closed);
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
    format!("`{text}`")
}
/// A link to a check's page, only to GitHub and only when it is safe in
/// Markdown.
fn details(url: &str) -> String {
    let safe = url.starts_with("https://github.com/")
        && !url
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '"'));
    if safe {
        format!(" · [details]({url})")
    } else {
        String::new()
    }
}
fn item_line(item: &Item, p: &Problem) -> String {
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
    } else if p.passed() {
        format!("passes at {}", code(short(&item.at)))
    } else {
        format!("not failing at {}", code(short(&item.at)))
    };
    let link = if item.failing {
        details(&item.url)
    } else {
        String::new()
    };
    format!(
        "- **I{}** · {} · {state}{link}",
        item.number,
        code(&item.check)
    )
}
fn items_text(t: &Thread, p: &Problem) -> String {
    // Failing first, then by number.
    let mut items: Vec<&Item> = t.items.iter().collect();
    items.sort_by_key(|i| (!i.failing, i.number));
    let mut lines: Vec<String> = items
        .iter()
        .take(MAX_ITEMS)
        .map(|i| item_line(i, p))
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
            json!({
                "id": format!("I{}", i.number),
                "check": i.check,
                "failing": i.failing,
                "since": i.since,
                "at": i.at,
            })
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
    let status = if p.is_open() {
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
    format!(
        "## Orbit · CI\n\n{status}\n\n{}\n\nOrbit opened this issue because CI fails on {on}. It edits this issue as CI changes, and closes it once every check has passed for an hour.\n\n{}",
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
            t.url = found.url;
            // What it says now is unknown, so it is brought up to date.
            t.posted.clear();
            match t.kind {
                ThreadKind::Comment => t.comment_id = Some(found.id),
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
                }
                Action::OpenIssue | Action::EditIssue | Action::ReopenIssue => {
                    t.number = posted.number.or(t.number);
                    t.url = posted.url;
                    t.posted = planned.body.clone();
                    t.closed = false;
                }
                Action::CloseIssue => {
                    t.posted = planned.body.clone();
                    t.closed = true;
                }
                Action::NoteOnIssue => t.noted = planned.closed_at.clone(),
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
