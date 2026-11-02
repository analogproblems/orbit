//! `actionlint` and `zizmor`, when they are installed, run on the same
//! workflow files as Orbit's own checks (`crate::risks`), at each new
//! commit on the default branch, and what they find joins those risks
//! (`merge`), in the same problem and issue.
//!
//! Each runs in a folder of its own that holds only those files, with no
//! configuration and no GitHub sign-in, and zizmor with `--offline`, so
//! neither reaches the network or reads anything else of the repository's.
//! actionlint's own use of shellcheck and pyflakes is off. Orbit keeps a
//! finding's rule, message, line and job, not the lines it quotes.
//! A tool that isn't installed, fails, can't read a file or takes too long
//! leaves what it found before as it was (see `crate::problems::see_risks`).
use crate::{
    cli::Launcher,
    risks::{normalize, sort, Risk, RiskKind, MAX_PER_FILE, MAX_SUBJECT},
    security::Severity,
    text::shorten,
    workflows::{Setup, ToolRun},
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};

/// How long a tool may take on a commit's files.
#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(test)]
const TIMEOUT: Duration = Duration::from_secs(3);
/// How old a folder a run left must be before another run removes it.
const LEFT: Duration = Duration::from_secs(60 * 60);
/// The most output Orbit reads from a tool, and of its standard error.
const MAX_OUTPUT: usize = 8 * 1024 * 1024;
const MAX_STDERR: usize = 64 * 1024;
/// The longest of a tool's own words Orbit keeps.
const MAX_DETAIL: usize = 200;
/// zizmor's audits about what Orbit's own checks look at, by their own
/// rules: pins (GitHub's own actions and yours need none), permissions, and
/// triggers, which Orbit counts only when they check out a pull request's
/// code. zizmor's `template-injection` stays, except where Orbit found the
/// same text on the same line.
const ZIZMOR_OWN: [&str; 3] = [
    "unpinned-uses",
    "excessive-permissions",
    "dangerous-triggers",
];
/// GitHub sign-ins and configuration the tools could pick up from Orbit's
/// own surroundings.
const UNSET: [&str; 8] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "ZIZMOR_GITHUB_TOKEN",
    "ZIZMOR_CONFIG",
    "GH_HOST",
    "SHELLCHECK_OPTS",
];

/// How to start the tools; tests start fakes.
#[derive(Clone, Debug)]
pub struct Tools {
    pub actionlint: Launcher,
    pub zizmor: Launcher,
}
impl Default for Tools {
    fn default() -> Self {
        // A test that sets no fakes runs neither, whatever is installed.
        let name = |tool: &str| {
            if cfg!(test) {
                format!("{tool}-is-not-available-in-tests")
            } else {
                tool.to_string()
            }
        };
        Self {
            actionlint: Launcher::new(&name("actionlint")),
            zizmor: Launcher::new(&name("zizmor")),
        }
    }
}

/// One file to check: its path, such as `.github/workflows/ci.yml`, and
/// its text.
pub type File = (String, String);

/// What one installed tool did: what it found, or why it couldn't check
/// any of the files, and each file it couldn't check while it checked
/// others, with why.
#[derive(Debug)]
pub struct Ran {
    pub tool: RiskKind,
    pub found: Result<Vec<Finding>, String>,
    pub missed: Vec<(String, String)>,
}
/// One thing a tool found, and the text it is about when that is text put
/// in a script, to tell whether Orbit found it too.
#[derive(Debug, Default)]
pub struct Finding {
    pub risk: Risk,
    script: String,
}

