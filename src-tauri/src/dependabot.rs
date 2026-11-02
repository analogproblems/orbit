//! Dependabot's pull requests: what each one updates, from which version
//! to which, the open Dependabot alerts it fixes, how its checks are doing,
//! and whether GitHub says it can merge. Fixes for the most severe alerts
//! come first, and fixes for high or critical ones are in the morning brief
//! (`crate::brief`). Orbit never merges them: it says which are ready.
//!
//! They are the open pull requests Orbit syncs that Dependabot made from a
//! branch in the repository itself. Their checks are what Orbit read of CI
//! (`crate::ci`). Whether one merges is GitHub's answer about that one pull
//! request (`read`), which Orbit asks a few a pass: once its head or its
//! base branch's latest commit moves, again a little later while GitHub is
//! still working it out, and otherwise every `readiness::EVERY`. An update
//! fixes an alert when it changes the alert's package, in the alert's
//! folder, to a version outside the alert's vulnerable range. All of it
//! only while Watch CI and Watch security alerts are both on.
use crate::{
    ci::{short, CiState},
    readiness::{self, Merge},
    security::{Alert, Severity, Source},
    types::*,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, sync::LazyLock};

/// Pull requests Orbit asks GitHub about in one pass.
pub const PER_PASS: usize = 4;
/// Packages an update's sentence names before "and N more".
const NAMED: usize = 3;
/// Packages Orbit reads from one description, which is someone else's
/// words and could be long.
const MAX_BUMPS: usize = 50;
/// The longest package name or version Orbit shows.
const MAX_WORD: usize = 80;

/// One package an update changes, as its title or description says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bump {
    pub name: String,
    pub from: String,
    pub to: String,
}
/// How big a change is, by the first part of its version that changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Patch,
    Minor,
    Major,
}
impl Change {
    pub fn word(self) -> &'static str {
        match self {
            Change::Patch => "patch",
            Change::Minor => "minor",
            Change::Major => "major",
        }
    }
}
/// What an update's title and description say it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub bumps: Vec<Bump>,
    /// The group it updates together, such as `npm_and_yarn`, if any.
    pub group: String,
    /// How many packages its title says it updates, or 0.
    pub count: usize,
    /// Its folder, without slashes and empty for the top one, or None when
    /// Orbit can't tell which one, as for a group across several: then it
    /// claims no fix.
    pub folder: Option<String>,
}

static GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bbump\s+the\s+(\S+)\s+group\b(?:\s+across\s+(\d+)\s+director(?:y|ies))?(?:\s+in\s+(\S+))?(?:\s+with\s+(\d+)\s+updates?)?",
    )
    .unwrap()
});
static SINGLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:bump|update)\s+(\S+)\s+(?:requirement\s+)?from\s+(\S+)\s+to\s+(\S+)(?:\s+in\s+(\S+))?",
    )
    .unwrap()
});
static LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^\s*updates\s+`([^`\n]+)`\s+from\s+(\S+)\s+to\s+(\S+)").unwrap()
});
static FOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bin the (\S+) directory\b").unwrap());

/// A word from a title or description, without the punctuation around it.
fn word(text: &str) -> String {
    let text = text.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '(' | ')' | '`'));
    crate::text::shorten(text, MAX_WORD)
}
fn folder(text: &str) -> String {
    word(text).trim_matches('/').to_string()
}

/// What Dependabot's title and description say an update changes:
/// "Bump lodash from 4.17.20 to 4.17.21 in /web", "Update lodash
/// requirement from ~4.17.20 to ~4.17.21", or "Bump the npm_and_yarn group
/// across 1 directory with 2 updates" with a line per package in its
/// description ("Updates `lodash` from 4.17.20 to 4.17.21"), with or
/// without a prefix such as "chore(deps): ".
pub fn parse(title: &str, body: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut across = None;
    if let Some(c) = GROUP.captures(title) {
        p.group = word(&c[1]);
        across = c.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
        p.folder = c.get(3).map(|m| folder(m.as_str()));
        p.count = c.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
    } else if let Some(c) = SINGLE.captures(title) {
        p.bumps.push(Bump {
            name: word(&c[1]),
            from: word(&c[2]),
            to: word(&c[3]),
        });
        p.folder = Some(c.get(4).map_or(String::new(), |m| folder(m.as_str())));
    }
    if p.bumps.is_empty() {
        for c in LINE.captures_iter(body) {
            let b = Bump {
                name: word(&c[1]),
                from: word(&c[2]),
                to: word(&c[3]),
            };
            if !p.bumps.contains(&b) {
                p.bumps.push(b);
            }
            if p.bumps.len() == MAX_BUMPS {
                break;
            }
        }
    }
    if p.folder.is_none() && across.is_none_or(|n| n == 1) {
        let mut named: Vec<String> = vec![];
        for c in FOLDER.captures_iter(body) {
            let f = folder(&c[1]);
            if !named.contains(&f) {
                named.push(f);
            }
        }
        p.folder = match named.as_slice() {
            [one] => Some(one.clone()),
            // A title that names no folder, and isn't across any, is about
            // the top one.
            [] if across.is_none() => Some(String::new()),
            _ => None,
        };
    }
    p
}

