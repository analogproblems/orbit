//! Why a CI job failed: the step that failed the job and the end of that
//! step's log, read from GitHub Actions once per job. What a log says is
//! someone else's words, so it is only ever shown as plain text, and a step
//! whose log matches a credential format is never kept.
use crate::{
    problems::{stamp, time, Problem, ProblemKind},
    types::*,
};
use chrono::{DateTime, Duration, SubsecRound, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

/// Jobs whose logs Orbit reads in one pass, at two requests each.
pub const PER_PASS: usize = 3;
/// The most lines an excerpt keeps, and the most characters per line.
pub const MAX_LINES: usize = 20;
pub const MAX_LINE: usize = 240;
/// The longest Orbit waits to read a log again that GitHub didn't answer
/// for.
pub const MAX_RETRY: Duration = Duration::hours(1);

/// Why one failing job failed, as Orbit read it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Failure {
    /// The job's page on GitHub, its check's link, which it is found by.
    pub url: String,
    pub check: String,
    /// The step that failed the job, such as "Run cargo test", when
    /// GitHub says.
    pub step: String,
    /// The last lines of that step's log up to its last error, without
    /// times or colours.
    pub lines: Vec<String>,
    /// The step's log looked like it held a secret, so Orbit dropped it.
    pub held: bool,
    /// Why Orbit can't say, such as a check that isn't a GitHub Actions
    /// job.
    pub error: Option<String>,
    pub read_at: String,
    /// Reads in a row that GitHub didn't answer for. Orbit tries again
    /// from `retry_at`.
    pub tries: u32,
    pub retry_at: Option<String>,
    /// GitHub's rate limit stopped the last try, so Orbit reads no log
    /// until `retry_at`.
    pub limited: bool,
    /// Why it looks like bad luck rather than the code, such as a lost
    /// runner or a network error, when it does (see `flaky`).
    pub flaky: Option<String>,
    /// The step couldn't upload an artifact as the account's artifact
    /// storage is full (see `QUOTA`), which only a person can change.
    pub quota: bool,
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

/// The failing checks on open problems whose logs Orbit should read now,
/// at most `PER_PASS`: first those it hasn't tried, default branches
/// first, then those due to be tried again, the longest waiting first, so
/// a log GitHub keeps failing on never holds up the others. None while
/// GitHub's rate limit is still waited out. Each check's repository, name
/// and link.
pub fn wanted(
    problems: &[Problem],
    read: &[Failure],
    now: DateTime<Utc>,
) -> Vec<(String, String, String)> {
    let waiting = |f: &Failure| {
        f.retry_at
            .as_deref()
            .and_then(time)
            .is_some_and(|at| at > now)
    };
    if read.iter().any(|f| f.limited && waiting(f)) {
        return vec![];
    }
    let mut open: Vec<&Problem> = problems.iter().filter(|p| p.is_open()).collect();
    open.sort_by_key(|p| p.kind != ProblemKind::DefaultBranch);
    let mut fresh: Vec<(String, String, String)> = vec![];
    let mut due: Vec<(DateTime<Utc>, (String, String, String))> = vec![];
    for p in open {
        for c in &p.failing {
            let listed = fresh.iter().any(|(_, _, url)| *url == c.url)
                || due.iter().any(|(_, (_, _, url))| *url == c.url);
            if c.url.is_empty() || listed {
                continue;
            }
            let job = (p.repo.clone(), c.name.clone(), c.url.clone());
            match read.iter().find(|f| f.url == c.url) {
                None => fresh.push(job),
                Some(f) => {
                    if let Some(at) = f.retry_at.as_deref().and_then(time) {
                        if at <= now {
                            due.push((at, job));
                        }
                    }
                }
            }
        }
    }
    due.sort_by_key(|(at, _)| *at);
    fresh
        .into_iter()
        .chain(due.into_iter().map(|(_, job)| job))
        .take(PER_PASS)
        .collect()
}
/// Keeps what Orbit read about a job, in place of an earlier try.
pub fn record(read: &mut Vec<Failure>, failure: Failure) {
    match read.iter_mut().find(|f| f.url == failure.url) {
        Some(f) => *f = failure,
        None => read.push(failure),
    }
}
/// Records that GitHub didn't answer for a job or its log, to try again
/// after a wait that doubles with each try, from a minute up to
/// `MAX_RETRY`, as GitHub asks when it doesn't say how long to wait.
pub fn retry_later(
    read: &mut Vec<Failure>,
    check: &str,
    url: &str,
    error: &str,
    now: DateTime<Utc>,
) {
    let tries = read
        .iter()
        .find(|f| f.url == url && f.retry_at.is_some())
        .map_or(0, |f| f.tries)
        + 1;
    let wait = Duration::minutes(1i64 << (tries - 1).min(6)).min(MAX_RETRY);
    record(
        read,
        Failure {
            url: url.into(),
            check: check.into(),
            error: Some(format!(
                "Orbit couldn't read its log yet: {}. It tries again later.",
                short(error)
            )),
            read_at: stamp(now),
            tries,
            retry_at: Some(stamp(now + wait)),
            limited: rate_limited(error),
            ..Default::default()
        },
    );
}
/// The first line of an error, without gh's name and cut short.
pub fn short(error: &str) -> String {
    let line = error.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    // Orbit names the program, and gh names itself.
    let line = match line.trim().split_once(": ") {
        Some((program, rest)) if !program.contains(' ') && program.ends_with("gh") => rest,
        _ => line.trim(),
    };
    let line = line
        .strip_prefix("gh: ")
        .unwrap_or(line)
        .trim_end_matches('.');
    let line = plain(line);
    if line.chars().count() > 120 {
        let cut: String = line.chars().take(119).collect();
        format!("{cut}…")
    } else {
        line
    }
}
/// Whether GitHub said Orbit asked too often. It says so with a 403 or a
/// 429.
/// Whether GitHub refused, or wouldn't show, what Orbit asked for: a 404,
/// or a 403 that isn't its rate limit, which waiting won't change.
pub fn refused(error: &str) -> bool {
    (error.contains("HTTP 403") && !rate_limited(error)) || error.contains("HTTP 404")
}
pub fn rate_limited(error: &str) -> bool {
    error.to_lowercase().contains("rate limit") || error.contains("HTTP 429")
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
/// failure that says so; for other errors, such as a rate limit, Orbit
/// tries again later (`retry_later`).
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
    let job = match crate::github::api(&endpoint, "GET", &[], None).await {
        Ok(v) => v,
        Err(e) => return lasting(e, failure),
    };
    let log = match crate::github::api_text(&format!("{endpoint}/logs")).await {
        Ok(log) => log,
        Err(e) => {
            // The job alone can still say it was bad luck.
            failure.flaky = flaky(failed_step(&job), &[]);
            return lasting(e, failure);
        }
    };
    Ok(understand(failure, &job, &log))
}
/// An error that won't go away by asking again, said on the failure, or
/// the error to try again later.
fn lasting(error: String, mut failure: Failure) -> Result<Failure, String> {
    failure.error = Some(
        if error.contains("HTTP 404") || error.contains("HTTP 410") {
            "GitHub no longer has this job's log.".into()
        } else if error.contains("HTTP 403") && !rate_limited(&error) {
            "Orbit's GitHub sign-in can't read this job's log.".into()
        } else {
            return Err(error);
        },
    );
    Ok(failure)
}
/// What GitHub's answer about a job and its log say about why it failed:
/// the step that failed the job and the end of what it logged. The whole
/// step is checked for secrets before anything is cut, so no part of one
/// is kept.
pub fn understand(mut failure: Failure, job: &Value, log: &str) -> Failure {
    let lines = parse(log);
    let step = failed_step(job);
    let name = step
        .and_then(|s| s["name"].as_str())
        .map(plain)
        .unwrap_or_default();
    // All the step's seconds are checked, and only its own lines shown.
    // Without the step's times, the whole log is checked and its last step
    // shown.
    let (checked, shown) = match step.and_then(|s| Some((s, section(&lines, s)?))) {
        Some((s, window)) => (window, own(window, s, job)),
        None => (&lines[..], last_step(&lines)),
    };
    // What the step printed, not the script GitHub echoed before running it.
    let own: Vec<&str> = shown.iter().map(|l| l.text.as_str()).collect();
    let said = output(&own);
    failure.flaky = flaky(step, &said);
    failure.quota = said.iter().any(|l| l.contains(QUOTA));
    let text: Vec<&str> = checked.iter().map(|l| l.text.as_str()).collect();
    if !credential_kinds(&format!("{name}\n{}", text.join("\n"))).is_empty() {
        failure.held = true;
        return failure;
    }
    failure.step = cut(&name);
    failure.lines = tail(shown).iter().map(|l| cut(l)).collect();
    failure
}

/// What a log says when the account's artifact storage is full, as
/// `actions/upload-artifact` puts it.
pub const QUOTA: &str = "Artifact storage quota has been hit";
/// What a log says when the runner went away.
const LOST: [&str; 4] = [
    "The runner has received a shutdown signal",
    "lost communication with the server",
    "The hosted runner encountered an error",
    "Runner connection is lost",
];
/// What a log says when the network or a registry failed.
const NETWORK: [&str; 14] = [
    "ECONNRESET",
    "ETIMEDOUT",
    "EAI_AGAIN",
    "ENOTFOUND",
    "socket hang up",
    "Connection reset by peer",
    "Could not resolve host",
    "Temporary failure in name resolution",
    "TLS handshake timeout",
    "i/o timeout",
    "502 Bad Gateway",
    "503 Service Unavailable",
    "504 Gateway Time",
    "toomanyrequests",
];
/// Why a failure looks like bad luck rather than the code, from the step
/// that failed the job and its own lines: no step failed, as when a runner is
/// lost; the runner went away; the job failed setting up, or getting the
/// code or tools ready, before any test ran; or a network or registry
/// error. A guess that costs at most one rerun.
pub fn flaky(step: Option<&Value>, lines: &[&str]) -> Option<String> {
    let Some(step) = step else {
        return Some("No step failed, as when a runner is lost.".into());
    };
    if LOST.iter().any(|p| lines.iter().any(|l| l.contains(p))) {
        return Some("The runner went away.".into());
    }
    let name = plain(step["name"].as_str().unwrap_or_default());
    if name == "Set up job" {
        return Some("It failed setting up the job.".into());
    }
    let ready = ["actions/checkout@", "actions/setup-", "actions/cache@"];
    if name
        .strip_prefix("Run ")
        .is_some_and(|action| ready.iter().any(|r| action.starts_with(r)))
    {
        return Some("It failed getting the code or tools ready, before any test ran.".into());
    }
    NETWORK
        .iter()
        .find(|p| lines.iter().any(|l| l.contains(*p)))
        .map(|p| format!("A network or registry error ({p})."))
}

/// The step most likely to have failed the job, from GitHub's answer about
/// it, which doesn't say which steps were allowed to fail: the first step
/// marked failed or timed out after which none of the workflow's own steps
/// succeeded, as a step that continued on error is usually followed by
/// ones that did; otherwise the last one marked failed.
pub fn failed_step(job: &Value) -> Option<&Value> {
    let steps: Vec<&Value> = job["steps"].as_array().into_iter().flatten().collect();
    let failed = |s: &Value| matches!(s["conclusion"].as_str(), Some("failure" | "timed_out"));
    // GitHub's own steps: setting up, the post steps and completing.
    let github = |s: &Value| {
        let name = s["name"].as_str().unwrap_or_default();
        name == "Set up job" || name == "Complete job" || name.starts_with("Post ")
    };
    let mut marked = steps.iter().enumerate().filter(|(_, s)| failed(s));
    let decisive = marked.clone().find(|(i, _)| {
        !steps[i + 1..]
            .iter()
            .any(|s| !github(s) && s["conclusion"] == "success")
    });
    decisive.or_else(|| marked.next_back()).map(|(_, s)| *s)
}

static TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z ?").unwrap());
static COLOUR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());

