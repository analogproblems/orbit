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
/// Checks read per page of a commit's checks.
const CHECKS: u32 = 100;
/// Further pages of checks read per repository, for commits with more than
/// `CHECKS`. A commit whose checks aren't all read never counts as passing.
pub const MORE_PAGES: usize = 20;

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
    /// Running while some checks weren't read: it can fail, but not pass.
    pub state: CiState,
    /// The checks that failed, by name.
    pub failing: Vec<Check>,
    pub running: u32,
    pub passing: u32,
    /// Orbit couldn't read all of its checks.
    pub incomplete: bool,
    /// When the earliest of the checks that passed began, by GitHub's
    /// clock: when its check suite or status was made. Empty when one of
    /// them doesn't say.
    pub ran_at: String,
    /// When the commit was made, by GitHub's clock, for a default branch's
    /// latest commit.
    pub committed_at: String,
    /// Where the next page of its checks starts, while one is still to read.
    #[serde(skip)]
    pub cursor: Option<String>,
    /// A check that passed didn't say when it began.
    #[serde(skip)]
    pub untimed: bool,
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

pub(crate) const NODES: &str = "pageInfo { hasNextPage endCursor } nodes { __typename \
... on CheckRun { name status conclusion detailsUrl checkSuite { createdAt workflowRun { workflow { name } } } } \
... on StatusContext { context state targetUrl createdAt } }";

/// The GraphQL query for one repository's CI.
pub fn query() -> String {
    let contexts = format!("statusCheckRollup {{ contexts(first: {CHECKS}) {{ {NODES} }} }}");
    format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ \
defaultBranchRef {{ name target {{ ... on Commit {{ oid committedDate messageHeadline {contexts} }} }} }} \
pullRequests(states: OPEN, first: {PULLS}, orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ \
pageInfo {{ hasNextPage }} nodes {{ number isDraft headRefOid \
commits(last: 1) {{ nodes {{ commit {{ oid {contexts} }} }} }} }} }} }} }}"
    )
}
/// The GraphQL query for the next page of one commit's checks.
pub fn more_query() -> String {
    format!(
        "query($owner: String!, $name: String!, $oid: GitObjectID!, $cursor: String!) {{ \
repository(owner: $owner, name: $name) {{ object(oid: $oid) {{ ... on Commit {{ \
statusCheckRollup {{ contexts(first: {CHECKS}, after: $cursor) {{ {NODES} }} }} }} }} }} }}"
    )
}

/// Reads one repository's CI from GitHub, with every page of each commit's
/// checks up to `MORE_PAGES` more.
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
    let mut ci = parse(repo, &v)?;
    let mut pages = MORE_PAGES;
    let commits = std::iter::once(&mut ci.head).chain(ci.pulls.iter_mut().map(|p| &mut p.ci));
    for commit in commits {
        while let Some(cursor) = commit.cursor.take() {
            if pages == 0 {
                commit.incomplete = true;
                break;
            }
            pages -= 1;
            let v = crate::github::api(
                "graphql",
                "POST",
                &[
                    ("query", more_query()),
                    ("owner", owner.to_string()),
                    ("name", name.to_string()),
                    ("oid", commit.sha.clone()),
                    ("cursor", cursor),
                ],
                None,
            )
            .await?;
            refused(&v)?;
            let contexts = &v["data"]["repository"]["object"]["statusCheckRollup"]["contexts"];
            if contexts.is_null() {
                // The rest of its checks can't be read now.
                commit.incomplete = true;
                break;
            }
            add_checks(commit, contexts);
        }
        settle(commit);
    }
    Ok(ci)
}

