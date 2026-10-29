//! Why a CI job failed: the step that failed and the end of that step's
//! log, read from GitHub Actions once per job. What a log says is someone
//! else's words, so it is only ever shown as plain text, and an excerpt
//! that matches a credential format is never kept.
use crate::{problems::Problem, types::*};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Jobs whose logs Orbit reads in one pass, at two requests each.
pub const PER_PASS: usize = 3;
/// The most lines an excerpt keeps, and the most characters per line.
pub const MAX_LINES: usize = 20;
pub const MAX_LINE: usize = 240;

/// Why one failing job failed, as Orbit read it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Failure {
    /// The job's page on GitHub, its check's link, which it is found by.
    pub url: String,
    pub check: String,
    /// The step that failed, such as "Run cargo test", when GitHub says.
    pub step: String,
    /// The last lines of that step's log up to its error, without times
    /// or colours.
    pub lines: Vec<String>,
    /// The excerpt looked like it held a secret, so Orbit dropped it.
    pub held: bool,
    /// Why Orbit can't say, such as a check that isn't a GitHub Actions
    /// job.
    pub error: Option<String>,
    pub read_at: String,
}

/// The job a check's link names, when it is a GitHub Actions job in
/// `repo`: `https://github.com/<repo>/actions/runs/<run>/job/<job>`.
pub fn job_of(url: &str, repo: &str) -> Option<u64> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (owner, rest) = rest.split_once('/')?;
    let (name, rest) = rest.split_once('/')?;
    if !same_repo(&format!("{owner}/{name}"), repo) {
        return None;
    }
    let parts: Vec<&str> = rest.split(['?', '#']).next()?.split('/').collect();
    match parts.as_slice() {
        ["actions", "runs", run, "job", job]
            if run.chars().all(|c| c.is_ascii_digit()) && !run.is_empty() =>
        {
            job.parse()
                .ok()
                .filter(|_| job.chars().all(|c| c.is_ascii_digit()))
        }
        _ => None,
    }
}

/// The failing checks on open problems whose logs Orbit hasn't read,
/// default branches first, at most `PER_PASS`: each check's repository,
/// name and link.
pub fn wanted(problems: &[Problem], read: &[Failure]) -> Vec<(String, String, String)> {
    let mut open: Vec<&Problem> = problems.iter().filter(|p| p.is_open()).collect();
    open.sort_by_key(|p| p.kind != crate::problems::ProblemKind::DefaultBranch);
    let mut out: Vec<(String, String, String)> = vec![];
    for p in open {
        for c in &p.failing {
            let known = c.url.is_empty()
                || read.iter().any(|f| f.url == c.url)
                || out.iter().any(|(_, _, url)| *url == c.url);
            if !known && out.len() < PER_PASS {
                out.push((p.repo.clone(), c.name.clone(), c.url.clone()));
            }
        }
    }
    out
}
/// Keeps what Orbit read about the jobs still failing on open problems.
/// Returns whether any went.
pub fn prune(read: &mut Vec<Failure>, problems: &[Problem]) -> bool {
    let before = read.len();
    read.retain(|f| {
        problems
            .iter()
            .filter(|p| p.is_open())
            .any(|p| p.failing.iter().any(|c| c.url == f.url))
    });
    read.len() != before
}

