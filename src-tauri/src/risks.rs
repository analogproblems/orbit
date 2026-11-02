//! Risky settings in a repository's workflow files, read at each new
//! commit on its default branch with the rest of `crate::workflows`: a
//! third-party action not pinned to a full commit, a workflow with no
//! `permissions` or `write-all`, a `pull_request_target` or `workflow_run`
//! workflow that checks out a pull request's code, and text anyone can
//! write, such as an issue's title, put straight into a script.
//!
//! Orbit reads each file itself, with no network and nothing run. A file
//! it can't read (too big, not YAML, or one that grows past `MAX_WEIGHT`
//! once its aliases are expanded) isn't checked, and what Orbit found
//! there before stays (see `crate::problems::see_risks`). Nor is a risk
//! called fixed because a file has more than `MAX_PER_FILE` and it is no
//! longer among the most severe.
use crate::{security::Severity, text::shorten};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};
use yaml_rust2::{
    parser::{Event, Parser},
    scanner::TScalarStyle,
};

/// Workflow files bigger than this aren't checked.
pub const MAX_TEXT: usize = 512 * 1024;
/// How big a file may grow once its aliases are expanded, in nodes, with
/// each 64 bytes of text a node too. Aliases can make a small file
/// enormous; past this, Orbit doesn't check it.
const MAX_WEIGHT: usize = 200_000;
/// How deeply a file's lists and maps may nest.
const MAX_DEPTH: usize = 64;
/// The most risks Orbit keeps for one file, the most severe first.
pub const MAX_PER_FILE: usize = 50;
/// The longest text Orbit keeps from a file, such as an action's name.
pub(crate) const MAX_SUBJECT: usize = 120;

/// What a risk is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskKind {
    /// A third-party action, reusable workflow or Docker image not pinned
    /// to a full commit or digest, so whoever owns it can change what runs.
    #[default]
    Unpinned,
    /// A workflow whose jobs get the token's default permissions, or one
    /// that asks for `write-all`.
    Permissions,
    /// A `pull_request_target` or `workflow_run` workflow that checks out a
    /// pull request's code, which then runs with the repository's secrets.
    PullCheckout,
    /// Text anyone can write put straight into a script, where it can run
    /// as part of it.
    Injection,
    /// What `actionlint` found, when it is installed (`crate::lint`).
    Actionlint,
    /// What `zizmor` found, when it is installed.
    Zizmor,
}
impl RiskKind {
    pub fn key(self) -> &'static str {
        match self {
            RiskKind::Unpinned => "unpinned",
            RiskKind::Permissions => "permissions",
            RiskKind::PullCheckout => "pull_checkout",
            RiskKind::Injection => "injection",
            RiskKind::Actionlint => "actionlint",
            RiskKind::Zizmor => "zizmor",
        }
    }
    /// Whether another tool found it, rather than Orbit's own checks.
    pub fn tool(self) -> bool {
        matches!(self, RiskKind::Actionlint | RiskKind::Zizmor)
    }
}

/// One risky setting in a workflow file. Its text is the file's words,
/// shown only as code.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Risk {
    /// Orbit's number for it within its problem, never reused there; 0
    /// until it has one.
    #[serde(skip_serializing_if = "is_zero")]
    pub number: u32,
    pub kind: RiskKind,
    pub severity: Severity,
    /// Such as `.github/workflows/ci.yml`, and the line, from 1.
    pub path: String,
    pub line: usize,
    /// The job's ID, or empty for the whole workflow.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub job: String,
    /// What it is about: the action as written, such as
    /// `codecov/codecov-action@v4`; `missing` or `write-all` for
    /// permissions; what a checkout takes, such as
    /// `github.event.pull_request.head.sha`; or the text put in a script,
    /// such as `github.event.issue.title`. For a tool's, the rule, such as
    /// `artipacked` or `expression`.
    pub subject: String,
    /// For a tool's, what it says, such as "credential persistence through
    /// GitHub Actions artifacts".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// The commit Orbit first saw it at.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub since: String,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
