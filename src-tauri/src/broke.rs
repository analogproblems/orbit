//! What broke a default branch: the commits after the last one whose
//! checks passed, up to the first that failed, and the pull request when
//! they are one merge. Read once from GitHub's history of the branch when
//! a default branch's problem opens, following each commit's first parent,
//! as each of those is one change landing on the branch.
use crate::{
    ci::{self, CiState, CommitCi},
    logs,
    problems::{stamp, time, Problem, ProblemKind},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// The most commits Orbit looks back from the first one it saw failing.
pub const BACK: usize = 20;
/// Commits of history read per request, and the most requests for one
/// problem, as a merge's own commits come between the branch's.
const HISTORY: u32 = 40;
const REQUESTS: usize = 3;
/// Problems whose history Orbit reads in one pass.
pub const PER_PASS: usize = 2;

/// What Orbit found about where a default branch broke.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Broke {
    /// Whether Orbit has its answer; until then it reads again from
    /// `retry_at`.
    pub done: bool,
    /// The last commit whose checks passed, empty when Orbit found none in
    /// the `BACK` commits before the first it saw failing.
    pub green: String,
    /// The first commit that failed after it.
    pub red: String,
    /// The commits after `green` up to and including `red`, newest first.
    pub commits: Vec<String>,
    /// The pull requests merged as those commits, newest first, and how
    /// many of the commits came from none.
    pub pulls: Vec<u64>,
    pub direct: u32,
    /// The pull request when the commits are its merge alone.
    pub pull: Option<u64>,
    /// Why Orbit couldn't read it, or found nothing.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
}

/// One commit of the branch's history, as Orbit reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub parent: Option<String>,
    pub state: CiState,
    /// The merged pull request that brought it to the branch.
    pub pull: Option<u64>,
}
/// Where a walk back through the history got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Walked {
    Found(Broke),
    /// The history read so far doesn't reach this commit.
    Need(String),
}

/// The GraphQL query for a commit's history, with each commit's first
/// parent, checks and the pull request that brought it.
pub fn query() -> String {
    format!(
        "query($owner: String!, $name: String!, $oid: GitObjectID!) {{ \
repository(owner: $owner, name: $name) {{ object(oid: $oid) {{ ... on Commit {{ \
history(first: {HISTORY}) {{ nodes {{ oid parents(first: 1) {{ nodes {{ oid }} }} \
statusCheckRollup {{ contexts(first: 100) {{ {} }} }} \
associatedPullRequests(first: 5) {{ nodes {{ number merged baseRefName mergeCommit {{ oid }} }} }} \
}} }} }} }} }} }}",
        ci::NODES
    )
}

/// The commits in GitHub's answer to `query`, by SHA, or None when GitHub
/// doesn't have the commit asked about. `branch` is the default branch, as
/// a pull request merged into another branch didn't bring a commit here.
pub fn commits(v: &Value, branch: &str) -> Result<Option<HashMap<String, Seen>>, String> {
    ci::refused(v)?;
    let object = &v["data"]["repository"]["object"];
    if object.is_null() {
        return Ok(None);
    }
    let nodes = object["history"]["nodes"]
        .as_array()
        .ok_or("GitHub didn't return the branch's history.")?;
    let mut out = HashMap::new();
    for node in nodes {
        let Some(sha) = node["oid"].as_str().filter(|s| valid_sha(s)) else {
            return Err("GitHub returned an invalid commit SHA.".into());
        };
        let parent = node["parents"]["nodes"][0]["oid"]
            .as_str()
            .filter(|s| valid_sha(s))
            .map(str::to_string);
        let mut checks = CommitCi::default();
        ci::add_checks(&mut checks, &node["statusCheckRollup"]["contexts"]);
        ci::settle(&mut checks);
        let pull = node["associatedPullRequests"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["merged"] == true && p["baseRefName"] == branch)
            .and_then(|p| p["number"].as_u64());
        out.insert(
            sha.to_string(),
            Seen {
                parent,
                state: checks.state,
                pull,
            },
        );
    }
    Ok(Some(out))
}

/// Walks back from `start`, the first commit Orbit saw failing, through
/// first parents to the newest commit whose checks passed. The commits
/// after it up to the oldest one that failed are where it broke: one that
/// didn't run or finish its checks in between may have broken it too.
pub fn walk(start: &str, history: &HashMap<String, Seen>) -> Walked {
    let mut after: Vec<(&str, &Seen)> = vec![];
    let mut at = start.to_string();
    for _ in 0..BACK {
        let Some(seen) = history.get(&at) else {
            return Walked::Need(at);
        };
        if seen.state == CiState::Passing {
            return Walked::Found(between(&at, &after));
        }
        after.push((history.get_key_value(&at).unwrap().0, seen));
        match &seen.parent {
            Some(parent) => at = parent.clone(),
            // The branch's first commit.
            None => break,
        }
    }
    Walked::Found(Broke {
        done: true,
        error: Some(format!(
            "Orbit found no commit that passed in the {BACK} before {}.",
            ci::short(start)
        )),
        ..Default::default()
    })
}
/// Where it broke, from `green` and the commits after it, newest first.
fn between(green: &str, after: &[(&str, &Seen)]) -> Broke {
    let Some(red) = after.iter().rposition(|(_, s)| s.state == CiState::Failing) else {
        // GitHub no longer has it failing, such as after a rerun passed.
        return Broke {
            done: true,
            green: green.into(),
            error: Some("GitHub no longer has these commits failing.".into()),
            ..Default::default()
        };
    };
    let suspects = &after[red..];
    let mut pulls: Vec<u64> = vec![];
    for n in suspects.iter().filter_map(|(_, s)| s.pull) {
        if !pulls.contains(&n) {
            pulls.push(n);
        }
    }
    let direct = suspects.iter().filter(|(_, s)| s.pull.is_none()).count() as u32;
    Broke {
        done: true,
        green: green.into(),
        red: suspects[0].0.into(),
        commits: suspects.iter().map(|(sha, _)| sha.to_string()).collect(),
        pull: (pulls.len() == 1 && direct == 0).then(|| pulls[0]),
        pulls,
        direct,
        ..Default::default()
    }
}

