//! Release dates: a GitHub milestone with a due date, named for a version
//! such as `v0.3.0` or `Release 0.3.0`, is the day that release is due.
//! Orbit reads each connected repository's open milestones with its
//! releases (`crate::releases`), and from a week before the date until the
//! release is out, or the milestone closes, it reminds you on Needs you of
//! what stands in the release's way (`waiting`): the milestone's open
//! issues and pull requests, version files that disagree, a failing default
//! branch, and open high or critical security alerts. The reminder comes
//! more often as the date nears: a week before, the day before, on the day,
//! then each day it is late, by the day in Bit's time zone
//! (`crate::clock`). Nothing here reaches GitHub but reads, and nothing is
//! posted there.
use crate::{
    problems::ProblemKind,
    releases::{version, Release, RepoReleases, Tag},
    types::*,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How many open milestones Orbit reads, those due soonest first, and how
/// many named for a version it keeps for a repository.
pub const READ: usize = 30;
pub const KEPT: usize = 5;
/// How many of a milestone's open issues, and of its open pull requests,
/// Orbit reads by number.
pub const LISTED: usize = 5;
/// How many days before its date a release's reminders start.
pub const WEEK: i64 = 7;

/// An open milestone named for a version, with a due date.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Milestone {
    pub number: u64,
    pub title: String,
    /// The version its title names, as written there: `v0.3.0`.
    pub version: String,
    /// The day it is due: `2026-11-04`.
    pub due: String,
    pub url: String,
    /// Its open issues and its open pull requests, by number, the first
    /// `LISTED` of each, and how many of each there are.
    pub issues: Vec<u64>,
    pub issue_count: u32,
    pub prs: Vec<u64>,
    pub pr_count: u32,
    /// A release of its version is out: one published on GitHub, or, in a
    /// repository with no releases, a tag for it.
    pub released: bool,
}

/// The version a milestone's title names, as written there: the whole
/// title, such as `v0.3.0`, or its last word, such as in `Release 0.3.0`.
pub fn version_of(title: &str) -> Option<&str> {
    let title = title.trim();
    let word = title.rsplit(char::is_whitespace).next().unwrap_or(title);
    version(word).map(|_| word)
}

/// The milestones in GitHub's answer to the releases query (its
/// `milestones` field), named for a version and with a due date, those due
/// soonest first, at most `KEPT`. `releases` and `tags` are what Orbit read
/// with them, to tell which are out already.
pub fn understand(nodes: &Value, releases: &[Release], tags: &[Tag]) -> Vec<Milestone> {
    let Some(nodes) = nodes["nodes"].as_array() else {
        return vec![];
    };
    let published: Vec<&Release> = releases.iter().filter(|r| !r.draft).collect();
    let mut kept: Vec<Milestone> = nodes
        .iter()
        .filter_map(|m| {
            let title = m["title"].as_str()?;
            let named = version_of(title)?;
            let v = version(named)?;
            // The day, as GitHub keeps it: `2026-11-04T07:00:00Z`.
            let due = m["dueOn"].as_str()?.get(..10)?;
            NaiveDate::parse_from_str(due, "%Y-%m-%d").ok()?;
            let same = |tag: &str| version(tag).is_some_and(|t| t == v);
            let released = published.iter().any(|r| same(&r.tag))
                || (releases.is_empty() && tags.iter().any(|t| same(&t.name)));
            let numbers = |field: &str| -> (Vec<u64>, u32) {
                let list = &m[field];
                let numbers = list["nodes"]
                    .as_array()
                    .map(|n| {
                        n.iter()
                            .filter_map(|i| i["number"].as_u64())
                            .take(LISTED)
                            .collect()
                    })
                    .unwrap_or_default();
                let count = list["totalCount"].as_u64().unwrap_or(0);
                (numbers, count.min(u32::MAX as u64) as u32)
            };
            let (issues, issue_count) = numbers("issues");
            let (prs, pr_count) = numbers("pullRequests");
            Some(Milestone {
                number: m["number"].as_u64()?,
                title: title.trim().to_string(),
                version: named.to_string(),
                due: due.to_string(),
                url: m["url"]
                    .as_str()
                    .filter(|u| u.starts_with("https://github.com/"))
                    .unwrap_or_default()
                    .to_string(),
                issues,
                issue_count,
                prs,
                pr_count,
                released,
            })
        })
        .collect();
    kept.sort_by(|a, b| a.due.cmp(&b.due).then(a.number.cmp(&b.number)));
    kept.truncate(KEPT);
    kept
}

