//! Release builds: the workflow runs each connected repository's newest
//! version tag started, read while Watch CI is on, a few repositories a
//! pass. A run counts when GitHub says it ran for that tag (`head_branch`)
//! at the tag's commit, whatever started it: the push of the tag, a
//! release made from it, or a run started by hand on it. Only the newest
//! run of each workflow counts, as a newer one is a try of the same build.
//! A failing build is a problem that needs you and that the repository's
//! agent hears of (`problems::see_build`), as for a default branch, so
//! Orbit sums up why its jobs failed and reruns bad luck as Bit's rules
//! say. Nothing here reaches GitHub but reads.
use crate::{
    ci::{self, Check, CiState, CommitCi},
    github::{Pages, PAGE},
    logs,
    problems::{stamp, time},
    releases::{version, RepoReleases, Tag},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Repositories Orbit reads in one pass.
pub const PER_PASS: usize = 4;
/// How often Orbit reads a build again while it runs or fails.
pub const EVERY: Duration = Duration::minutes(5);
/// How often Orbit reads a passing build again, as a workflow may run at
/// the same tag later, such as one a published release starts.
pub const PASSED_EVERY: Duration = Duration::minutes(30);
/// How long Orbit keeps looking for the runs of a tag that has none yet.
pub const LOOK_FOR: Duration = Duration::days(1);
/// The most pages of runs, and of a failed run's jobs, Orbit reads.
pub const RUN_PAGES: usize = 3;
pub const JOB_PAGES: usize = 3;

/// One workflow's newest run for a tag.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Run {
    pub id: u64,
    /// The run's name, as GitHub shows it: its workflow's, unless the
    /// workflow names its runs. Someone else's words.
    pub name: String,
    /// Its page on GitHub.
    pub url: String,
    /// What started it, such as `push` or `release`.
    pub event: String,
    pub state: CiState,
}
/// The runs a repository's newest version tag started, as Orbit last read
/// them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Build {
    pub repo: String,
    /// The tag, and the commit it points at.
    pub tag: String,
    pub sha: String,
    /// Each workflow's newest run, by name.
    pub runs: Vec<Run>,
    /// Where they stand together: the failed jobs of failed runs, by name
    /// as `Workflow / job`, and how many runs pass or still run. A run
    /// that wasn't read keeps it from passing.
    pub ci: CommitCi,
    /// When Orbit first read this tag's runs, and last did.
    pub first_read_at: String,
    pub read_at: String,
    /// Why the latest read failed, and when Orbit tries again. What was
    /// read before is kept.
    pub error: Option<String>,
    pub retry_at: Option<String>,
    pub tries: u32,
}

