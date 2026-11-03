//! Looking things up on GitHub for Bit: Orbit stands between Bit's answerer
//! and GitHub. The answerer has no tools. Its answer can name up to
//! `MAX_LOOKUPS` things it wants to read (`Wanted`), and Orbit checks each
//! one (`check`): a kind of lookup in `KINDS`, in a connected repository,
//! with a well-formed job, branch, commit or path. Orbit then reads it (`read`) and
//! hands what it found to the answerer in its next message, marked as data.
//! An answerer looks things up at most `MAX_ROUNDS` times, then answers.
//!
//! Everything here only reads, and nothing else reads for Bit's answers:
//! every request goes through `get` or `get_text`, which only ever ask
//! GitHub with `GET` (a test reads this file to keep it that way). What
//! Orbit reads is someone else's words, such as a log, a workflow file or
//! a change: anything that looks like a credential is held back, never
//! shown or passed on, and each lookup is cut to `MAX_FOUND` characters.
use crate::{
    ci::short,
    logs,
    readiness::branch_path,
    text::{sentence, shorten},
    types::*,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most things an answer looks up at once.
pub const MAX_LOOKUPS: usize = 4;
/// The most times an answerer looks things up before it answers.
pub const MAX_ROUNDS: u32 = 2;
/// The most characters of what one lookup found that the answerer reads.
pub const MAX_FOUND: usize = 8000;
/// The runs a runs lookup lists, newest first, and those whose failed jobs
/// it names.
pub const RUNS: usize = 10;
const FAILED_RUNS: usize = 3;
/// The most lines of a job's log a log lookup gives.
pub const LOG_LINES: usize = 60;
/// The most failed jobs a runs lookup names in each run.
const FAILED_JOBS: usize = 10;
/// The most files a changes lookup lists, and the most characters of each
/// file's changes it gives.
pub const CHANGED_FILES: usize = 30;
pub const FILE_CHANGES: usize = 1500;
/// The most files GitHub sends in one page.
const PAGE: usize = 100;

/// What Orbit can look up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The latest workflow runs on a branch, or on a pull request's latest
    /// commit, with the jobs that failed in them.
    Runs,
    /// A job's failed step and the end of its log, up to its error.
    Log,
    /// A workflow file on the default branch.
    Workflow,
    /// A pull request's or a commit's changes, file by file.
    Changes,
}
pub const KINDS: [Kind; 4] = [Kind::Runs, Kind::Log, Kind::Workflow, Kind::Changes];
impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Runs => "runs",
            Kind::Log => "log",
            Kind::Workflow => "workflow",
            Kind::Changes => "changes",
        }
    }
}

/// A lookup as an answer names it. Every field is in the answer's form,
/// null when a kind doesn't use it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Wanted {
    pub kind: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub pr: Option<u64>,
    pub job: Option<u64>,
    pub path: Option<String>,
    pub commit: Option<String>,
}

/// A lookup Orbit checked, ready to read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lookup {
    pub kind: Kind,
    /// As it is connected.
    pub repo: String,
    /// The branch runs are read on, or a workflow file is read at: the
    /// default branch unless the answer named another for runs. Empty for
    /// a pull request's runs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    /// For a pull request's runs: its number, and its latest commit as
    /// Orbit last read it. For a pull request's changes, only its number;
    /// for a commit's changes, only the commit, as the answer named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<u64>,
    /// A workflow file's path, such as `.github/workflows/ci.yml`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}