/// Reads why a failing check failed. A check that isn't a GitHub Actions
/// job, or whose job or log GitHub no longer has or won't show, gives a
/// failure that says so; other errors are tried again on a later pass.
pub async fn read(repo: &str, check: &str, url: &str) -> Result<Failure, String> {
    validate_repo(repo)?;
    let mut failure = Failure {
        url: url.to_string(),
        check: check.to_string(),
        read_at: now(),
        ..Default::default()
    };
    let Some(job) = job_of(url, repo) else {
        failure.error = Some("Orbit can only read the logs of GitHub Actions jobs.".into());
        return Ok(failure);
    };
    let endpoint = format!("repos/{repo}/actions/jobs/{job}");
    let steps = match crate::github::api(&endpoint, "GET", &[], None).await {
        Ok(v) => v,
        Err(e) => return lasting(e, failure),
    };
    let log = match crate::github::api_text(&format!("{endpoint}/logs")).await {
        Ok(log) => log,
        Err(e) => return lasting(e, failure),
    };
    let step = failed_step(&steps);
    let lines = region(&log);
    // Checked before lines are cut, so no part of a secret is shown.
    if credential_kinds(&format!("{step}\n{}", lines.join("\n"))).is_empty() {
        failure.step = cut(&step);
        failure.lines = lines.iter().map(|l| cut(l)).collect();
    } else {
        failure.held = true;
    }
    Ok(failure)
}
/// An error that won't go away by asking again, said on the failure, or
/// the error to try again later.
fn lasting(error: String, mut failure: Failure) -> Result<Failure, String> {
    failure.error = Some(
        if error.contains("HTTP 404") || error.contains("HTTP 410") {
            "GitHub no longer has this job's log.".into()
        } else if error.contains("HTTP 403") {
            "Orbit's GitHub sign-in can't read this job's log.".into()
        } else {
            return Err(error);
        },
    );
    Ok(failure)
}

/// The first step GitHub marks failed, from its answer about a job.
pub fn failed_step(job: &serde_json::Value) -> String {
    let steps = job["steps"].as_array().into_iter().flatten();
    steps
        .filter(|s| matches!(s["conclusion"].as_str(), Some("failure" | "timed_out")))
        .filter_map(|s| s["name"].as_str())
        .map(plain)
        .next()
        .unwrap_or_default()
}

static TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z ?").unwrap());
static COLOUR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());

/// A log line as text: without its time, colours or other control
/// characters.
fn plain(line: &str) -> String {
    let line = line.trim_start_matches('\u{feff}');
    let line = TIME.replace(line, "");
    let line = COLOUR.replace_all(&line, "");
    let line: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    line.trim_end().to_string()
}
/// A line cut to `MAX_LINE` characters.
fn cut(line: &str) -> String {
    if line.chars().count() > MAX_LINE {
        let cut: String = line.chars().take(MAX_LINE - 1).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}
/// The end of the step that failed, from a job's whole log, as Orbit
/// shows it: each line cut to `MAX_LINE` characters.
pub fn excerpt(log: &str) -> Vec<String> {
    region(log).iter().map(|l| cut(l)).collect()
}
/// The end of the step that failed, from a job's whole log: the lines
/// after the step's command up to its first error, and the errors, at
/// most `MAX_LINES`. GitHub marks a step's command with `##[group]Run`
/// and an error with `##[error]`. A log with no error gives its end.
fn region(log: &str) -> Vec<String> {
    let lines: Vec<String> = log.lines().map(plain).collect();
    let error = lines.iter().position(|l| l.starts_with("##[error]"));
    let end = error.unwrap_or(lines.len());
    let group = lines[..end]
        .iter()
        .rposition(|l| l.starts_with("##[group]Run "));
    let start = group.map_or(0, |g| {
        lines[g..end]
            .iter()
            .position(|l| l.starts_with("##[endgroup]"))
            .map_or(g + 1, |e| g + e + 1)
    });
    let errors = error.map_or(0, |e| {
        lines[e..]
            .iter()
            .take_while(|l| l.starts_with("##[error]"))
            .count()
    });
    let mut kept: Vec<String> = lines[start..end + errors]
        .iter()
        .filter(|l| {
            !l.trim().is_empty() && !l.starts_with("##[group]") && !l.starts_with("##[endgroup]")
        })
        .map(|l| match l.strip_prefix("##[error]") {
            Some(rest) => format!("Error: {rest}"),
            None => l
                .strip_prefix("##[warning]")
                .map_or_else(|| l.clone(), |rest| format!("Warning: {rest}")),
        })
        .collect();
    if kept.len() > MAX_LINES {
        kept.drain(..kept.len() - MAX_LINES);
    }
    kept
}

#[cfg(test)]
#[path = "logs_tests.rs"]
mod tests;