/// The GraphQL query for one commit's checks, a page at a time.
pub fn commit_query() -> String {
    format!(
        "query($owner: String!, $name: String!, $oid: GitObjectID!, $cursor: String) {{ \
repository(owner: $owner, name: $name) {{ object(oid: $oid) {{ ... on Commit {{ \
statusCheckRollup {{ contexts(first: {CHECKS}, after: $cursor) {{ {NODES} }} }} }} }} }} }}"
    )
}
/// Where the check named `name` stands at commit `oid`: failing if any run
/// of it failed, running while one runs or while some of the commit's
/// checks are still to read, passing once one passed, and None when it
/// didn't run there.
pub async fn check_at(repo: &str, oid: &str, name: &str) -> Result<CiState, String> {
    check_at_within(repo, oid, name, MORE_PAGES + 1).await
}
/// `check_at`, reading at most `most` pages of the commit's checks.
pub(crate) async fn check_at_within(
    repo: &str,
    oid: &str,
    name: &str,
    most: usize,
) -> Result<CiState, String> {
    validate_repo(repo)?;
    if !valid_sha(oid) {
        return Err("Invalid commit".into());
    }
    let (owner, name_of_repo) = repo.split_once('/').ok_or("Invalid repository")?;
    let mut states = vec![];
    let mut cursor: Option<String> = None;
    // Only a last page proves there are no more runs of it.
    let mut unread = true;
    for _ in 0..most {
        let mut fields = vec![
            ("query", commit_query()),
            ("owner", owner.to_string()),
            ("name", name_of_repo.to_string()),
            ("oid", oid.to_string()),
        ];
        if let Some(c) = cursor.take() {
            fields.push(("cursor", c));
        }
        let v = crate::github::api("graphql", "POST", &fields, None).await?;
        refused(&v)?;
        let contexts = &v["data"]["repository"]["object"]["statusCheckRollup"]["contexts"];
        for node in contexts["nodes"].as_array().into_iter().flatten() {
            if let Some((check, state)) = check(node) {
                if check.name == name {
                    states.push(state);
                }
            }
        }
        if contexts["pageInfo"]["hasNextPage"] != true {
            unread = false;
            break;
        }
        match contexts["pageInfo"]["endCursor"].as_str() {
            Some(c) if !c.is_empty() => cursor = Some(c.to_string()),
            // More checks, but nowhere to read them from.
            _ => break,
        }
    }
    Ok(if states.contains(&CiState::Failing) {
        CiState::Failing
    } else if states.contains(&CiState::Running) || unread {
        CiState::Running
    } else if states.contains(&CiState::Passing) {
        CiState::Passing
    } else {
        CiState::None
    })
}

/// What GitHub refused, from a GraphQL answer.
pub(crate) fn refused(v: &Value) -> Result<(), String> {
    match v["errors"].as_array().and_then(|e| e.first()) {
        Some(error) => Err(error["message"]
            .as_str()
            .unwrap_or("GitHub returned an error.")
            .to_string()),
        None => Ok(()),
    }
}

/// A repository's CI from GitHub's answer to `query`. A commit with more
/// checks than one page keeps a `cursor` for `read` to follow.
pub fn parse(repo: &str, v: &Value) -> Result<RepoCi, String> {
    refused(v)?;
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

/// A commit's checks, from its `oid` and the first page of its
/// `statusCheckRollup`.
fn commit(v: &Value) -> Result<CommitCi, String> {
    let sha = v["oid"].as_str().unwrap_or("").to_string();
    if !valid_sha(&sha) {
        return Err("GitHub returned an invalid commit SHA.".into());
    }
    let mut ci = CommitCi {
        sha,
        committed_at: v["committedDate"].as_str().unwrap_or("").to_string(),
        ..Default::default()
    };
    add_checks(&mut ci, &v["statusCheckRollup"]["contexts"]);
    settle(&mut ci);
    Ok(ci)
}

/// Adds one page of a commit's checks, and where the next page starts.
pub fn add_checks(ci: &mut CommitCi, contexts: &Value) {
    for node in contexts["nodes"].as_array().into_iter().flatten() {
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
            CiState::Passing => {
                ci.passing += 1;
                match began(node) {
                    Some(at) => {
                        let earlier = crate::problems::time(&ci.ran_at).is_none_or(|t| at < t);
                        if earlier {
                            ci.ran_at = crate::problems::stamp(at);
                        }
                    }
                    None => ci.untimed = true,
                }
            }
            CiState::None => {}
        }
    }
    let more = contexts["pageInfo"]["hasNextPage"].as_bool() == Some(true);
    ci.cursor = None;
    if more {
        match contexts["pageInfo"]["endCursor"].as_str() {
            Some(cursor) if !cursor.is_empty() => ci.cursor = Some(cursor.to_string()),
            // More checks, but nowhere to read them from.
            _ => ci.incomplete = true,
        }
    }
}

/// Where a commit's checks stand from what was read. Checks still to read
/// keep it from passing: one failure fails it, otherwise it is running.
pub fn settle(ci: &mut CommitCi) {
    let unread = ci.incomplete || ci.cursor.is_some();
    if ci.untimed {
        ci.ran_at.clear();
    }
    ci.state = if !ci.failing.is_empty() {
        CiState::Failing
    } else if ci.running > 0 || unread {
        CiState::Running
    } else if ci.passing > 0 {
        CiState::Passing
    } else {
        CiState::None
    };
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

/// When a check began: when GitHub made its check suite, which for GitHub
/// Actions is when its workflow was started, or for a commit status, when
/// it was set.
fn began(node: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    let at = match node["__typename"].as_str()? {
        "CheckRun" => &node["checkSuite"]["createdAt"],
        _ => &node["createdAt"],
    };
    crate::problems::time(at.as_str()?)
}

/// A commit's first seven characters, as GitHub shows them.
pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

#[cfg(test)]
#[path = "ci_tests.rs"]
mod tests;