impl Lookup {
    fn new(kind: Kind, repo: String) -> Self {
        Self {
            kind,
            repo,
            branch: String::new(),
            pr: None,
            sha: String::new(),
            job: None,
            path: String::new(),
        }
    }
    /// What it is, in plain words, such as "The log of job 5 in a/b".
    pub fn what(&self) -> String {
        let repo = &self.repo;
        match self.kind {
            Kind::Runs => match self.pr {
                Some(n) => format!("The latest runs on {repo}#{n}"),
                None => format!("The latest runs on {} in {repo}", self.branch),
            },
            Kind::Log => format!("The log of job {} in {repo}", self.job.unwrap_or(0)),
            Kind::Workflow => format!("{} in {repo}", self.path),
            Kind::Changes => match self.pr {
                Some(n) => format!("The changes in {repo}#{n}"),
                None => format!("The changes in {} in {repo}", short(&self.sha)),
            },
        }
    }
    /// Where GitHub shows it. A job's own page is known once it's read.
    pub fn url(&self) -> String {
        let repo = &self.repo;
        match self.kind {
            Kind::Runs => match self.pr {
                Some(n) => format!("https://github.com/{repo}/pull/{n}/checks"),
                None => format!(
                    "https://github.com/{repo}/actions?query=branch%3A{}",
                    branch_path(&self.branch).replace('/', "%2F")
                ),
            },
            Kind::Log => format!("https://github.com/{repo}/actions"),
            Kind::Workflow => format!(
                "https://github.com/{repo}/blob/{}/{}",
                branch_path(&self.branch),
                self.path
            ),
            Kind::Changes => match self.pr {
                Some(n) => format!("https://github.com/{repo}/pull/{n}/files"),
                None => format!("https://github.com/{repo}/commit/{}", self.sha),
            },
        }
    }
}

/// What a lookup found, as Bit's answer lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Looked {
    /// What it is, in plain words.
    pub what: String,
    /// Where GitHub shows it; empty for a lookup Orbit refused.
    pub url: String,
    /// Why Orbit didn't read it, couldn't, or held it back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
/// What a lookup found, with the text the answerer reads. The text is only
/// ever held while the question is answered, never saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// None for a lookup Orbit refused.
    pub lookup: Option<Lookup>,
    pub looked: Looked,
    pub text: String,
}
impl Found {
    /// A lookup that is about to be read, as Bit's answer shows it while it
    /// is.
    pub fn reading(l: &Lookup) -> Self {
        Self {
            lookup: Some(l.clone()),
            looked: Looked {
                what: l.what(),
                url: l.url(),
                error: None,
            },
            text: String::new(),
        }
    }
    /// A lookup Orbit didn't make, and why.
    pub fn refused(w: &Wanted, why: String) -> Self {
        let what = match (w.repository.as_deref(), w.kind.trim()) {
            (Some(repo), kind) if !repo.trim().is_empty() => {
                format!("A {} lookup in {}", shown(kind), shown(repo))
            }
            (_, kind) => format!("A {} lookup", shown(kind)),
        };
        Self {
            lookup: None,
            looked: Looked {
                what,
                url: String::new(),
                error: Some(why),
            },
            text: String::new(),
        }
    }
    /// A lookup that took too long.
    pub fn timed_out(l: &Lookup) -> Self {
        let mut found = Self::reading(l);
        found.looked.error = Some("GitHub took too long to answer.".into());
        found
    }
}
/// Text from an answer, as Orbit repeats it in a refusal.
fn shown(text: &str) -> String {
    let plain: String = text
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let plain = if plain.is_empty() {
        "nameless".to_string()
    } else {
        plain
    };
    clean(&plain, 60)
}
/// What Orbit says in place of a name or line that looks like it holds a
/// credential.
pub const HELD_TEXT: &str = "(held back, as it looks like it holds a credential)";
/// Text from GitHub or an answer, on one line and cut to `max` characters,
/// or `HELD_TEXT` when the whole of it looks like it holds a credential:
/// checked before it's cut, so a credential that crosses the cut can't
/// pass as a harmless piece.
pub fn clean(text: &str, max: usize) -> String {
    if !credential_kinds(text).is_empty() {
        return HELD_TEXT.into();
    }
    let plain: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    shorten(&plain, max)
}

/// The lookups an answer asks for, checked against what Orbit knows now,
/// at most `MAX_LOOKUPS`: those Orbit will read, without any it already
/// read for the question (`done`) or that the answer names twice, and the
/// ones it refused, with why, each once.
pub fn check_all(s: &Snapshot, wanted: &[Wanted], done: &[Found]) -> (Vec<Lookup>, Vec<Found>) {
    let mut lookups: Vec<Lookup> = vec![];
    let mut refused = vec![];
    for w in wanted.iter().take(MAX_LOOKUPS) {
        match check(s, w) {
            Ok(l) => {
                let again =
                    lookups.contains(&l) || done.iter().any(|f| f.lookup.as_ref() == Some(&l));
                if !again {
                    lookups.push(l);
                }
            }
            Err(why) => {
                let found = Found::refused(w, why);
                // Refused once is enough, in this round or an earlier one.
                let again = refused.contains(&found) || done.contains(&found);
                if !again {
                    refused.push(found);
                }
            }
        }
    }
    (lookups, refused)
}

