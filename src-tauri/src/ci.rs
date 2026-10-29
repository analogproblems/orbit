//! Reading CI from GitHub: the checks on a repository's default branch and
//! on every open pull request's latest commit, in one GraphQL request per
//! repository. Orbit's own `orbit/review` status is left out, so Orbit never
//! reacts to itself.
use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The commit status Orbit sets itself (see `crate::engine` `status`).
pub const OWN_STATUS: &str = "orbit/review";
/// Open pull requests read per repository. With more open, the list is
/// incomplete, and a problem isn't closed because its pull request is
/// missing from it.
const PULLS: u32 = 100;

/// Where a commit's checks, or one check, stand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiState {
    /// No checks ran, or only ones that were skipped or cancelled.
    #[default]
    None,
    /// Some are still running and none failed.
    Running,
    Passing,
    Failing,
}
/// One check that didn't pass.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Check {
    /// Such as "CI / Rust (ubuntu-24.04)": the workflow, then the job.
    pub name: String,
    /// Its page on GitHub, when it has one.
    pub url: String,
}
/// A commit's checks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitCi {
    pub sha: String,
    pub state: CiState,
    /// The checks that failed, by name.
    pub failing: Vec<Check>,
    pub running: u32,
    pub passing: u32,
}
/// An open pull request's latest commit and its checks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PullCi {
    pub number: u64,
    pub draft: bool,
    pub ci: CommitCi,
}
/// A repository's CI, as Orbit last read it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoCi {
    pub repo: String,
    /// The default branch, such as `main`.
    pub branch: String,
    /// The default branch's latest commit.
    pub head: CommitCi,
    /// That commit's first line, as GitHub shows it. Someone else's words.
    pub title: String,
    pub pulls: Vec<PullCi>,
    /// Whether `pulls` lists every open pull request.
    pub complete: bool,
    /// When CI was last read without an error.
    pub checked_at: String,
    /// Why the latest read failed. What was read before is kept.
    pub error: Option<String>,
}
impl RepoCi {
    pub fn pull(&self, number: u64) -> Option<&PullCi> {
        self.pulls.iter().find(|p| p.number == number)
    }
}

const CONTEXTS: &str = "statusCheckRollup { contexts(first: 100) { nodes { __typename \
... on CheckRun { name status conclusion detailsUrl checkSuite { workflowRun { workflow { name } } } } \
... on StatusContext { context state targetUrl } } } }";

/// The GraphQL query for one repository's CI.
pub fn query() -> String {
    format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ \
defaultBranchRef {{ name target {{ ... on Commit {{ oid messageHeadline {CONTEXTS} }} }} }} \
pullRequests(states: OPEN, first: {PULLS}, orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ \
pageInfo {{ hasNextPage }} nodes {{ number isDraft headRefOid \
commits(last: 1) {{ nodes {{ commit {{ oid {CONTEXTS} }} }} }} }} }} }} }}"
    )
}

/// Reads one repository's CI from GitHub.
pub async fn read(repo: &str) -> Result<RepoCi, String> {
    validate_repo(repo)?;
    let (owner, name) = repo.split_once('/').ok_or("Invalid repository")?;
    let v = crate::github::api(
        "graphql",
        "POST",
        &[
            ("query", query()),
            ("owner", owner.to_string()),
            ("name", name.to_string()),
        ],
        None,
    )
    .await?;
    parse(repo, &v)
}