/// A line of a job's log: when GitHub logged it, and its text.
struct Line {
    at: Option<DateTime<Utc>>,
    text: String,
}
/// A job's log, line by line. A line without a time has the one before's.
fn parse(log: &str) -> Vec<Line> {
    let mut at = None;
    log.lines()
        .map(|raw| {
            let raw = raw.trim_start_matches('\u{feff}');
            if let Some(t) = TIME.find(raw).and_then(|m| time(m.as_str().trim_end())) {
                at = Some(t);
            }
            Line {
                at,
                text: plain(raw),
            }
        })
        .collect()
}
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
/// The lines GitHub logged while `step` ran, by the time on each line, to
/// the second, when GitHub gives the step's times.
fn section<'a>(lines: &'a [Line], step: &Value) -> Option<&'a [Line]> {
    let started = step["started_at"].as_str().and_then(time)?;
    let completed = step["completed_at"].as_str().and_then(time)?;
    let (from, to) = (
        started.trunc_subsecs(0),
        completed.trunc_subsecs(0) + Duration::seconds(1),
    );
    let inside = |l: &Line| l.at.is_some_and(|t| t >= from && t < to);
    let first = lines.iter().position(inside);
    let last = lines.iter().rposition(inside);
    Some(match (first, last) {
        (Some(first), Some(last)) => &lines[first..=last],
        _ => &[],
    })
}
/// A step's own lines in its seconds. GitHub gives step times only to the
/// second and marks no step's end in the log, and a composite action runs
/// several commands in one step, so a second another step that ran also
/// spans is left out. The one exception is the end of the failed step's
/// last second when its command failed: its lines up to its own last line,
/// `##[error]Process completed with exit code`, when no other command
/// starts before it.
fn own<'a>(window: &'a [Line], step: &Value, job: &Value) -> &'a [Line] {
    let second = |s: &Value, key: &str| s[key].as_str().and_then(time).map(|t| t.trunc_subsecs(0));
    let (Some(start), Some(end)) = (second(step, "started_at"), second(step, "completed_at"))
    else {
        return window;
    };
    let others: Vec<(DateTime<Utc>, DateTime<Utc>)> = job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| !std::ptr::eq(*s, step) && s["conclusion"] != "skipped")
        .filter_map(|s| Some((second(s, "started_at")?, second(s, "completed_at")?)))
        .collect();
    let shared = |t: DateTime<Utc>| others.iter().any(|(from, to)| *from <= t && t <= *to);
    let from = |t: DateTime<Utc>| {
        window
            .iter()
            .position(|l| l.at.is_some_and(|at| at >= t))
            .unwrap_or(window.len())
    };
    let first = if shared(start) {
        from(start + Duration::seconds(1))
    } else {
        0
    };
    let mut last = window.len();
    if shared(end) {
        let at = from(end).max(first);
        let mark = window[at..].iter().position(|l| {
            l.text.starts_with("##[group]Run ")
                || l.text
                    .starts_with("##[error]Process completed with exit code")
        });
        last = match mark {
            Some(m) if window[at + m].text.starts_with("##[error]") => at + m + 1,
            _ => at,
        };
    }
    &window[first..last.max(first)]
}
/// The last step of a whole log: from the last command before its last
/// error.
fn last_step(lines: &[Line]) -> &[Line] {
    let end = lines
        .iter()
        .rposition(|l| l.text.starts_with("##[error]"))
        .unwrap_or(lines.len());
    let start = lines[..end]
        .iter()
        .rposition(|l| l.text.starts_with("##[group]Run "));
    &lines[start.unwrap_or(0)..]
}
/// The end of a whole log's last step, as Orbit shows it when it doesn't
/// know the failed step's times: each line cut to `MAX_LINE` characters.
pub fn excerpt(log: &str) -> Vec<String> {
    tail(last_step(&parse(log)))
        .iter()
        .map(|l| cut(l))
        .collect()
}
/// The end of a step's log: its lines up to its last error, without the
/// commands GitHub echoes, at most `MAX_LINES`. GitHub echoes each
/// command in a `##[group]Run` group and marks an error with
/// `##[error]`. A step with no error gives its end.
fn tail(lines: &[Line]) -> Vec<String> {
    let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    let end = text
        .iter()
        .rposition(|l| l.starts_with("##[error]"))
        .map_or(text.len(), |e| e + 1);
    let mut kept: Vec<String> = vec![];
    for line in output(&text[..end]) {
        if line.trim().is_empty()
            || line.starts_with("##[group]")
            || line.starts_with("##[endgroup]")
        {
            continue;
        }
        kept.push(match line.strip_prefix("##[error]") {
            Some(rest) => format!("Error: {rest}"),
            None => line
                .strip_prefix("##[warning]")
                .map_or_else(|| line.to_string(), |rest| format!("Warning: {rest}")),
        });
    }
    if kept.len() > MAX_LINES {
        kept.drain(..kept.len() - MAX_LINES);
    }
    kept
}
/// A log's lines without the commands GitHub echoes before running them:
/// each `##[group]Run` group up to its end, unless another group starts
/// first. What a command printed stays.
fn output<'a>(text: &[&'a str]) -> Vec<&'a str> {
    let mut kept = vec![];
    let mut i = 0;
    while i < text.len() {
        let line = text[i];
        if line.starts_with("##[group]Run ") {
            let close = text[i + 1..]
                .iter()
                .position(|l| l.starts_with("##[endgroup]") || l.starts_with("##[group]"));
            i += match close {
                Some(c) if text[i + 1 + c].starts_with("##[endgroup]") => c + 2,
                _ => 1,
            };
            continue;
        }
        kept.push(line);
        i += 1;
    }
    kept
}

#[cfg(test)]
#[path = "logs_tests.rs"]
mod tests;