/// A lookup an answer asks for, if Orbit makes it: one of `KINDS`, in a
/// connected repository, with what that kind needs.
pub fn check(s: &Snapshot, w: &Wanted) -> Result<Lookup, String> {
    let named = [&w.repository, &w.branch, &w.path, &w.commit];
    if named
        .iter()
        .filter_map(|t| t.as_deref())
        .chain([w.kind.as_str()])
        .any(|t| !credential_kinds(t).is_empty())
    {
        return Err("It names something that looks like a credential.".into());
    }
    let kind = KINDS
        .into_iter()
        .find(|k| k.key() == w.kind.trim())
        .ok_or_else(|| format!("Orbit doesn't look up \"{}\".", shown(&w.kind)))?;
    let name = w.repository.as_deref().map(str::trim).unwrap_or_default();
    let repo = s
        .settings
        .repositories
        .iter()
        .find(|r| same_repo(&r.name, name))
        .map(|r| r.name.clone())
        .ok_or_else(|| {
            if name.is_empty() {
                "It didn't name a repository.".to_string()
            } else {
                format!("{} isn't a connected repository.", shown(name))
            }
        })?;
    let default = || {
        s.ci.iter()
            .find(|c| same_repo(&c.repo, &repo))
            .map(|c| c.branch.clone())
            .filter(|b| !b.is_empty())
            .ok_or_else(|| format!("Orbit hasn't read {repo}'s default branch yet."))
    };
    let mut l = Lookup::new(kind, repo.clone());
    match kind {
        Kind::Runs => {
            let branch = w.branch.as_deref().map(str::trim).unwrap_or_default();
            match w.pr {
                Some(n) => {
                    l.sha = head_of(s, &repo, n)
                        .ok_or_else(|| format!("Orbit hasn't read {repo}#{n}."))?;
                    l.pr = Some(n);
                }
                None if branch.is_empty() => l.branch = default()?,
                None if valid_branch(branch) => l.branch = branch.to_string(),
                None => return Err(format!("\"{}\" isn't a branch name.", shown(branch))),
            }
        }
        Kind::Log => {
            l.job = Some(
                w.job
                    .filter(|j| *j > 0)
                    .ok_or("A log lookup needs a job's id.")?,
            );
        }
        Kind::Workflow => {
            let path = w.path.as_deref().map(str::trim).unwrap_or_default();
            if !valid_workflow(path) {
                return Err(format!(
                    "\"{}\" isn't a workflow file: Orbit reads only .yml or .yaml files in {}.",
                    shown(path),
                    crate::workflows::FOLDER
                ));
            }
            l.path = path.to_string();
            l.branch = default()?;
        }
        Kind::Changes => {
            let commit = w.commit.as_deref().map(str::trim).unwrap_or_default();
            match w.pr {
                Some(n) if known_pull(s, &repo, n) => l.pr = Some(n),
                Some(n) => return Err(format!("Orbit hasn't read {repo}#{n}.")),
                None if commit.is_empty() => {
                    return Err(
                        "A changes lookup needs a pull request's number or a commit.".into(),
                    )
                }
                None if valid_commit(commit) => l.sha = commit.to_ascii_lowercase(),
                None => return Err(format!("\"{}\" isn't a commit.", shown(commit))),
            }
        }
    }
    Ok(l)
}
/// Whether Orbit has read pull request `number`: one open now, or one it
/// found broke a default branch.
fn known_pull(s: &Snapshot, repo: &str, number: u64) -> bool {
    let broke = s
        .problems
        .iter()
        .filter(|p| same_repo(&p.repo, repo))
        .filter_map(|p| p.broke.as_ref())
        .any(|b| b.pulls.contains(&number) || b.pull == Some(number));
    head_of(s, repo, number).is_some() || broke
}
/// A commit as an answer names it: 7 to 40 hexadecimal digits, as the
/// summary shortens them.
fn valid_commit(sha: &str) -> bool {
    (7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit())
}
/// A pull request's latest commit, as Orbit last read it.
fn head_of(s: &Snapshot, repo: &str, number: u64) -> Option<String> {
    let synced = s
        .prs
        .iter()
        .find(|p| p.number == number && same_repo(&p.repo, repo))
        .map(|p| p.head_sha.clone());
    let ci = || {
        s.ci.iter()
            .find(|c| same_repo(&c.repo, repo))?
            .pull(number)
            .map(|p| p.ci.sha.clone())
    };
    synced.or_else(ci).filter(|sha| valid_sha(sha))
}
/// A branch name Orbit passes on: the characters Git allows that need no
/// quoting, with no `..`, and not starting with `-` or `/`.
fn valid_branch(name: &str) -> bool {
    (1..=200).contains(&name.len())
        && !name.starts_with(['-', '/'])
        && !name.ends_with('/')
        && !name.contains("..")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
}
/// A workflow file's path: a `.yml` or `.yaml` file right in
/// `.github/workflows`, with a plain name.
fn valid_workflow(path: &str) -> bool {
    let Some(file) = path
        .strip_prefix(crate::workflows::FOLDER)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    let stem = file
        .strip_suffix(".yml")
        .or_else(|| file.strip_suffix(".yaml"));
    stem.is_some_and(|stem| {
        (1..=100).contains(&stem.len())
            && !stem.starts_with('.')
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    })
}