/// How near a release's date is, on a day: the reminders' steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// Within a week: this many days to go, 2 to `WEEK`.
    Soon(i64),
    Tomorrow,
    Today,
    /// This many days late.
    Late(i64),
}
impl When {
    /// The step, for the Needs you item's key, which changes with it, so
    /// each step is announced once: the week before, the day before, the
    /// day, and each day late.
    fn step(self) -> String {
        match self {
            When::Soon(_) => "week".into(),
            When::Tomorrow => "tomorrow".into(),
            When::Today => "today".into(),
            When::Late(days) => format!("late-{days}"),
        }
    }
    /// "is due in 5 days", "is due tomorrow", "was due 2 days ago".
    fn text(self) -> String {
        match self {
            When::Soon(days) => format!("is due in {days} days"),
            When::Tomorrow => "is due tomorrow".into(),
            When::Today => "is due today".into(),
            When::Late(1) => "was due yesterday".into(),
            When::Late(days) => format!("was due {days} days ago"),
        }
    }
}
/// How near the day `due` is on the day `today`, from `WEEK` days before
/// it; None before that, or when `due` isn't a day.
pub fn when(due: &str, today: NaiveDate) -> Option<When> {
    let due = NaiveDate::parse_from_str(due, "%Y-%m-%d").ok()?;
    match (due - today).num_days() {
        days if days > WEEK => None,
        days if days >= 2 => Some(When::Soon(days)),
        1 => Some(When::Tomorrow),
        0 => Some(When::Today),
        days => Some(When::Late(-days)),
    }
}

/// A release that is due within a week, or late, and what stands in its
/// way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub repo: String,
    pub milestone: Milestone,
    pub when: When,
    /// What Orbit sees in its way, in words, such as "2 open pull requests
    /// (#12 and #14)".
    pub blockers: Vec<String>,
}

/// The Needs you item's key for a release that is due:
/// `due:<repo>:<milestone>:<step>`, so each step is announced once.
pub fn key(d: &Due) -> String {
    format!("due:{}:{}:{}", d.repo, d.milestone.number, d.when.step())
}
/// What the Needs you item says, such as "v0.3.0 of acme/api is due
/// tomorrow. In its way: 1 open pull request (#12) and CI failing on
/// main."
pub fn reason(d: &Due) -> String {
    let lead = format!("{} of {} {}.", d.milestone.version, d.repo, d.when.text());
    if d.blockers.is_empty() {
        format!("{lead} Nothing Orbit watches stands in its way.")
    } else {
        format!("{lead} In its way: {}.", and(&d.blockers))
    }
}

/// The releases due within a week, or late, on the day it is at `now` in
/// Bit's time zone, in each repository Orbit watches the releases of: not
/// out yet, with their milestones still open when Orbit last read them.
pub fn waiting(s: &Snapshot, now: DateTime<Utc>) -> Vec<Due> {
    if !s.settings.watch_ci {
        return vec![];
    }
    let today = crate::clock::local(&s.settings.buddy.time_zone, now).date();
    let connected = |repo: &str| {
        s.settings
            .repositories
            .iter()
            .any(|c| same_repo(&c.name, repo))
    };
    let mut due = vec![];
    for r in &s.releases {
        if r.read_at.is_empty() || r.unseen.is_some() || !connected(&r.repo) {
            continue;
        }
        for m in r.milestones.iter().filter(|m| !m.released) {
            let Some(when) = when(&m.due, today) else {
                continue;
            };
            due.push(Due {
                repo: r.repo.clone(),
                milestone: m.clone(),
                when,
                blockers: blockers(s, r, m),
            });
        }
    }
    due
}

/// What stands in a release's way, as Orbit sees it now.
fn blockers(s: &Snapshot, r: &RepoReleases, m: &Milestone) -> Vec<String> {
    let mut out = vec![];
    let numbered = |count: u32, listed: &[u64], one: &str, many: &str| {
        let mut numbers: Vec<String> = listed.iter().map(|n| format!("#{n}")).collect();
        let more = count.saturating_sub(listed.len() as u32);
        if more > 0 {
            numbers.push(format!("{more} more"));
        }
        let name = if count == 1 { one } else { many };
        if numbers.is_empty() {
            format!("{count} {name}")
        } else {
            format!("{count} {name} ({})", and(&numbers))
        }
    };
    if m.pr_count > 0 {
        out.push(numbered(
            m.pr_count,
            &m.prs,
            "open pull request",
            "open pull requests",
        ));
    }
    if m.issue_count > 0 {
        out.push(numbered(
            m.issue_count,
            &m.issues,
            "open issue",
            "open issues",
        ));
    }
    let open = |kind: ProblemKind| {
        s.problems
            .iter()
            .filter(move |p| p.is_open() && p.kind == kind && same_repo(&p.repo, &r.repo))
    };
    if open(ProblemKind::Versions).next().is_some() {
        out.push("version files that disagree".into());
    }
    if let Some(p) = open(ProblemKind::DefaultBranch).next() {
        out.push(format!("CI failing on {}", p.branch));
    }
    let severe = open(ProblemKind::Security)
        .flat_map(|p| &p.security_alerts)
        .filter(|a| a.severity >= crate::security::Severity::High)
        .count();
    match severe {
        0 => {}
        1 => out.push("1 high or critical security alert".into()),
        n => out.push(format!("{n} high or critical security alerts")),
    }
    out
}

fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
#[path = "milestones_tests.rs"]
mod tests;