/// Runs each tool that is installed on `files`, in a folder of its own
/// under `dir`, removed after. actionlint checks them all at once and
/// reports on each; zizmor checks one at a time, as a file it can't read
/// would otherwise be skipped with only a warning.
pub async fn run(tools: &Tools, dir: &Path, files: &[File]) -> Vec<Ran> {
    let files: Vec<&File> = files.iter().filter(|(p, _)| name(p).is_some()).collect();
    if files.is_empty() {
        return vec![];
    }
    sweep(dir, LEFT);
    let known: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    let mut ran = vec![];
    for (tool, launcher) in [
        (RiskKind::Actionlint, &tools.actionlint),
        (RiskKind::Zizmor, &tools.zizmor),
    ] {
        let folder = match Folder::new(dir, tool, &files) {
            Ok(folder) => folder,
            Err(e) => {
                ran.push(Ran {
                    tool,
                    found: Err(e),
                    missed: vec![],
                });
                continue;
            }
        };
        let batches: Vec<Vec<&str>> = match tool {
            RiskKind::Actionlint => vec![known.clone()],
            _ => known.iter().map(|p| vec![*p]).collect(),
        };
        let deadline = tokio::time::Instant::now() + TIMEOUT;
        let (mut found, mut missed) = (vec![], vec![]);
        let mut installed = true;
        for batch in batches {
            match call(tool, launcher, &folder.0, &batch, deadline).await {
                Said::Missing => {
                    installed = false;
                    break;
                }
                Said::Failed(why) => {
                    missed.extend(batch.iter().map(|p| (p.to_string(), why.clone())))
                }
                Said::Found(v) => found.extend(match tool {
                    RiskKind::Actionlint => actionlint(&v, &known),
                    _ => zizmor(&v, &known),
                }),
            }
        }
        if !installed {
            continue;
        }
        // One that couldn't check any of them failed.
        ran.push(if missed.len() == known.len() {
            Ran {
                tool,
                found: Err(missed.swap_remove(0).1),
                missed: vec![],
            }
        } else {
            Ran {
                tool,
                found: Ok(found),
                missed,
            }
        });
    }
    ran
}

/// What a tool said about some files.
enum Said {
    /// It isn't installed.
    Missing,
    Failed(String),
    Found(Value),
}

/// Runs a tool on `paths` in `folder`, until `deadline`.
async fn call(
    tool: RiskKind,
    launcher: &Launcher,
    folder: &Path,
    paths: &[&str],
    deadline: tokio::time::Instant,
) -> Said {
    let mut cmd = launcher.command();
    match tool {
        RiskKind::Actionlint => cmd.args([
            "-format",
            "{{json .}}",
            "-no-color",
            "-shellcheck=",
            "-pyflakes=",
        ]),
        _ => cmd.args([
            "--offline",
            "--no-config",
            "--strict-collection",
            "--no-progress",
            "--color",
            "never",
            "--format",
            "json-v1",
            "--no-exit-codes",
            "--min-severity",
            "low",
        ]),
    };
    cmd.args(paths)
        .current_dir(folder)
        .env("NO_COLOR", "1")
        .env("RUST_BACKTRACE", "0")
        .env("RUST_LIB_BACKTRACE", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for key in UNSET {
        cmd.env_remove(key);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Said::Missing,
        Err(e) => return Said::Failed(format!("it didn't start: {e}")),
    };
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let read = async {
        tokio::join!(
            capped(stdout, MAX_OUTPUT),
            capped(stderr, MAX_STDERR),
            child.wait()
        )
    };
    // Dropping the child at a timeout stops it.
    let Ok(((out, more), (err, _), status)) = tokio::time::timeout_at(deadline, read).await else {
        return Said::Failed(format!("it took longer than {} seconds", TIMEOUT.as_secs()));
    };
    let Ok(status) = status else {
        return Said::Failed("Orbit couldn't tell how it ended".into());
    };
    // actionlint ends with 1 when it found something.
    let code = status.code();
    if !(code == Some(0) || (tool == RiskKind::Actionlint && code == Some(1))) {
        return Said::Failed(match failure(&String::from_utf8_lossy(&err)) {
            said if said.is_empty() => "it failed".into(),
            said => format!("it failed: {said}"),
        });
    }
    if more {
        return Said::Failed("it said too much to read".into());
    }
    let text = String::from_utf8_lossy(&out);
    match text.trim() {
        "" => Said::Found(Value::Array(vec![])),
        text => serde_json::from_str(text)
            .map(Said::Found)
            .unwrap_or_else(|_| Said::Failed("Orbit couldn't read what it said".into())),
    }
}

/// Reads all of a stream, keeping at most `cap` bytes, and says whether
/// there was more. It reads on past `cap`, so the tool never waits on it.
async fn capped<R: AsyncRead + Unpin>(stream: Option<R>, cap: usize) -> (Vec<u8>, bool) {
    let (mut kept, mut more) = (vec![], false);
    let Some(mut stream) = stream else {
        return (kept, more);
    };
    let mut buf = [0u8; 8192];
    while let Ok(n @ 1..) = stream.read(&mut buf).await {
        let room = cap.saturating_sub(kept.len());
        kept.extend_from_slice(&buf[..n.min(room)]);
        more |= n > room;
    }
    (kept, more)
}

/// Why a tool says it failed, from its standard error: its own lines, not
/// its log lines or a backtrace, on one line.
fn failure(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with("Stack backtrace"))
        .filter(|l| {
            let log = ["INFO ", "WARN ", "DEBUG ", "TRACE "]
                .iter()
                .any(|p| l.starts_with(p));
            !l.is_empty() && !log && *l != "Caused by:"
        })
        .map(|l| {
            // A numbered cause, such as "0: input does not match".
            l.split_once(": ")
                .filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit()))
                .map_or(l, |(_, rest)| rest)
        })
        .collect();
    words(&lines.join("; "))
}