impl Risk {
    /// Whether it is the same risk as `other`, wherever it moved to in its
    /// file.
    pub fn same(&self, other: &Risk) -> bool {
        self.kind == other.kind
            && self.path == other.path
            && self.job == other.job
            && self.subject == other.subject
            && self.detail == other.detail
    }
    /// The file's name, without its folder.
    pub fn file(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
    /// What it is, in a few plain words, for a timeline or Bit: "codecov/
    /// codecov-action@v4 isn't pinned, in ci.yml".
    pub fn name(&self) -> String {
        let file = self.file();
        let job = if self.job.is_empty() {
            String::new()
        } else {
            format!(", job {}", self.job)
        };
        match (self.kind, self.subject.as_str()) {
            (RiskKind::Unpinned, s) => format!("{s} isn't pinned, in {file}"),
            (RiskKind::Permissions, "missing") => format!("No permissions set, in {file}"),
            (RiskKind::Permissions, s) => format!("{s} permissions, in {file}{job}"),
            (RiskKind::PullCheckout, _) => format!("Pull request code checked out, in {file}{job}"),
            (RiskKind::Injection, s) => format!("{s} in a script, in {file}{job}"),
            (RiskKind::Actionlint | RiskKind::Zizmor, _) => {
                format!("{}: {}, in {file}{job}", self.kind.key(), self.detail)
            }
        }
    }
    /// What to change, in a sentence an agent can act on.
    pub fn advice(&self) -> String {
        let advice = match (self.kind, self.subject.as_str()) {
            (RiskKind::Unpinned, s) if s.starts_with("docker://") => {
                "pin the image to its digest (`@sha256:` and the full digest)"
            }
            (RiskKind::Unpinned, _) => {
                "pin it to a full commit (40 characters), with the version in a comment after it"
            }
            (RiskKind::Permissions, "missing") => {
                "add `permissions: contents: read` at the top, and give each job only what it needs"
            }
            (RiskKind::Permissions, _) => {
                "replace `write-all` with only the permissions this needs"
            }
            (RiskKind::PullCheckout, _) => {
                "don't check out the pull request's code here; run it in a `pull_request` workflow, which gets no secrets from a fork"
            }
            (RiskKind::Injection, _) => {
                "pass it in through `env:` and use the variable in the script, quoted"
            }
            (RiskKind::Actionlint, _) => {
                "fix what actionlint says; its checks are explained at https://github.com/rhysd/actionlint/blob/main/docs/checks.md"
            }
            // `crate::lint` keeps only a rule of letters, digits and `-`.
            (RiskKind::Zizmor, rule) => {
                return format!(
                    "fix what zizmor says, as explained at https://docs.zizmor.sh/audits/#{rule}"
                )
            }
        };
        advice.into()
    }
    fn severity_of(kind: RiskKind, subject: &str) -> Severity {
        match (kind, subject) {
            (RiskKind::Permissions, "missing") => Severity::Low,
            (RiskKind::Unpinned | RiskKind::Permissions, _) => Severity::Medium,
            (RiskKind::PullCheckout | RiskKind::Injection, _) => Severity::High,
            // A tool's severity is its own (`crate::lint`).
            (RiskKind::Actionlint | RiskKind::Zizmor, _) => Severity::Low,
        }
    }
}
/// The most severe first, then by file and line.
pub fn sort(risks: &mut [Risk]) {
    risks.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.number.cmp(&b.number))
    });
}
/// How many risks there are, and how many of them are high, to go in a
/// sentence: "3 risky settings" and ", 1 of them high".
pub fn count(risks: &[Risk]) -> (String, String) {
    let n = risks.len();
    let high = risks
        .iter()
        .filter(|r| r.severity >= Severity::High)
        .count();
    let many = if n == 1 {
        "1 risky setting".to_string()
    } else {
        format!("{n} risky settings")
    };
    let high = match (n, high) {
        (_, 0) => String::new(),
        (1, _) => ", and it is high".to_string(),
        (_, h) => format!(", {h} of them high"),
    };
    (many, high)
}
/// Risks in a sentence: "A, B and 2 more".
pub fn names(risks: &[Risk]) -> String {
    let all: Vec<String> = risks.iter().map(Risk::name).collect();
    match all.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}

/// What Orbit found in one workflow file: the most severe `MAX_PER_FILE`
/// risks, and how many more there are.
#[derive(Debug, Default)]
pub struct Checked {
    pub risks: Vec<Risk>,
    pub more: u32,
}