/// Reads a lookup on GitHub. What it found is held back when it looks like
/// it holds a credential, and cut to `MAX_FOUND` characters otherwise.
pub async fn read(l: &Lookup) -> Found {
    let mut found = Found::reading(l);
    let read = match l.kind {
        Kind::Runs => runs(l).await,
        Kind::Log => log(l, &mut found.looked).await,
        Kind::Workflow => workflow(l).await,
        Kind::Changes => changes(l).await,
    };
    match read {
        Ok(Some(text)) if credential_kinds(&text).is_empty() => found.text = cut(&text),
        Ok(_) => found.looked.error = Some(HELD.into()),
        Err(e) => found.looked.error = Some(failed(&e)),
    }
    found
}
/// Why Orbit passes nothing on from a lookup that looks like it holds a
/// credential.
pub const HELD: &str = "Orbit held it back, as it looks like it holds a credential.";
/// What a lookup's error says, in plain words.
fn failed(error: &str) -> String {
    if error.contains("HTTP 404") || error.contains("HTTP 410") {
        "GitHub doesn't have it.".into()
    } else if logs::rate_limited(error) {
        "GitHub's rate limit stopped Orbit reading it. Ask again later.".into()
    } else if error.contains("HTTP 403") {
        "Orbit's GitHub sign-in can't read it.".into()
    } else {
        clean(&sentence(error), 200)
    }
}
/// What one lookup passes on, cut to `MAX_FOUND` characters.
fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_FOUND {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_FOUND).collect();
    format!("{kept}\n… (cut: Orbit passes on at most {MAX_FOUND} characters of each lookup)")
}

/// Asks GitHub to read `endpoint`, with `fields` as its query. The only
/// way this file reaches GitHub, with `get_text`.
async fn get(endpoint: &str, fields: &[(&str, String)]) -> Result<Value, String> {
    crate::github::api(endpoint, "GET", fields, None).await
}
/// Asks GitHub to read `endpoint`, whose answer is text, such as a log.
async fn get_text(endpoint: &str) -> Result<String, String> {
    crate::github::api_text(endpoint).await
}