/// The default branch problems whose history Orbit should read now, at
/// most `PER_PASS`: those it hasn't tried first, then those due again.
pub fn wanted(problems: &[Problem], now: DateTime<Utc>) -> Vec<(String, String, String, String)> {
    let mut due: Vec<&Problem> = problems
        .iter()
        .filter(|p| p.is_open() && p.kind == ProblemKind::DefaultBranch)
        .filter(|p| valid_sha(&p.since_sha) && !p.branch.is_empty())
        .filter(|p| match &p.broke {
            None => true,
            Some(b) => {
                !b.done
                    && b.retry_at
                        .as_deref()
                        .and_then(time)
                        .is_none_or(|at| now >= at)
            }
        })
        .collect();
    due.sort_by_key(|p| p.broke.as_ref().map_or(0, |b| b.tries));
    due.into_iter()
        .take(PER_PASS)
        .map(|p| {
            (
                p.id.clone(),
                p.repo.clone(),
                p.branch.clone(),
                p.since_sha.clone(),
            )
        })
        .collect()
}

/// Reads the branch's history back from `start` and finds where it broke.
pub async fn read(repo: &str, branch: &str, start: &str) -> Result<Broke, String> {
    validate_repo(repo)?;
    let (owner, name) = repo.split_once('/').ok_or("Invalid repository")?;
    let mut history = HashMap::new();
    let mut from = start.to_string();
    for _ in 0..REQUESTS {
        let v = crate::github::api(
            "graphql",
            "POST",
            &[
                ("query", query()),
                ("owner", owner.to_string()),
                ("name", name.to_string()),
                ("oid", from.clone()),
            ],
            None,
        )
        .await?;
        let Some(more) = commits(&v, branch)? else {
            return Ok(Broke {
                done: true,
                error: Some(format!("GitHub no longer has commit {}.", ci::short(&from))),
                ..Default::default()
            });
        };
        if more.is_empty() {
            return Err("GitHub returned no history.".into());
        }
        history.extend(more);
        match walk(start, &history) {
            Walked::Found(broke) => return Ok(broke),
            Walked::Need(at) => from = at,
        }
    }
    Ok(Broke {
        done: true,
        error: Some("The branch's history was too tangled for Orbit to follow.".into()),
        ..Default::default()
    })
}

/// Records what Orbit read back from `start` for a problem, unless it has
/// since started failing again from another commit. Returns whether
/// anything changed.
pub fn record(
    problems: &mut [Problem],
    id: &str,
    start: &str,
    result: Result<Broke, String>,
    now: DateTime<Utc>,
) -> bool {
    let Some(p) = problems
        .iter_mut()
        .find(|p| p.id == id && p.is_open() && p.since_sha == start)
    else {
        return false;
    };
    let broke = match result {
        Ok(broke) => broke,
        Err(error) => {
            let tries = p.broke.as_ref().map_or(0, |b| b.tries) + 1;
            let wait = Duration::minutes(1i64 << (tries - 1).min(6)).min(logs::MAX_RETRY);
            Broke {
                error: Some(format!(
                    "Orbit couldn't read the branch's history yet: {}. It tries again later.",
                    logs::short(&error)
                )),
                tries,
                retry_at: Some(stamp(now + wait)),
                ..Default::default()
            }
        }
    };
    p.broke = Some(broke);
    true
}

/// Where the branch broke, in a sentence, once Orbit has found it.
pub fn text(b: &Broke) -> Option<String> {
    sentence(b, |sha| ci::short(sha).to_string())
}
/// `text`, with each commit as code, for Orbit's issue.
pub fn markdown(b: &Broke) -> Option<String> {
    sentence(b, |sha| format!("`{}`", ci::short(sha)))
}
fn sentence(b: &Broke, code: impl Fn(&str) -> String) -> Option<String> {
    if !b.done || b.commits.is_empty() {
        return None;
    }
    let (red, green) = (code(&b.red), code(&b.green));
    let n = b.commits.len();
    Some(match (b.pull, n) {
        (Some(pr), 1) => format!("It started failing with #{pr}, merged as {red}; {green} before it passed."),
        (Some(pr), _) => format!(
            "It started failing with #{pr}, merged as {n} commits up to {red}; {green} before them passed."
        ),
        (None, 1) => format!("It started failing at {red}, pushed without a pull request; {green} before it passed."),
        (None, _) => {
            let mut from: Vec<String> = b.pulls.iter().map(|n| format!("#{n}")).collect();
            if b.direct > 0 {
                from.push(match b.direct {
                    1 => "1 pushed without a pull request".into(),
                    d => format!("{d} pushed without one"),
                });
            }
            format!(
                "It started failing in the {n} commits after {green}, which passed, up to {red}: from {}.",
                and(&from)
            )
        }
    })
}
/// "A", "A and B", "A, B and C".
fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
#[path = "broke_tests.rs"]
mod tests;