/// The risky settings in one workflow file, at `path`, in a repository
/// owned by `owner`, whose own actions count as its own rather than a
/// third party's. An error says why Orbit couldn't check it.
pub fn check(path: &str, text: &str, owner: &str) -> Result<Checked, String> {
    if text.len() > MAX_TEXT {
        return Err("it's too big to check".into());
    }
    let Some(root) = load(text)? else {
        return Ok(Checked::default());
    };
    let mut found = Found {
        path,
        risks: vec![],
        seen: HashSet::new(),
    };
    // Anything else isn't a workflow GitHub runs.
    if root.map().is_none() {
        return Ok(Checked::default());
    }
    let events = events(root.get("on"));
    let jobs: Vec<(&str, &Node)> = root
        .get("jobs")
        .and_then(Node::map)
        .into_iter()
        .flatten()
        .filter(|(_, _, job)| job.map().is_some())
        .map(|(id, _, job)| (id.as_str(), job.as_ref()))
        .collect();
    permissions(&mut found, &root, &events, &jobs);
    let exposed = events
        .iter()
        .any(|e| e == "pull_request_target" || e == "workflow_run");
    for (id, job) in &jobs {
        if let Some((uses, line)) = job.get("uses").and_then(|u| u.text_at()) {
            unpinned(&mut found, uses, line, owner);
        }
        let steps = job.get("steps").and_then(Node::list).into_iter().flatten();
        for step in steps {
            let uses = step.get("uses").and_then(|u| u.text_at());
            if let Some((uses, line)) = uses {
                unpinned(&mut found, uses, line, owner);
            }
            let action = uses.map(|(u, _)| u.trim().to_ascii_lowercase());
            let action = action.as_deref().unwrap_or_default();
            let with = |key: &str| step.get("with").and_then(|w| w.get(key));
            if exposed && action.starts_with("actions/checkout@") {
                for key in ["ref", "repository"] {
                    let Some((value, line)) = with(key).and_then(Node::text_at) else {
                        continue;
                    };
                    if let Some(head) = pull_head(value) {
                        found.add(RiskKind::PullCheckout, line, id, head);
                    }
                }
            }
            let run = step.get("run");
            if let Some((script, _)) = run.and_then(Node::text_at) {
                if exposed {
                    if let Some(head) = run_checkout(script) {
                        let line = run.map_or(0, |r| r.line);
                        found.add(RiskKind::PullCheckout, line, id, head);
                    }
                }
                injection(&mut found, run, id);
            }
            if action.starts_with("actions/github-script@") {
                injection(&mut found, with("script"), id);
            }
        }
    }
    let mut risks = found.risks;
    sort(&mut risks);
    risks.truncate(MAX_PER_FILE);
    let more = found.seen.len().saturating_sub(risks.len());
    Ok(Checked {
        risks,
        more: u32::try_from(more).unwrap_or(u32::MAX),
    })
}

/// What a file's checks found so far, each risk once, where it first is:
/// the most severe of them, and what makes each one found the same one
/// (see `Risk::same`), hashed.
struct Found<'a> {
    path: &'a str,
    risks: Vec<Risk>,
    seen: HashSet<u64>,
}
impl Found<'_> {
    fn add(&mut self, kind: RiskKind, line: usize, job: &str, subject: &str) {
        let job = shorten(job, MAX_SUBJECT);
        let subject = shorten(subject, MAX_SUBJECT);
        let mut same = DefaultHasher::new();
        (kind, &job, &subject).hash(&mut same);
        if !self.seen.insert(same.finish()) {
            return;
        }
        self.risks.push(Risk {
            kind,
            severity: Risk::severity_of(kind, &subject),
            path: self.path.to_string(),
            line,
            job,
            subject,
            ..Default::default()
        });
        // Only the most severe are kept, so a file with thousands holds
        // few at a time, and one found late can still be among them.
        if self.risks.len() >= 4 * MAX_PER_FILE {
            sort(&mut self.risks);
            self.risks.truncate(MAX_PER_FILE);
        }
    }
}

/// The events a workflow's `on:` names.
fn events(on: Option<&Node>) -> Vec<String> {
    let Some(on) = on else { return vec![] };
    match &on.value {
        Value::Text(t) => vec![t.trim().to_string()],
        Value::List(items) => items
            .iter()
            .filter_map(|i| i.text())
            .map(|t| t.trim().to_string())
            .collect(),
        Value::Map(entries) => entries.iter().map(|(k, _, _)| k.clone()).collect(),
    }
}