/// A list of workflow runs as the answerer reads it: each run, and the jobs
/// that failed in the newest few that failed.
async fn runs(l: &Lookup) -> Result<Option<String>, String> {
    validate_repo(&l.repo)?;
    let mut fields = vec![("per_page", RUNS.to_string())];
    match l.pr {
        Some(_) => fields.push(("head_sha", l.sha.clone())),
        None => fields.push(("branch", l.branch.clone())),
    }
    let list = get(&format!("repos/{}/actions/runs", l.repo), &fields).await?;
    let runs: Vec<&Value> = list["workflow_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .take(RUNS)
        .collect();
    let mut jobs = vec![];
    for run in runs.iter().filter(|r| failed_run(r)).take(FAILED_RUNS) {
        let Some(id) = run["id"].as_u64() else {
            continue;
        };
        let endpoint = format!("repos/{}/actions/runs/{id}/jobs", l.repo);
        let fields = [
            ("filter", "latest".to_string()),
            ("per_page", "100".to_string()),
        ];
        jobs.push((id, get(&endpoint, &fields).await?));
    }
    Ok(Some(runs_text(l, &runs, &jobs)))
}
/// Whether a run ended in a way worth naming its failed jobs.
fn failed_run(run: &Value) -> bool {
    matches!(
        run["conclusion"].as_str(),
        Some("failure" | "timed_out" | "startup_failure")
    )
}
/// The runs, and the failed jobs of those in `jobs`, as lines of text.
pub fn runs_text(l: &Lookup, runs: &[&Value], jobs: &[(u64, Value)]) -> String {
    if runs.is_empty() {
        return format!("{}: none.", l.what());
    }
    let mut lines = vec![format!("{}, newest first:", l.what())];
    for run in runs {
        let id = run["id"].as_u64().unwrap_or(0);
        let state = match (run["status"].as_str(), run["conclusion"].as_str()) {
            (Some("completed"), Some(c)) => c.replace('_', " "),
            (Some(status), _) => status.replace('_', " "),
            _ => "unknown".into(),
        };
        lines.push(format!(
            "- Run {id}, \"{}\" ({}), at {}, made {}: {state}.",
            text(&run["name"]),
            text(&run["event"]),
            short(run["head_sha"].as_str().unwrap_or_default()),
            text(&run["created_at"]),
        ));
        let Some((_, list)) = jobs.iter().find(|(run, _)| *run == id) else {
            continue;
        };
        let failed: Vec<&Value> = list["jobs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|j| failed_run(j) || j["conclusion"] == "cancelled")
            .collect();
        for job in failed.iter().take(FAILED_JOBS) {
            let step = logs::failed_step(job)
                .map(|s| format!(", at the step \"{}\"", text(&s["name"])))
                .unwrap_or_default();
            lines.push(format!(
                "  - Job {}, \"{}\": {}{step}.",
                job["id"].as_u64().unwrap_or(0),
                text(&job["name"]),
                text(&job["conclusion"]).replace('_', " "),
            ));
        }
        if failed.len() > FAILED_JOBS {
            lines.push(format!(
                "  - And {} more failed jobs.",
                failed.len() - FAILED_JOBS
            ));
        }
    }
    lines.join("\n")
}
/// A text field from GitHub, on one line, as `clean` gives it.
fn text(v: &Value) -> String {
    clean(v.as_str().unwrap_or_default(), 200)
}

/// A job's failed step and the end of its log. None when they look like
/// they hold a credential. The job's own page goes on `looked`.
async fn log(l: &Lookup, looked: &mut Looked) -> Result<Option<String>, String> {
    validate_repo(&l.repo)?;
    let endpoint = format!("repos/{}/actions/jobs/{}", l.repo, l.job.unwrap_or(0));
    let job = get(&endpoint, &[]).await?;
    if let Some(url) = job["html_url"].as_str().filter(|u| page_of(u, &l.repo)) {
        looked.url = url.to_string();
    }
    let log = get_text(&format!("{endpoint}/logs")).await?;
    Ok(log_text(&job, &log))
}
/// Whether `url` is a page of `repo` on GitHub.
fn page_of(url: &str, repo: &str) -> bool {
    url.strip_prefix("https://github.com/")
        .and_then(|rest| rest.get(..repo.len() + 1))
        .is_some_and(|start| start.eq_ignore_ascii_case(&format!("{repo}/")))
}
/// A job and its log as the answerer reads them. None when the failed step
/// looks like it holds a credential.
pub fn log_text(job: &Value, log: &str) -> Option<String> {
    let (step, lines) = logs::longer(job, log, LOG_LINES)?;
    let state = match (job["status"].as_str(), job["conclusion"].as_str()) {
        (Some("completed"), Some(c)) => c.replace('_', " "),
        (Some(status), _) => status.replace('_', " "),
        _ => "unknown".into(),
    };
    let mut out = vec![format!(
        "Job {}, \"{}\", in run {} (\"{}\"), at {}: {state}.",
        job["id"].as_u64().unwrap_or(0),
        text(&job["name"]),
        job["run_id"].as_u64().unwrap_or(0),
        text(&job["workflow_name"]),
        short(job["head_sha"].as_str().unwrap_or_default()),
    )];
    if !step.is_empty() {
        out.push(format!("The step that failed it: {step}."));
    }
    if lines.is_empty() {
        out.push("Its log has no lines to show.".into());
    } else {
        out.push("The end of its log, up to its error:".into());
        out.extend(lines);
    }
    Some(out.join("\n"))
}

/// A workflow file's text on the default branch.
async fn workflow(l: &Lookup) -> Result<Option<String>, String> {
    validate_repo(&l.repo)?;
    let endpoint = format!("repos/{}/contents/{}", l.repo, l.path);
    let file = get(&endpoint, &[("ref", l.branch.clone())]).await?;
    file_text(&file).map(Some)
}
/// A file's text from GitHub's answer about it.
pub fn file_text(file: &Value) -> Result<String, String> {
    if file["type"] != "file" || file["encoding"] != "base64" {
        return Err("GitHub didn't send it as a file.".into());
    }
    let packed: String = file["content"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    let bytes = STANDARD
        .decode(packed)
        .map_err(|_| "GitHub sent a file Orbit couldn't read.".to_string())?;
    String::from_utf8(bytes).map_err(|_| "It isn't a text file.".to_string())
}

/// A pull request's or a commit's changes, file by file.
async fn changes(l: &Lookup) -> Result<Option<String>, String> {
    validate_repo(&l.repo)?;
    let per_page = [("per_page", PAGE.to_string())];
    match l.pr {
        Some(n) => {
            let endpoint = format!("repos/{}/pulls/{n}/files", l.repo);
            let files = get(&endpoint, &per_page).await?;
            let files: Vec<&Value> = files.as_array().into_iter().flatten().collect();
            Ok(Some(changes_text(l, None, &files)))
        }
        None => {
            let endpoint = format!("repos/{}/commits/{}", l.repo, l.sha);
            let commit = get(&endpoint, &per_page).await?;
            let files: Vec<&Value> = commit["files"].as_array().into_iter().flatten().collect();
            Ok(Some(changes_text(l, Some(&commit), &files)))
        }
    }
}
/// Changes as the answerer reads them: each file, how it changed and its
/// changes, cut to `FILE_CHANGES` characters. A file whose changes look
/// like they hold a credential is listed without them.
pub fn changes_text(l: &Lookup, commit: Option<&Value>, files: &[&Value]) -> String {
    let mut lines = vec![];
    match commit {
        Some(c) => {
            let message = c["commit"]["message"].as_str().unwrap_or_default();
            let title = message.lines().next().unwrap_or_default();
            lines.push(format!(
                "Commit {} in {}, \"{}\", by {}, file by file:",
                short(c["sha"].as_str().unwrap_or(&l.sha)),
                l.repo,
                text(&Value::from(title)),
                text(&c["commit"]["author"]["name"]),
            ));
        }
        None => lines.push(format!("{}, file by file:", l.what())),
    }
    if files.is_empty() {
        lines.push("No files changed.".into());
    }
    for file in files.iter().take(CHANGED_FILES) {
        let name = text(&file["filename"]);
        let status = text(&file["status"]).replace('_', " ");
        let moved = match file["previous_filename"].as_str() {
            Some(old) if status == "renamed" => format!("{} → ", text(&Value::from(old))),
            _ => String::new(),
        };
        lines.push(format!(
            "- {moved}{name} ({status}, +{} −{}):",
            file["additions"].as_u64().unwrap_or(0),
            file["deletions"].as_u64().unwrap_or(0),
        ));
        match file["patch"].as_str() {
            Some(patch) if !credential_kinds(patch).is_empty() => lines.push(
                "  Orbit left its changes out, as they look like they hold a credential.".into(),
            ),
            Some(patch) => {
                let kept: String = patch.chars().take(FILE_CHANGES).collect();
                lines.extend(kept.lines().map(|line| format!("  {line}")));
                if kept.len() < patch.len() {
                    lines.push(format!(
                        "  … (cut: Orbit gives at most {FILE_CHANGES} characters of each file)"
                    ));
                }
            }
            None => lines.push("  GitHub shows no text changes for it.".into()),
        }
    }
    // GitHub sends at most a page of files, so a full page may have more.
    if files.len() > CHANGED_FILES {
        let more = files.len() - CHANGED_FILES;
        lines.push(if files.len() >= PAGE {
            format!("And at least {more} more files.")
        } else {
            format!("And {more} more files.")
        });
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "broker_tests.rs"]
mod tests;
