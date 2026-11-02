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
//! A tool that isn't installed, fails or takes too long leaves what it
//! found before as it was (see `crate::problems::see_risks`).
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

/// How long a tool may take on a commit's files.
#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(test)]
const TIMEOUT: Duration = Duration::from_secs(3);
/// How old a folder a run left must be before another run removes it.
const LEFT: Duration = Duration::from_secs(60 * 60);
/// The most output Orbit reads from a tool.
const MAX_OUTPUT: usize = 8 * 1024 * 1024;
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
/// the files.
#[derive(Debug)]
pub struct Ran {
    pub tool: RiskKind,
    pub found: Result<Vec<Finding>, String>,
}
/// One thing a tool found, and the text it is about when that is text put
/// in a script, to tell whether Orbit found it too.
#[derive(Debug, Default)]
pub struct Finding {
    pub risk: Risk,
    script: String,
}

/// Runs each tool that is installed on `files`, in a folder of its own
/// under `dir`, removed after.
pub async fn run(tools: &Tools, dir: &Path, files: &[File]) -> Vec<Ran> {
    let files: Vec<&File> = files.iter().filter(|(p, _)| name(p).is_some()).collect();
    if files.is_empty() {
        return vec![];
    }
    sweep(dir, LEFT);
    let mut ran = vec![];
    for (tool, launcher) in [
        (RiskKind::Actionlint, &tools.actionlint),
        (RiskKind::Zizmor, &tools.zizmor),
    ] {
        let found = match one(tool, launcher, dir, &files).await {
            Ok(Some(found)) => Ok(found),
            // Not installed.
            Ok(None) => continue,
            Err(e) => Err(e),
        };
        ran.push(Ran { tool, found });
    }
    ran
}

/// Runs one tool, or says it isn't installed.
async fn one(
    tool: RiskKind,
    launcher: &Launcher,
    dir: &Path,
    files: &[&File],
) -> Result<Option<Vec<Finding>>, String> {
    let folder = Folder::new(dir, tool)?;
    let workflows = folder.0.join(".github").join("workflows");
    std::fs::create_dir_all(&workflows)
        .map_err(|e| format!("Orbit couldn't make a folder: {e}"))?;
    let mut paths = vec![];
    for (path, text) in files {
        let Some(name) = name(path) else { continue };
        std::fs::write(workflows.join(name), text)
            .map_err(|e| format!("Orbit couldn't write {name}: {e}"))?;
        paths.push(format!(".github/workflows/{name}"));
    }
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
    cmd.args(&paths)
        .current_dir(&folder.0)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for key in UNSET {
        cmd.env_remove(key);
    }
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("it didn't start: {e}")),
    };
    let out = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| format!("it took longer than {} seconds", TIMEOUT.as_secs()))?
        .map_err(|e| format!("Orbit couldn't read what it said: {e}"))?;
    // actionlint ends with 1 when it found something.
    let code = out.status.code();
    let ok = code == Some(0) || (tool == RiskKind::Actionlint && code == Some(1));
    if !ok {
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.lines().rev().find(|l| !l.trim().is_empty());
        return Err(match said {
            Some(said) => format!("it failed: {}", shorten(said.trim(), MAX_DETAIL)),
            None => "it failed".into(),
        });
    }
    if out.stdout.len() > MAX_OUTPUT {
        return Err("it said too much to read".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let text = text.trim();
    if text.is_empty() {
        return Ok(Some(vec![]));
    }
    let v: Value =
        serde_json::from_str(text).map_err(|_| "Orbit couldn't read what it said".to_string())?;
    let known: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    Ok(Some(match tool {
        RiskKind::Actionlint => actionlint(&v, &known),
        _ => zizmor(&v, &known),
    }))
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
/// A folder of its own for one run, removed when it ends.
struct Folder(PathBuf);
impl Folder {
    fn new(dir: &Path, tool: RiskKind) -> Result<Self, String> {
        let path = dir.join(format!("{}-{}", tool.key(), uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path).map_err(|e| format!("Orbit couldn't make a folder: {e}"))?;
        Ok(Self(path))
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
/// which tools ran. What a tool found about text Orbit already found put
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