/// No `permissions` at the top while a job has none either, so those jobs
/// get the token's default, or `write-all` anywhere. A workflow only other
/// workflows call gets what its caller gives it, so it needs none.
fn permissions(found: &mut Found, root: &Node, events: &[String], jobs: &[(&str, &Node)]) {
    let write_all = |n: &Node| n.text().is_some_and(|t| t.trim() == "write-all");
    match root.entry("permissions") {
        Some((line, p)) if write_all(p) => found.add(RiskKind::Permissions, line, "", "write-all"),
        Some(_) => {}
        None => {
            let called = !events.is_empty() && events.iter().all(|e| e == "workflow_call");
            let bare = jobs.iter().any(|(_, job)| job.get("permissions").is_none());
            if bare && !called {
                let line = root.entry("jobs").map_or(1, |(line, _)| line);
                found.add(RiskKind::Permissions, line, "", "missing");
            }
        }
    }
    for (id, job) in jobs {
        if let Some((line, _)) = job.entry("permissions").filter(|(_, p)| write_all(p)) {
            found.add(RiskKind::Permissions, line, id, "write-all");
        }
    }
}

/// An action, reusable workflow or Docker image someone else owns that
/// isn't pinned to a full commit or digest. GitHub's own (`actions/`,
/// `github/`), the repository owner's, and those in the repository itself
/// are left out.
fn unpinned(found: &mut Found, uses: &str, line: usize, owner: &str) {
    let uses = uses.trim();
    if uses.starts_with("./") || uses.is_empty() {
        return;
    }
    let hex = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_hexdigit());
    if let Some(image) = uses.strip_prefix("docker://") {
        let digest = image.split_once("@sha256:").map(|(_, d)| d);
        if !digest.is_some_and(|d| hex(d, 64)) {
            found.add(RiskKind::Unpinned, line, "", uses);
        }
        return;
    }
    // GitHub refuses one with no version, so it never runs.
    let Some((name, version)) = uses.rsplit_once('@') else {
        return;
    };
    let who = name.split('/').next().unwrap_or_default();
    let theirs = ["actions", "github", owner]
        .iter()
        .any(|o| !o.is_empty() && who.eq_ignore_ascii_case(o));
    if !theirs && !hex(version, 40) {
        found.add(RiskKind::Unpinned, line, "", uses);
    }
}