/// A file's name when it is safe to write in a folder: one plain name in
/// `.github/workflows`.
fn name(path: &str) -> Option<&str> {
    let name = path.strip_prefix(".github/workflows/")?;
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        });
    plain.then_some(name)
}

/// Folders older than `age` under `dir`, which a run left when Orbit
/// stopped during it.
fn sweep(dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let modified = entry.metadata().and_then(|m| m.modified()).ok();
        let old = modified
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|a| a >= age);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
/// A folder of its own for one run, holding only the files, removed when
/// it ends.
struct Folder(PathBuf);
impl Folder {
    fn new(dir: &Path, tool: RiskKind, files: &[&File]) -> Result<Self, String> {
        let path = dir.join(format!("{}-{}", tool.key(), uuid::Uuid::new_v4().simple()));
        let workflows = path.join(".github").join("workflows");
        std::fs::create_dir_all(&workflows)
            .map_err(|e| format!("Orbit couldn't make a folder: {e}"))?;
        let folder = Self(path);
        for (path, text) in files {
            let Some(name) = name(path) else { continue };
            std::fs::write(workflows.join(name), text)
                .map_err(|e| format!("Orbit couldn't write {name}: {e}"))?;
        }
        Ok(folder)
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The path of one of `known` that a tool names, as it may write it.
fn known_path(path: &str, known: &[&str]) -> Option<String> {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./");
    known.iter().find(|k| **k == path).map(|k| k.to_string())
}
/// A tool's rule, kept only when it is letters, digits and `-`.
fn rule(text: &str) -> Option<String> {
    let ok = !text.is_empty()
        && text.len() <= 64
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    ok.then(|| text.to_string())
}
/// A tool's own words, on one line.
fn words(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    shorten(text.trim(), MAX_DETAIL)
}

/// What `actionlint -format '{{json .}}'` said: a list of errors, each with
/// its `message`, `filepath`, `line` and `kind`. They are low: mistakes in
/// a workflow rather than ways in.
fn actionlint(v: &Value, known: &[&str]) -> Vec<Finding> {
    let mut found = vec![];
    for e in v.as_array().into_iter().flatten() {
        let Some(path) = e["filepath"].as_str().and_then(|p| known_path(p, known)) else {
            continue;
        };
        let message = words(e["message"].as_str().unwrap_or_default());
        if message.is_empty() {
            continue;
        }
        let script = if message.contains("potentially untrusted") {
            message.clone()
        } else {
            String::new()
        };
        found.push(Finding {
            risk: Risk {
                kind: RiskKind::Actionlint,
                severity: Severity::Low,
                path,
                line: e["line"].as_u64().unwrap_or(0) as usize,
                subject: rule(e["kind"].as_str().unwrap_or_default()).unwrap_or("other".into()),
                detail: message,
                ..Default::default()
            },
            script,
        });
    }
    found
}

/// What `zizmor --format json-v1` said: a list of findings, each with its
/// audit (`ident`), its words (`desc`), its severity, and where it is: the
/// primary location's file, line (from 0) and route, which names the job,
/// and the part of the line it is about, such as an expression, which goes
/// in its words so two in one job are two.
/// Findings an ignore comment in the file hides, and the audits in
/// `ZIZMOR_OWN`, are left out.
fn zizmor(v: &Value, known: &[&str]) -> Vec<Finding> {
    let mut found = vec![];
    for f in v.as_array().into_iter().flatten() {
        let Some(ident) = f["ident"].as_str().and_then(rule) else {
            continue;
        };
        if f["ignored"] == true || ZIZMOR_OWN.contains(&ident.as_str()) {
            continue;
        }
        let severity = match f["determinations"]["severity"].as_str() {
            Some("High") => Severity::High,
            Some("Medium") => Severity::Medium,
            Some("Low") => Severity::Low,
            _ => continue,
        };
        let locations = f["locations"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let primary = locations
            .iter()
            .find(|l| l["symbolic"]["kind"] == "Primary")
            .or(locations.first());
        let Some(at) = primary else { continue };
        let path = at["symbolic"]["key"]["Local"]["verbatim_path"].as_str();
        let Some(path) = path.and_then(|p| known_path(p, known)) else {
            continue;
        };
        let route = at["symbolic"]["route"]["route"].as_array();
        let route = route.map(Vec::as_slice).unwrap_or_default();
        let job = match route {
            [first, job, ..] if first["Key"] == "jobs" => job["Key"].as_str().unwrap_or_default(),
            _ => "",
        };
        let row = at["concrete"]["location"]["start_point"]["row"].as_u64();
        // The part of the line it is about, such as the expression put in
        // a script, names it; the whole step only tells Orbit's own apart.
        let part = at["symbolic"]["feature_kind"]["Subfeature"]["fragment"]["Raw"].as_str();
        let desc = words(f["desc"].as_str().unwrap_or_default());
        let detail = match part.map(words).filter(|p| !p.is_empty()) {
            Some(part) => words(&format!("{desc} ({part})")),
            None => desc,
        };
        let script = if ident == "template-injection" {
            normalize(
                part.or(at["concrete"]["feature"].as_str())
                    .unwrap_or_default(),
            )
        } else {
            String::new()
        };
        found.push(Finding {
            risk: Risk {
                kind: RiskKind::Zizmor,
                severity,
                path,
                line: row.map_or(0, |r| r as usize + 1),
                job: shorten(job, MAX_SUBJECT),
                detail,
                subject: ident,
                ..Default::default()
            },
            script,
        });
    }
    found
}

/// Adds what the tools found to each file's risks, once each, keeping the
/// most severe `MAX_PER_FILE` and counting the rest in `more`, and records
/// which tools ran, and which files one couldn't check. What a tool found about text Orbit already found put
/// in a script on the same line is left out, as Orbit's own says it.
pub fn merge(setup: &mut Setup, ran: Vec<Ran>) {
    setup.tools = ran
        .iter()
        .map(|r| ToolRun {
            tool: r.tool,
            error: r.found.as_ref().err().cloned().unwrap_or_default(),
        })
        .collect();
    for ran in ran {
        for (path, why) in &ran.missed {
            if let Some(w) = setup.workflows.iter_mut().find(|w| w.path == *path) {
                w.missed.push(ToolRun {
                    tool: ran.tool,
                    error: why.clone(),
                });
            }
        }
        let Ok(found) = ran.found else { continue };
        let mut seen = HashSet::new();
        for Finding { risk, script } in found {
            let Some(w) = setup
                .workflows
                .iter_mut()
                .find(|w| w.checked && w.path == risk.path)
            else {
                continue;
            };
            let orbits = !script.is_empty()
                && w.risks.iter().any(|r| {
                    r.kind == RiskKind::Injection
                        && r.line == risk.line
                        && script.contains(&r.subject)
                });
            let key = (
                risk.path.clone(),
                risk.job.clone(),
                risk.subject.clone(),
                risk.detail.clone(),
            );
            if orbits || !seen.insert(key) {
                continue;
            }
            w.risks.push(risk);
        }
    }
    for w in &mut setup.workflows {
        sort(&mut w.risks);
        let over = w.risks.len().saturating_sub(MAX_PER_FILE);
        w.risks.truncate(MAX_PER_FILE);
        w.more = w
            .more
            .saturating_add(u32::try_from(over).unwrap_or(u32::MAX));
    }
}

#[cfg(test)]
#[path = "lint_tests.rs"]
pub(crate) mod tests;