/// The tag whose build Orbit watches in a repository, once it has read its
/// tags: the newest version tag, the highest version Orbit read. Orbit
/// reads the tags of the newest commits only, so when there are more, the
/// build it knows of (`known`) stays the one watched while its tag is a
/// higher version than any Orbit read, or the same one spelled another way
/// (`2.0.0` or `v2.0.0+build` for `v2.0.0`), and isn't among them, as it
/// may just be on an older commit. Only a higher version Orbit read, or a
/// full list without it, replaces it.
pub fn target(r: &RepoReleases, known: Option<&Build>) -> Option<Tag> {
    if r.read_at.is_empty() || r.unseen.is_some() {
        return None;
    }
    let newest = r.tags.first().filter(|t| valid_sha(&t.sha));
    let kept = known.filter(|b| {
        r.more_tags
            && same_repo(&b.repo, &r.repo)
            && valid_sha(&b.sha)
            && !r.tags.iter().any(|t| t.name == b.tag)
            && version(&b.tag)
                .is_some_and(|v| newest.and_then(|t| version(&t.name)).is_none_or(|n| v >= n))
    });
    match kept {
        Some(b) => Some(Tag {
            name: b.tag.clone(),
            sha: b.sha.clone(),
        }),
        None => newest.cloned(),
    }
}
/// Forgets a repository's build once a read of its tags that worked shows
/// its tag is no longer the one to watch (`target`): a higher version came,
/// the tag moved, or, from a full list, it is gone. Its problem closes
/// apart (`problems::close_old_builds`).
pub fn keep_current(builds: &mut Vec<Build>, r: &RepoReleases) {
    if r.read_at.is_empty() || r.unseen.is_some() {
        return;
    }
    builds.retain(|b| {
        !same_repo(&b.repo, &r.repo)
            || target(r, Some(b)).is_some_and(|t| t.name == b.tag && t.sha == b.sha)
    });
}
/// The repositories whose newest version tag's runs Orbit should read
/// now, with that tag (`target`), at most `PER_PASS`: those it hasn't read
/// for that tag at that commit first, then those read longest ago. A build
/// is read again every `EVERY` while it runs or fails, or while it has no
/// runs for up to `LOOK_FOR` after Orbit first read it, as its workflow may
/// not have started yet, and every `PASSED_EVERY` once it passes, as a
/// later run at the tag, such as one a published release starts, may fail.
/// A read that failed waits for its `retry_at`.
pub fn wanted(
    repos: &[String],
    releases: &[RepoReleases],
    builds: &[Build],
    now: DateTime<Utc>,
) -> Vec<(String, Tag)> {
    let since = |at: &str, every: Duration| time(at).is_none_or(|at| now - at >= every);
    let mut due: Vec<(String, String, Tag)> = repos
        .iter()
        .filter(|r| validate_repo(r).is_ok())
        .filter_map(|repo| {
            let r = releases.iter().find(|r| same_repo(&r.repo, repo))?;
            let known = builds.iter().find(|b| same_repo(&b.repo, repo));
            let tag = target(r, known)?;
            let b = known.filter(|b| b.tag == tag.name && b.sha == tag.sha);
            let Some(b) = b else {
                return Some((String::new(), repo.clone(), tag));
            };
            let ready = match &b.error {
                Some(_) => b
                    .retry_at
                    .as_deref()
                    .and_then(time)
                    .is_none_or(|retry| now >= retry),
                None => match b.ci.state {
                    CiState::Running | CiState::Failing => since(&b.read_at, EVERY),
                    CiState::None => {
                        let new = time(&b.first_read_at).is_some_and(|at| now - at < LOOK_FOR);
                        new && since(&b.read_at, EVERY)
                    }
                    CiState::Passing => since(&b.read_at, PASSED_EVERY),
                },
            };
            ready.then(|| (b.read_at.clone(), repo.clone(), tag))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter()
        .take(PER_PASS)
        .map(|(_, repo, tag)| (repo, tag))
        .collect()
}

/// Where a run stands, as GitHub says it: running until it completes,
/// then by how it ended, as `ci` reads a check. A run waiting for someone
/// to let it go on is running; one that was cancelled or skipped counts
/// neither way.
pub fn run_state(v: &Value) -> CiState {
    if v["status"] != "completed" {
        return CiState::Running;
    }
    match v["conclusion"].as_str().unwrap_or("") {
        "success" => CiState::Passing,
        "failure" | "timed_out" | "startup_failure" => CiState::Failing,
        "action_required" => CiState::Running,
        _ => CiState::None,
    }
}
/// Each workflow's newest run from GitHub's list, for `tag` at its commit
/// only, by name.
pub fn runs_of(items: &[Value], tag: &Tag) -> Vec<(Run, Value)> {
    let mut newest: Vec<(Value, u64)> = vec![];
    for v in items {
        if v["head_branch"] != tag.name.as_str() || v["head_sha"] != tag.sha.as_str() {
            continue;
        }
        let Some(id) = v["id"].as_u64() else {
            continue;
        };
        let workflow = |v: &Value| match v["workflow_id"].as_u64() {
            Some(w) => w.to_string(),
            None => v["name"].as_str().unwrap_or_default().to_string(),
        };
        match newest.iter_mut().find(|(n, _)| workflow(n) == workflow(v)) {
            Some(n) if n.1 < id => *n = (v.clone(), id),
            Some(_) => {}
            None => newest.push((v.clone(), id)),
        }
    }
    let mut runs: Vec<(Run, Value)> = newest
        .into_iter()
        .map(|(v, id)| {
            let text = |key: &str| v[key].as_str().unwrap_or_default().to_string();
            let run = Run {
                id,
                name: text("name"),
                url: text("html_url"),
                event: text("event"),
                state: run_state(&v),
            };
            (run, v)
        })
        .collect();
    runs.sort_by(|a, b| a.0.name.cmp(&b.0.name).then(a.0.id.cmp(&b.0.id)));
    runs
}
/// The jobs that failed in a run's latest try, from GitHub's pages of its
/// jobs, as checks named `Workflow / job`.
pub fn failed_jobs(run: &Run, jobs: &[Value]) -> Vec<Check> {
    jobs.iter()
        .filter(|j| j["status"] == "completed")
        .filter(|j| {
            matches!(
                j["conclusion"].as_str(),
                Some("failure" | "timed_out" | "startup_failure")
            )
        })
        .map(|j| {
            let job = j["name"].as_str().unwrap_or_default();
            Check {
                name: if run.name.is_empty() {
                    job.to_string()
                } else {
                    format!("{} / {job}", run.name)
                },
                url: j["html_url"].as_str().unwrap_or_default().to_string(),
            }
        })
        .collect()
}

/// Reads the runs `tag` started in `repo`, and the jobs of each that
/// failed. A run whose jobs Orbit can't find failed is a check of its own,
/// named for the run, so a build that failed before any job ran still
/// fails.
pub async fn read(repo: &str, tag: &Tag, now: DateTime<Utc>) -> Result<Build, String> {
    validate_repo(repo)?;
    if crate::releases::version(&tag.name).is_none() {
        return Err("Invalid tag".into());
    }
    if !valid_sha(&tag.sha) {
        return Err("Invalid commit".into());
    }
    let mut list = Pages::new("workflow_runs", "release runs");
    for n in 1..=RUN_PAGES {
        let fields = [
            ("branch", tag.name.clone()),
            ("head_sha", tag.sha.clone()),
            ("per_page", PAGE.to_string()),
            ("page", n.to_string()),
        ];
        let v =
            crate::github::api(&format!("repos/{repo}/actions/runs"), "GET", &fields, None).await?;
        if !list.add(&v)? {
            break;
        }
    }
    let mut ci = CommitCi {
        sha: tag.sha.clone(),
        incomplete: !list.all(),
        ..Default::default()
    };
    let runs = runs_of(&list.items, tag);
    for (run, _) in &runs {
        match run.state {
            CiState::Failing => {
                let jobs = jobs(repo, run.id).await?;
                let failed = failed_jobs(run, &jobs);
                if failed.is_empty() {
                    ci.failing.push(Check {
                        name: run.name.clone(),
                        url: run.url.clone(),
                    });
                }
                for check in failed {
                    if !ci.failing.iter().any(|c| c.name == check.name) {
                        ci.failing.push(check);
                    }
                }
            }
            CiState::Running => ci.running += 1,
            CiState::Passing => ci.passing += 1,
            CiState::None => {}
        }
    }
    ci::settle(&mut ci);
    let at = stamp(now);
    Ok(Build {
        repo: repo.to_string(),
        tag: tag.name.clone(),
        sha: tag.sha.clone(),
        runs: runs.into_iter().map(|(run, _)| run).collect(),
        ci,
        first_read_at: at.clone(),
        read_at: at,
        ..Default::default()
    })
}
/// The latest try of each job in a run, every page up to `JOB_PAGES`.
async fn jobs(repo: &str, run: u64) -> Result<Vec<Value>, String> {
    let mut jobs: Vec<Value> = vec![];
    for page in 1..=JOB_PAGES {
        let fields = [
            ("filter", "latest".to_string()),
            ("per_page", PAGE.to_string()),
            ("page", page.to_string()),
        ];
        let v = crate::github::api(
            &format!("repos/{repo}/actions/runs/{run}/jobs"),
            "GET",
            &fields,
            None,
        )
        .await?;
        let (some, total) = crate::reruns::page_of(&v)?;
        jobs.extend(some.iter().cloned());
        if some.is_empty() || jobs.len() as u64 >= total {
            break;
        }
    }
    Ok(jobs)
}

/// Keeps what Orbit read of a repository's build, in place of what it read
/// before. A read of the same tag at the same commit keeps when Orbit
/// first read it. A failed read keeps the earlier read of that tag, if
/// any, with why and when to try again, backing off from a minute up to
/// `logs::MAX_RETRY`.
pub fn record(
    builds: &mut Vec<Build>,
    repo: &str,
    tag: &Tag,
    result: Result<Build, String>,
    now: DateTime<Utc>,
) {
    let known = builds.iter().position(|b| same_repo(&b.repo, repo));
    let same = known
        .map(|i| &builds[i])
        .filter(|b| b.tag == tag.name && b.sha == tag.sha)
        .cloned();
    let build = match result {
        Ok(mut build) => {
            if let Some(before) = same.filter(|b| !b.first_read_at.is_empty()) {
                build.first_read_at = before.first_read_at;
            }
            build
        }
        Err(error) => {
            let mut before = same.unwrap_or_else(|| Build {
                repo: repo.to_string(),
                tag: tag.name.clone(),
                sha: tag.sha.clone(),
                first_read_at: stamp(now),
                ..Default::default()
            });
            before.tries += 1;
            let wait = Duration::minutes(1i64 << (before.tries - 1).min(6)).min(logs::MAX_RETRY);
            before.error = Some(format!(
                "Orbit couldn't read the release build yet: {}. It tries again later.",
                logs::short(&error)
            ));
            before.retry_at = Some(stamp(now + wait));
            before
        }
    };
    match known {
        Some(i) => builds[i] = build,
        None => builds.push(build),
    }
}

/// Forgets repositories that are no longer connected.
pub fn prune(builds: &mut Vec<Build>, connected: &[String]) {
    builds.retain(|b| connected.iter().any(|c| same_repo(c, &b.repo)));
}

/// What the build of a repository's newest tag says, in a sentence, or
/// None before Orbit has read it, or once a tag with no runs is too old to
/// still be waiting for them (`LOOK_FOR`), as GitHub deletes old runs.
pub fn sentence(b: &Build, now: DateTime<Utc>) -> Option<String> {
    if b.read_at.is_empty() {
        return None;
    }
    let new = time(&b.first_read_at).is_some_and(|at| now - at < LOOK_FOR);
    let runs = |n: u32| match n {
        1 => "1 run".to_string(),
        n => format!("{n} runs"),
    };
    Some(match b.ci.state {
        CiState::Failing => format!(
            "The release build for {} is failing: {}.",
            b.tag,
            crate::problems::names(&b.ci.failing)
        ),
        CiState::Running => format!("The release build for {} is still running.", b.tag),
        CiState::Passing => format!(
            "The release build for {} passed ({}).",
            b.tag,
            runs(b.ci.passing)
        ),
        CiState::None if new => format!("Nothing has run for {} yet.", b.tag),
        CiState::None => return None,
    })
}

#[cfg(test)]
#[path = "builds_tests.rs"]
mod tests;