/// What in a checkout's `ref` or `repository` names a pull request's code.
fn pull_head(value: &str) -> Option<&'static str> {
    if value.contains("refs/pull/") {
        return Some("refs/pull/");
    }
    expressions(value).find_map(|e| PULL_HEADS.iter().find(|p| mentions(&e, p, false)).copied())
}
/// A script that checks out a pull request's code: `gh pr checkout`, or
/// git fetching or checking out one of `PULL_HEADS` or `refs/pull/`.
fn run_checkout(script: &str) -> Option<&'static str> {
    if script.contains("gh pr checkout") {
        return Some("gh pr checkout");
    }
    let git = ["git fetch", "git checkout", "git switch", "git pull"];
    git.iter()
        .any(|g| script.contains(g))
        .then(|| pull_head(script))
        .flatten()
}
/// What names a pull request's own commit, branch or repository, once
/// normalized (see `normalize`).
const PULL_HEADS: [&str; 10] = [
    "github.event.pull_request.head.sha",
    "github.event.pull_request.head.ref",
    "github.event.pull_request.head.repo.full_name",
    "github.event.pull_request.merge_commit_sha",
    "github.head_ref",
    "github.event.workflow_run.head_sha",
    "github.event.workflow_run.head_branch",
    "github.event.workflow_run.head_repository.full_name",
    "github.event.workflow_run.pull_requests.head.sha",
    "github.event.workflow_run.pull_requests.head.ref",
];
/// Text anyone who can open an issue, a pull request, a branch or a
/// comment can write, as in `PULL_HEADS`.
const UNTRUSTED: [&str; 25] = [
    "github.event.issue.title",
    "github.event.issue.body",
    "github.event.pull_request.title",
    "github.event.pull_request.body",
    "github.event.comment.body",
    "github.event.review.body",
    "github.event.review_comment.body",
    "github.event.discussion.title",
    "github.event.discussion.body",
    "github.event.pages.page_name",
    "github.event.commits.message",
    "github.event.commits.author.email",
    "github.event.commits.author.name",
    "github.event.head_commit.message",
    "github.event.head_commit.author.email",
    "github.event.head_commit.author.name",
    "github.event.pull_request.head.ref",
    "github.event.pull_request.head.label",
    "github.event.pull_request.head.repo.default_branch",
    "github.head_ref",
    "github.event.workflow_run.head_branch",
    "github.event.workflow_run.head_commit.message",
    "github.event.workflow_run.head_commit.author.email",
    "github.event.workflow_run.head_commit.author.name",
    "github.event.workflow_run.display_title",
];
/// Whole objects that hold such text, such as `toJSON(github.event)`.
const UNTRUSTED_WHOLE: [&str; 11] = [
    "github",
    "github.event",
    "github.event.issue",
    "github.event.pull_request",
    "github.event.comment",
    "github.event.review",
    "github.event.review_comment",
    "github.event.discussion",
    "github.event.commits",
    "github.event.head_commit",
    "github.event.workflow_run",
];
/// Each `${{ }}` in a script or `actions/github-script`'s `script` that
/// holds text anyone can write.
fn injection(found: &mut Found, node: Option<&Node>, job: &str) {
    let Some((script, start)) = node.and_then(Node::text_at) else {
        return;
    };
    // A `|` or `>` block's line is where its text starts. Lines a `>`
    // block folds count as one, so a line after them reads a little early.
    let mut from = 0;
    while let Some(open) = script[from..].find("${{").map(|i| i + from) {
        let close = script[open..].find("}}").map_or(script.len(), |i| i + open);
        let e = normalize(&script[open + 3..close]);
        let hit = UNTRUSTED
            .iter()
            .find(|p| mentions(&e, p, false))
            .or_else(|| UNTRUSTED_WHOLE.iter().find(|p| mentions(&e, p, true)));
        if let Some(hit) = hit {
            let lines = script[..open].matches('\n').count();
            let line = start + lines;
            found.add(RiskKind::Injection, line, job, hit);
        }
        from = (close + 2).min(script.len());
    }
}
/// Each `${{ }}` in some text, normalized.
fn expressions(text: &str) -> impl Iterator<Item = String> + '_ {
    let mut from = 0;
    std::iter::from_fn(move || {
        let open = text[from..].find("${{")? + from;
        let close = text[open..].find("}}").map_or(text.len(), |i| i + open);
        from = (close + 2).min(text.len());
        Some(normalize(&text[open + 3..close]))
    })
}
/// An expression in lower case with no spaces, a quoted name in brackets
/// read as a property and other indexes taken out: `github['event'].issue`
/// reads `github.event.issue`, and `github.event.commits[0].message` and
/// `github.event.commits.*.message` read `github.event.commits.message`.
pub(crate) fn normalize(expression: &str) -> String {
    let mut out = String::new();
    let mut chars = expression.chars();
    while let Some(c) = chars.next() {
        match c {
            '[' => {
                // To the bracket that closes this one, past quoted text.
                let mut inside = String::new();
                let (mut depth, mut quote) = (1usize, None);
                for c in chars.by_ref() {
                    match (quote, c) {
                        (Some(q), c) if c == q => quote = None,
                        (Some(_), _) => {}
                        (None, '\'' | '"') => quote = Some(c),
                        (None, '[') => depth += 1,
                        (None, ']') if depth == 1 => break,
                        (None, ']') => depth -= 1,
                        _ => {}
                    }
                    inside.push(c);
                }
                if let Some(name) = quoted(inside.trim()) {
                    out.push('.');
                    let name = name.chars().filter(|c| !c.is_whitespace());
                    out.extend(name.map(|c| c.to_ascii_lowercase()));
                }
            }
            c if c.is_whitespace() => {}
            c => out.push(c.to_ascii_lowercase()),
        }
    }
    out.replace(".*", "")
}
/// The text in a quoted string such as `'issue'`, or nothing if it is
/// anything else.
fn quoted(text: &str) -> Option<&str> {
    let q = text.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inside = text.strip_prefix(q)?.strip_suffix(q)?;
    (!inside.contains(q)).then_some(inside)
}
/// Whether an expression names `path`, not as part of a longer name; with
/// `whole`, only the object itself, not something in it.
fn mentions(expression: &str, path: &str, whole: bool) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut from = 0;
    while let Some(at) = expression[from..].find(path).map(|i| i + from) {
        let before = expression[..at].chars().next_back();
        let after = expression[at + path.len()..].chars().next();
        let starts = before.is_none_or(|c| !word(c) && c != '.');
        let ends = after.is_none_or(|c| !word(c) && !(whole && c == '.'));
        if starts && ends {
            return true;
        }
        from = at + 1;
    }
    false
}