/// A repository's CI from GitHub's answer to `query`.
pub fn parse(repo: &str, v: &Value) -> Result<RepoCi, String> {
    if let Some(error) = v["errors"].as_array().and_then(|e| e.first()) {
        return Err(error["message"]
            .as_str()
            .unwrap_or("GitHub returned an error.")
            .to_string());
    }
    let r = &v["data"]["repository"];
    if r.is_null() {
        return Err("GitHub didn't return the repository.".into());
    }
    let branch = &r["defaultBranchRef"];
    let target = &branch["target"];
    let head = if target.is_null() {
        // A repository with no commits yet has no default branch to check.
        CommitCi::default()
    } else {
        commit(target)?
    };
    let pulls = r["pullRequests"]["nodes"]
        .as_array()
        .ok_or("GitHub didn't return the pull requests.")?
        .iter()
        .map(|p| {
            let number = p["number"].as_u64().ok_or("Invalid pull request number")?;
            let latest = &p["commits"]["nodes"][0]["commit"];
            let ci = if latest.is_null() {
                CommitCi {
                    sha: p["headRefOid"].as_str().unwrap_or("").to_string(),
                    ..Default::default()
                }
            } else {
                commit(latest)?
            };
            Ok(PullCi {
                number,
                draft: p["isDraft"].as_bool().unwrap_or(false),
                ci,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RepoCi {
        repo: repo.to_string(),
        branch: branch["name"].as_str().unwrap_or("").to_string(),
        head,
        title: target["messageHeadline"].as_str().unwrap_or("").to_string(),
        pulls,
        complete: !r["pullRequests"]["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(true),
        checked_at: now(),
        error: None,
    })
}

/// A commit's checks, from its `oid` and `statusCheckRollup`.
fn commit(v: &Value) -> Result<CommitCi, String> {
    let sha = v["oid"].as_str().unwrap_or("").to_string();
    if !valid_sha(&sha) {
        return Err("GitHub returned an invalid commit SHA.".into());
    }
    let mut ci = CommitCi {
        sha,
        ..Default::default()
    };
    let nodes = v["statusCheckRollup"]["contexts"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for node in &nodes {
        let Some((check, state)) = check(node) else {
            continue;
        };
        match state {
            CiState::Failing => {
                if !ci.failing.iter().any(|c| c.name == check.name) {
                    ci.failing.push(check);
                }
            }
            CiState::Running => ci.running += 1,
            CiState::Passing => ci.passing += 1,
            CiState::None => {}
        }
    }
    ci.state = if !ci.failing.is_empty() {
        CiState::Failing
    } else if ci.running > 0 {
        CiState::Running
    } else if ci.passing > 0 {
        CiState::Passing
    } else {
        CiState::None
    };
    Ok(ci)
}

/// One check and where it stands, or None for Orbit's own status and
/// anything GitHub didn't describe.
fn check(node: &Value) -> Option<(Check, CiState)> {
    let text = |key: &str| node[key].as_str().unwrap_or("").to_string();
    match node["__typename"].as_str()? {
        "CheckRun" => {
            let job = text("name");
            let workflow = node["checkSuite"]["workflowRun"]["workflow"]["name"]
                .as_str()
                .unwrap_or("");
            let name = if workflow.is_empty() {
                job
            } else {
                format!("{workflow} / {job}")
            };
            let state = if node["status"] != "COMPLETED" {
                CiState::Running
            } else {
                match node["conclusion"].as_str().unwrap_or("") {
                    "SUCCESS" => CiState::Passing,
                    "FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE" => CiState::Failing,
                    // Waiting for someone to let it run.
                    "ACTION_REQUIRED" => CiState::Running,
                    // Cancelled, skipped, neutral or stale: neither way.
                    _ => CiState::None,
                }
            };
            Some((
                Check {
                    name,
                    url: text("detailsUrl"),
                },
                state,
            ))
        }
        "StatusContext" => {
            let name = text("context");
            if name == OWN_STATUS {
                return None;
            }
            let state = match node["state"].as_str().unwrap_or("") {
                "SUCCESS" => CiState::Passing,
                "FAILURE" | "ERROR" => CiState::Failing,
                "PENDING" | "EXPECTED" => CiState::Running,
                _ => CiState::None,
            };
            Some((
                Check {
                    name,
                    url: text("targetUrl"),
                },
                state,
            ))
        }
        _ => None,
    }
}

/// A commit's first seven characters, as GitHub shows them.
pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

#[cfg(test)]
#[path = "ci_tests.rs"]
mod tests;