/// A version's numbers, as in `4.17.21` or `v1.2`: None for anything else,
/// such as `1.0.0-beta` or `~> 1.2`.
fn numbers(version: &str) -> Option<Vec<u64>> {
    let v = version.strip_prefix('v').unwrap_or(version);
    if v.is_empty() {
        return None;
    }
    v.split('.').map(|p| p.parse().ok()).collect()
}
fn compare(a: &[u64], b: &[u64]) -> Ordering {
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..a.len().max(b.len()))
        .map(|i| at(a, i).cmp(&at(b, i)))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}
/// How big the change from one version to another is, when both are plain
/// versions, a requirement's `~` or `^` aside.
pub fn change(from: &str, to: &str) -> Option<Change> {
    let loose = |v: &str| numbers(v.trim_start_matches(['~', '^', '=', ' ']));
    let (a, b) = (loose(from)?, loose(to)?);
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    let first = (0..a.len().max(b.len())).find(|&i| at(&a, i) != at(&b, i))?;
    Some(match first {
        0 => Change::Major,
        1 => Change::Minor,
        _ => Change::Patch,
    })
}
/// Whether `version` is in a vulnerable range as GitHub writes one, such
/// as "< 4.17.21" or ">= 2.0.0, < 2.0.5". None when Orbit can't tell.
pub fn in_range(range: &str, version: &str) -> Option<bool> {
    let version = numbers(version)?;
    let mut inside = true;
    for part in range.split(',') {
        let part = part.trim();
        let (op, rest) = ["<=", ">=", "<", ">", "="]
            .iter()
            .find_map(|op| part.strip_prefix(op).map(|rest| (*op, rest)))?;
        let order = compare(&version, &numbers(rest.trim())?);
        inside &= match op {
            "<" => order.is_lt(),
            "<=" => order.is_le(),
            ">" => order.is_gt(),
            ">=" => order.is_ge(),
            _ => order.is_eq(),
        };
    }
    Some(inside)
}
/// The folder a file is in, without slashes: "" for the top one.
fn folder_of(path: &str) -> &str {
    path.trim_matches('/')
        .rsplit_once('/')
        .map_or("", |(folder, _)| folder)
}
/// Whether an update to `b`, in `folder` (None when Orbit can't tell
/// which), fixes an open Dependabot alert: the same package, in the
/// alert's folder, to a version outside every range its advisory affects.
/// Where Orbit can't tell the folder, nothing is claimed; where it can't
/// read every range, only the version that fixes it counts.
pub fn fixes(a: &Alert, b: &Bump, folder: Option<&str>) -> bool {
    if a.source != Source::Dependabot || !a.subject.eq_ignore_ascii_case(&b.name) {
        return false;
    }
    let Some(folder) = folder else {
        return false;
    };
    if !a.path.is_empty() && folder_of(&a.path) != folder {
        return false;
    }
    let exact = !a.fixed_in.is_empty() && a.fixed_in == b.to;
    let outside: Option<Vec<bool>> = a
        .ranges
        .iter()
        .map(|r| in_range(r, &b.to).map(|inside| !inside))
        .collect();
    match outside {
        Some(outside) if !outside.is_empty() => outside.iter().all(|&o| o),
        _ => exact,
    }
}