/// A YAML node and the line it starts on, from 1.
#[derive(Debug)]
struct Node {
    line: usize,
    value: Value,
}
#[derive(Debug)]
enum Value {
    Text(String),
    List(Vec<Rc<Node>>),
    /// Each key, the line it is on, and its value. A key that isn't text
    /// reads as empty.
    Map(Vec<(String, usize, Rc<Node>)>),
}
impl Node {
    fn text(&self) -> Option<&str> {
        match &self.value {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }
    fn text_at(&self) -> Option<(&str, usize)> {
        self.text().map(|t| (t, self.line))
    }
    fn list(&self) -> Option<&[Rc<Node>]> {
        match &self.value {
            Value::List(items) => Some(items),
            _ => None,
        }
    }
    fn map(&self) -> Option<&[(String, usize, Rc<Node>)]> {
        match &self.value {
            Value::Map(entries) => Some(entries),
            _ => None,
        }
    }
    /// A key's line and value; GitHub refuses a key twice, so the first.
    fn entry(&self, key: &str) -> Option<(usize, &Node)> {
        self.map()?
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, line, v)| (*line, v.as_ref()))
    }
    fn get(&self, key: &str) -> Option<&Node> {
        self.entry(key).map(|(_, v)| v)
    }
}

/// A list or map being read: its line and anchor, and what it holds so
/// far with its weight.
enum Open {
    List(usize, usize, Vec<Rc<Node>>, usize),
    Map(
        usize,
        usize,
        Vec<(String, usize, Rc<Node>)>,
        Option<(String, usize)>,
        usize,
    ),
}
/// Reads the first document in a file, or none when it's empty. An alias
/// shares what it points to, and weighs all of it each time, so no file
/// grows past `MAX_WEIGHT` however its aliases nest, and checking it
/// stays quick.
fn load(text: &str) -> Result<Option<Rc<Node>>, String> {
    let mut parser = Parser::new_from_str(text);
    let mut open: Vec<Open> = vec![];
    // Each anchor's node and weight, by the parser's number for it.
    let mut anchors: HashMap<usize, (Rc<Node>, usize)> = HashMap::new();
    let too_big = || "it grows too big to check once its aliases are expanded".to_string();
    loop {
        let (event, mark) = parser
            .next_token()
            .map_err(|e| format!("it isn't valid YAML ({e})"))?;
        let line = mark.line();
        let (node, weight, anchor) = match event {
            Event::StreamEnd | Event::DocumentEnd => return Ok(None),
            Event::Scalar(text, style, anchor, _) => {
                // `~` or `null` is no value, as is nothing at all.
                let plain = style == TScalarStyle::Plain;
                let text = if plain && matches!(text.as_str(), "~" | "null" | "Null" | "NULL") {
                    String::new()
                } else {
                    text
                };
                let weight = 1 + text.len() / 64;
                let value = Value::Text(text);
                (Rc::new(Node { line, value }), weight, anchor)
            }
            Event::Alias(id) => {
                let (node, weight) = anchors
                    .get(&id)
                    .cloned()
                    .ok_or("it uses an alias with no anchor")?;
                (node, weight, 0)
            }
            Event::SequenceStart(anchor, _) => {
                if open.len() >= MAX_DEPTH {
                    return Err("it nests too deeply to check".into());
                }
                open.push(Open::List(line, anchor, vec![], 1));
                continue;
            }
            Event::MappingStart(anchor, _) => {
                if open.len() >= MAX_DEPTH {
                    return Err("it nests too deeply to check".into());
                }
                open.push(Open::Map(line, anchor, vec![], None, 1));
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let (line, anchor, value, weight) = match open.pop() {
                    Some(Open::List(line, anchor, items, weight)) => {
                        (line, anchor, Value::List(items), weight)
                    }
                    Some(Open::Map(line, anchor, entries, _, weight)) => {
                        (line, anchor, Value::Map(entries), weight)
                    }
                    None => return Err("it isn't valid YAML".into()),
                };
                (Rc::new(Node { line, value }), weight, anchor)
            }
            _ => continue,
        };
        if weight > MAX_WEIGHT {
            return Err(too_big());
        }
        if anchor > 0 {
            anchors.insert(anchor, (node.clone(), weight));
        }
        let add = |sum: &mut usize| {
            *sum = sum.saturating_add(weight);
            (*sum <= MAX_WEIGHT).then_some(()).ok_or_else(too_big)
        };
        match open.last_mut() {
            None => return Ok(Some(node)),
            Some(Open::List(_, _, items, sum)) => {
                add(sum)?;
                items.push(node);
            }
            Some(Open::Map(_, _, entries, key, sum)) => {
                add(sum)?;
                match key.take() {
                    None => *key = Some((node.text().unwrap_or_default().to_string(), node.line)),
                    Some((k, line)) => entries.push((k, line, node)),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "risks_tests.rs"]
mod tests;