/// Whether Dependabot made a pull request, from a branch in the repository
/// itself.
pub fn made_by_dependabot(pr: &PullRequest) -> bool {
    pr.author.eq_ignore_ascii_case("dependabot[bot]") && same_repo(&pr.head_repo, &pr.repo)
}
/// Whether Orbit reads and shows Dependabot's pull requests now.
pub fn watched(s: &Snapshot) -> bool {
    s.settings.watch_ci && s.settings.watch_security
}
/// Dependabot's open pull requests in connected repositories, while
/// watched.
fn pulls(s: &Snapshot) -> impl Iterator<Item = &PullRequest> {
    let on = watched(s);
    s.prs.iter().filter(move |pr| {
        on && !pr.draft
            && made_by_dependabot(pr)
            && s.settings
                .repositories
                .iter()
                .any(|r| same_repo(&r.name, &pr.repo))
    })
}
/// The commit of its base branch Orbit asks GitHub about: the latest one
/// Orbit read of CI for the default branch, or what the sync said for
/// another branch, or while Orbit's latest read of CI failed, as what it
/// kept from before may be older than the sync.
fn base_of(s: &Snapshot, pr: &PullRequest) -> String {
    ci_of(s, pr)
        .filter(|c| c.error.is_none() && c.branch == pr.base_ref && !c.head.sha.is_empty())
        .map_or_else(|| pr.base_sha.clone(), |c| c.head.sha.clone())
}
fn ci_of<'a>(s: &'a Snapshot, pr: &PullRequest) -> Option<&'a crate::ci::RepoCi> {
    s.ci.iter().find(|c| same_repo(&c.repo, &pr.repo))
}
fn asked<'a>(s: &'a Snapshot, pr: &PullRequest, base: &str) -> Option<&'a Merge> {
    s.dependabot
        .iter()
        .find(|m| m.pr_id == pr.id && m.head_sha == pr.head_sha && m.base_sha == base)
}

/// The pull requests to ask GitHub about now, at most `PER_PASS`, by
/// repository, number, head and base: those not asked about at their head
/// and base first, then those asked longest ago, once their wait is over.
pub fn wanted(s: &Snapshot, now: DateTime<Utc>) -> Vec<(String, u64, String, String)> {
    let mut due: Vec<(String, (String, u64, String, String))> = pulls(s)
        .filter_map(|pr| {
            let base = base_of(s, pr);
            let ask = (
                pr.repo.clone(),
                pr.number,
                pr.head_sha.clone(),
                base.clone(),
            );
            let Some(m) = asked(s, pr, &base) else {
                return Some((String::new(), ask));
            };
            let time = crate::problems::time;
            let ready = match m.retry_at.as_deref().and_then(time) {
                Some(retry) => now >= retry,
                None => time(&m.read_at).is_none_or(|at| now - at >= readiness::EVERY),
            };
            ready.then(|| (m.read_at.clone(), ask))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter().take(PER_PASS).map(|(_, ask)| ask).collect()
}
/// Asks GitHub whether one pull request can merge. An answer about another
/// head or base commit is an error, as it moved since Orbit last synced.
pub async fn read(repo: &str, number: u64, head: &str, base: &str) -> Result<Merge, String> {
    validate_repo(repo)?;
    let v = crate::github::api(&format!("repos/{repo}/pulls/{number}"), "GET", &[], None).await?;
    readiness::understand(&v, &pr_id(repo, number), head, base, Utc::now())
}
/// Records what GitHub said, while Orbit still shows that pull request at
/// that head, and forgets what it no longer shows.
pub fn record(
    s: &mut Snapshot,
    repo: &str,
    number: u64,
    head: &str,
    base: &str,
    result: Result<Merge, String>,
    now: DateTime<Utc>,
) {
    let id = pr_id(repo, number);
    if pulls(s).any(|pr| pr.id == id && pr.head_sha == head) {
        readiness::record(&mut s.dependabot, &id, head, base, result, now);
    }
    prune(s);
}
/// Forgets GitHub's answers about pull requests Orbit no longer shows.
/// Returns whether any went.
pub fn prune(s: &mut Snapshot) -> bool {
    let kept: Vec<String> = pulls(s).map(|pr| pr.id.clone()).collect();
    let before = s.dependabot.len();
    s.dependabot.retain(|m| kept.contains(&m.pr_id));
    s.dependabot.len() != before
}

/// An open Dependabot alert an update fixes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fix {
    pub number: u64,
    pub severity: Severity,
    /// The package, and with its severity: "lodash (critical)".
    pub package: String,
    pub name: String,
    pub url: String,
}
/// One of Dependabot's pull requests, as Operations and the brief show it.
/// Derived like `crate::readiness::Readiness`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Update {
    pub pr_id: String,
    pub repo: String,
    pub number: u64,
    /// Its title, in Dependabot's words, and its page on GitHub.
    pub title: String,
    pub url: String,
    pub head_sha: String,
    /// What it updates, in a sentence.
    pub what: String,
    /// The biggest change among its packages, when Orbit can tell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
    /// The open Dependabot alerts it fixes, the most severe first.
    pub fixes: Vec<Fix>,
    pub checks: CiState,
    /// How its checks on its latest commit are doing, in a sentence.
    pub checks_text: String,
    /// GitHub says it conflicts with its base branch.
    pub conflicts: bool,
    /// Whether GitHub says it can merge, in a sentence.
    pub merge_text: String,
    /// Every check on its latest commit passed, GitHub says it can merge,
    /// and Orbit's latest read of CI worked.
    pub ready: bool,
    /// Why Orbit's latest question about it failed, or else its latest
    /// read of CI, when one did. What GitHub said before stands meanwhile,
    /// but after a failed read of CI it isn't ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Update {
    /// The most severe alert it fixes, if any.
    pub fn severity(&self) -> Option<Severity> {
        self.fixes.iter().map(|f| f.severity).max()
    }
}

fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
/// What an update changes, in a sentence: "Updates lodash from 4.17.20 to
/// 4.17.21, a patch update." or "Updates 2 packages in the npm_and_yarn
/// group: ….".
pub fn what_text(p: &Parsed) -> String {
    let place = match p.folder.as_deref() {
        Some("") | None => String::new(),
        Some(folder) => format!(" in /{folder}"),
    };
    let one = |b: &Bump| format!("{} from {} to {}", b.name, b.from, b.to);
    let biggest = p.bumps.iter().filter_map(|b| change(&b.from, &b.to)).max();
    match (p.bumps.as_slice(), p.group.is_empty()) {
        ([b], true) => {
            let size = biggest.map_or(String::new(), |c| format!(", a {} update", c.word()));
            format!("Updates {}{place}{size}.", one(b))
        }
        ([], true) => "Orbit can't tell from its title what it updates.".into(),
        (bumps, _) => {
            let n = p.count.max(bumps.len());
            let group = if p.group.is_empty() {
                String::new()
            } else {
                format!(" in the {} group", p.group)
            };
            let head = match n {
                0 => format!("Updates the packages{group}{place}"),
                1 => format!("Updates 1 package{group}{place}"),
                n => format!("Updates {n} packages{group}{place}"),
            };
            if bumps.is_empty() {
                return format!("{head}.");
            }
            let mut named: Vec<String> = bumps.iter().take(NAMED).map(one).collect();
            if n > NAMED {
                named.push(format!("{} more", n - NAMED));
            }
            match biggest {
                Some(c) if n == 1 => format!("{head}: {}, a {} update.", and(&named), c.word()),
                Some(c) => format!(
                    "{head}: {}. The biggest is a {} update.",
                    and(&named),
                    c.word()
                ),
                None => format!("{head}: {}.", and(&named)),
            }
        }
    }
}
/// How its checks are doing, from what Orbit read of CI, in a sentence.
fn checks_of(s: &Snapshot, pr: &PullRequest) -> (CiState, String) {
    let head = short(&pr.head_sha);
    let Some(pull) =
        s.ci.iter()
            .find(|c| same_repo(&c.repo, &pr.repo))
            .and_then(|c| c.pull(pr.number))
            .filter(|p| p.ci.sha == pr.head_sha)
    else {
        return (
            CiState::None,
            format!("Orbit hasn't read the checks on {head} yet."),
        );
    };
    let ci = &pull.ci;
    let text = match ci.state {
        CiState::Passing => match ci.passing {
            0 => "Its checks passed.".to_string(),
            1 => "Its one check passed.".to_string(),
            n => format!("All {n} of its checks passed."),
        },
        CiState::Failing => format!("{} failed on {head}.", crate::problems::names(&ci.failing)),
        CiState::Running => "Its checks are still running.".into(),
        CiState::None => "No checks ran on it.".into(),
    };
    (ci.state, text)
}
/// Whether GitHub says it can merge, in a sentence, whether it conflicts,
/// and whether it merges.
fn merge_of(m: Option<&Merge>, pr: &PullRequest) -> (String, bool, bool) {
    let base = &pr.base_ref;
    let Some(m) = m else {
        return (
            "Orbit hasn't asked GitHub yet whether it merges.".into(),
            false,
            false,
        );
    };
    if m.read_at.is_empty() {
        if let Some(error) = &m.error {
            return (error.clone(), false, false);
        }
    }
    if m.mergeable.is_none() || m.state == "unknown" {
        return (
            "GitHub is still checking whether it merges.".into(),
            false,
            false,
        );
    }
    if m.mergeable == Some(false) || m.state == "dirty" {
        return (format!("It conflicts with {base}."), true, false);
    }
    let text = match m.state.as_str() {
        "clean" | "has_hooks" => return ("GitHub says it can merge.".into(), false, true),
        "behind" => format!("GitHub wants it brought up to date with {base} first."),
        "blocked" => "GitHub says a rule blocks merging it, such as a required review.".into(),
        "unstable" => "GitHub says a check on it isn't passing.".into(),
        "draft" => "GitHub says it is a draft.".into(),
        other => format!("GitHub says it can't merge yet ({other})."),
    };
    (text, false, false)
}
/// The open Dependabot alerts an update fixes, from what Orbit last read of
/// its repository's alerts, the most severe first.
fn fixes_of(s: &Snapshot, pr: &PullRequest, p: &Parsed) -> Vec<Fix> {
    let alerts: Vec<&Alert> = s
        .security
        .iter()
        .filter(|r| same_repo(&r.repo, &pr.repo))
        .filter_map(|r| r.kind(Source::Dependabot))
        .filter(|k| k.unseen.is_none())
        .flat_map(|k| &k.alerts)
        .collect();
    let mut found: Vec<Fix> = alerts
        .into_iter()
        .filter(|a| p.bumps.iter().any(|b| fixes(a, b, p.folder.as_deref())))
        .map(|a| Fix {
            number: a.number,
            severity: a.severity,
            package: a.subject.clone(),
            name: a.name(),
            url: a.url.clone(),
        })
        .collect();
    found.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.number.cmp(&b.number)));
    found
}
/// One pull request's update, as shown.
pub fn of(s: &Snapshot, pr: &PullRequest) -> Update {
    let parsed = parse(&pr.title, &pr.body);
    let (checks, checks_text) = checks_of(s, pr);
    let merge = asked(s, pr, &base_of(s, pr));
    let (merge_text, conflicts, merges) = merge_of(merge, pr);
    let ci_failed = ci_of(s, pr).is_some_and(|c| c.error.is_some());
    let error = merge.and_then(|m| m.error.clone()).or_else(|| {
        ci_failed.then(|| {
            "Orbit's latest read of CI here failed, so its checks and base may be out of date."
                .to_string()
        })
    });
    Update {
        pr_id: pr.id.clone(),
        repo: pr.repo.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        head_sha: pr.head_sha.clone(),
        what: what_text(&parsed),
        change: parsed
            .bumps
            .iter()
            .filter_map(|b| change(&b.from, &b.to))
            .max(),
        fixes: fixes_of(s, pr, &parsed),
        checks,
        checks_text,
        conflicts,
        merge_text,
        ready: checks == CiState::Passing && merges && !ci_failed,
        error,
    }
}
/// Every one of Dependabot's open pull requests Orbit shows: fixes for the
/// most severe alerts first, then by repository and number.
pub fn all(s: &Snapshot) -> Vec<Update> {
    let mut all: Vec<Update> = pulls(s).map(|pr| of(s, pr)).collect();
    all.sort_by(|a, b| {
        b.severity()
            .cmp(&a.severity())
            .then_with(|| {
                a.repo
                    .to_ascii_lowercase()
                    .cmp(&b.repo.to_ascii_lowercase())
            })
            .then(a.number.cmp(&b.number))
    });
    all
}
/// The brief's line about a fix for a high or critical alert: ready, or
/// what is in the way first.
pub fn brief_text(u: &Update) -> String {
    let names: Vec<String> = u.fixes.iter().map(|f| f.name.clone()).collect();
    let mut text = format!(
        "Dependabot's fix for {} in {}#{}",
        and(&names),
        u.repo,
        u.number
    );
    if u.ready {
        text.push_str(" is ready to merge.");
    } else {
        let first = if u.checks == CiState::Passing {
            &u.merge_text
        } else {
            &u.checks_text
        };
        text.push_str(&format!(" isn't ready yet. {first}"));
    }
    if u.error.is_some() {
        text.push_str(" Orbit's latest question about it failed, so this may be out of date.");
    }
    text
}

#[cfg(test)]
#[path = "dependabot_tests.rs"]
pub(crate) mod tests;
