//! Security alerts: each connected repository's open Dependabot, code
//! scanning and secret scanning alerts, read every `EVERY`, a few
//! repositories a pass, while Watch security alerts is on. Orbit reads the
//! newest `PAGE` open alerts of each kind, and says what it can't see: a
//! kind that's off for a repository, or one GitHub shows only to admins and
//! security managers. An alert that left its list is looked up on its own
//! (`look_up`), so Orbit lets it go only once GitHub says it's fixed or
//! dismissed, and says which (see `crate::problems::see_security`).
//!
//! Secrets stay secret. Orbit asks GitHub to hide each secret's value and
//! never reads the field that holds it, so no secret is kept, shown or sent
//! anywhere. Nothing here reaches GitHub but reads.
use crate::{
    logs,
    problems::{stamp, time},
    text::shorten,
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Repositories Orbit reads in one pass, each a read of every kind.
pub const PER_PASS: usize = 2;
/// How often Orbit reads each repository again.
pub const EVERY: Duration = Duration::minutes(30);
/// The most open alerts of each kind Orbit reads, the newest first.
pub const PAGE: usize = 100;
/// The most alerts that left a list Orbit looks up for a repository in one
/// pass; the rest wait for the next.
pub const LOOK_UPS: usize = 10;
/// The longest text Orbit keeps from an alert.
const MAX_TEXT: usize = 200;

/// Where GitHub found an alert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A dependency with a known weakness.
    #[default]
    Dependabot,
    /// Code a scanning tool such as CodeQL flagged.
    CodeScanning,
    /// A secret, such as a token, committed to the repository.
    SecretScanning,
}
impl Source {
    pub const ALL: [Source; 3] = [
        Source::Dependabot,
        Source::CodeScanning,
        Source::SecretScanning,
    ];
    /// Where GitHub lists them, under a repository.
    pub fn path(self) -> &'static str {
        match self {
            Source::Dependabot => "dependabot/alerts",
            Source::CodeScanning => "code-scanning/alerts",
            Source::SecretScanning => "secret-scanning/alerts",
        }
    }
    /// As in a problem's key.
    pub fn key(self) -> &'static str {
        match self {
            Source::Dependabot => "dependabot",
            Source::CodeScanning => "code_scanning",
            Source::SecretScanning => "secret_scanning",
        }
    }
    /// "Dependabot alert", in a sentence.
    pub fn one(self) -> &'static str {
        match self {
            Source::Dependabot => "Dependabot alert",
            Source::CodeScanning => "code scanning alert",
            Source::SecretScanning => "secret scanning alert",
        }
    }
    /// "Dependabot alerts", in a sentence.
    pub fn many(self) -> &'static str {
        match self {
            Source::Dependabot => "Dependabot alerts",
            Source::CodeScanning => "code scanning alerts",
            Source::SecretScanning => "secret scanning alerts",
        }
    }
    /// "Dependabot alerts", to start a sentence or a title.
    pub fn title(self) -> &'static str {
        match self {
            Source::Dependabot => "Dependabot alerts",
            Source::CodeScanning => "Code scanning alerts",
            Source::SecretScanning => "Secret scanning alerts",
        }
    }
}

/// How severe GitHub says an alert is.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}
impl Severity {
    pub fn word(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
    /// GitHub's word for it. GitHub's pages call medium "moderate".
    pub fn of(word: &str) -> Option<Severity> {
        match word.to_ascii_lowercase().as_str() {
            "low" => Some(Severity::Low),
            "medium" | "moderate" => Some(Severity::Medium),
            "high" => Some(Severity::High),
            "critical" => Some(Severity::Critical),
            _ => None,
        }
    }
}

/// One open alert, as Orbit read it. All its text is GitHub's or the
/// tool's words, shown as plain text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Alert {
    pub source: Source,
    pub number: u64,
    pub severity: Severity,
    /// What it is: the advisory's summary, the rule's description, or the
    /// kind of secret.
    pub title: String,
    /// What it's about: the package, the rule's ID, or the secret's type.
    pub subject: String,
    /// The file: the manifest that names the package, the file the code is
    /// in, or where the secret was first found. Empty when GitHub didn't
    /// say.
    pub path: String,
    /// The line in that file, or 0.
    pub line: u64,
    /// The version that fixes it, for a dependency, when there is one.
    pub fixed_in: String,
    pub url: String,
    pub created_at: String,
    /// For a secret: whether GitHub says it still works (`active`),
    /// doesn't (`inactive`), or can't tell.
    pub validity: String,
    /// For a secret: someone pushed it past push protection.
    pub bypassed: bool,
    /// When Orbit last asked GitHub about it on its own, after it left its
    /// list, whatever the answer, so each missing alert gets its turn
    /// (`to_look_up`). Empty until then.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub looked_at: String,
}
impl Alert {
    /// Whether only you should see to it now: every secret, as only a person
    /// can revoke one, and every high or critical alert.
    pub fn needs_you(&self) -> bool {
        self.source == Source::SecretScanning || self.severity >= Severity::High
    }
    /// Where it is: "package.json", "src/db.ts line 12", or nothing.
    pub fn place(&self) -> String {
        match (self.path.is_empty(), self.line) {
            (true, _) => String::new(),
            (false, 0) => self.path.clone(),
            (false, line) => format!("{} line {line}", self.path),
        }
    }
    /// What Orbit calls it in a sentence: "lodash (critical)", "Database
    /// query built from user-controlled sources in src/db.ts line 12
    /// (high)", or "a GitHub Personal Access Token in src/config.ts line 4".
    pub fn name(&self) -> String {
        let place = self.place();
        let at = if place.is_empty() {
            String::new()
        } else {
            format!(" in {place}")
        };
        match self.source {
            Source::Dependabot => format!("{} ({})", self.subject, self.severity.word()),
            Source::CodeScanning => {
                let what = if self.title.is_empty() {
                    &self.subject
                } else {
                    &self.title
                };
                format!("{what}{at} ({})", self.severity.word())
            }
            Source::SecretScanning => {
                let what = if self.title.is_empty() {
                    "secret"
                } else {
                    &self.title
                };
                format!("{} {what}{at}", article(what))
            }
        }
    }
    /// Its number and what it's about, for a timeline: "Dependabot alert
    /// 12, lodash".
    pub fn label(&self) -> String {
        let about = match self.source {
            Source::SecretScanning if !self.title.is_empty() => &self.title,
            _ => &self.subject,
        };
        let one = self.source.one();
        let mut label = format!("{}{} {}", one[..1].to_uppercase(), &one[1..], self.number);
        if !about.is_empty() {
            label.push_str(&format!(", {about}"));
        }
        label
    }
}
fn article(word: &str) -> &'static str {
    match word.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// What Orbit read of one kind of alert in a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Kind {
    pub source: Source,
    /// The open ones, the most severe first.
    pub alerts: Vec<Alert>,
    /// GitHub has more open ones than Orbit read, as it reads the newest
    /// `PAGE`.
    pub more: bool,
    /// Why Orbit can't see them, when it can't.
    pub unseen: Option<String>,
    /// GitHub says they're off for the repository, rather than hidden from
    /// Orbit's sign-in.
    pub off: bool,
}
/// What Orbit read of a repository's security alerts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoSecurity {
    pub repo: String,
    /// When Orbit last read it, by Orbit's clock.
    pub read_at: String,
    /// Each kind, in `Source::ALL`'s order.
    pub kinds: Vec<Kind>,
    /// Why the last read failed, how many in a row did, and when Orbit
    /// tries again. What was read before is kept meanwhile.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    /// GitHub says only those it lets in can see the repository. Anything
    /// else, unknown included, counts as public, so its alerts stay with
    /// you rather than going in an issue everyone can read.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
}
impl RepoSecurity {
    pub fn kind(&self, source: Source) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.source == source)
    }
}

/// What became of an alert that left its list, as GitHub says when Orbit
/// looks it up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Still open: it's only past the newest `PAGE` Orbit reads.
    Open,
    /// Closed: fixed (for a secret, revoked) or not, by whom when GitHub
    /// says, and GitHub's reason in plain words, such as "tolerable risk".
    Closed {
        fixed: bool,
        by: String,
        why: String,
    },
    /// GitHub no longer has it.
    Gone,
}
/// An alert that left its list, and what became of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Left {
    pub source: Source,
    pub number: u64,
    pub outcome: Outcome,
}

/// The repositories Orbit should read now, at most `PER_PASS`: those never
/// read first, then those read longest ago. One is read again `EVERY`
/// after it was, or after its wait once a read failed.
pub fn wanted(repos: &[String], read: &[RepoSecurity], now: DateTime<Utc>) -> Vec<String> {
    let mut due: Vec<(String, String)> = repos
        .iter()
        .filter(|r| validate_repo(r).is_ok())
        .filter_map(|repo| {
            let Some(s) = read.iter().find(|s| same_repo(&s.repo, repo)) else {
                return Some((String::new(), repo.clone()));
            };
            let ready = match &s.error {
                Some(_) => s
                    .retry_at
                    .as_deref()
                    .and_then(time)
                    .is_none_or(|retry| now >= retry),
                None => time(&s.read_at).is_none_or(|at| now - at >= EVERY),
            };
            ready.then(|| (s.read_at.clone(), repo.clone()))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter()
        .take(PER_PASS)
        .map(|(_, repo)| repo)
        .collect()
}

/// The fields Orbit asks GitHub for, for a list or one alert: a secret's
/// value always hidden.
fn fields(source: Source, list: bool) -> Vec<(&'static str, String)> {
    let mut fields = vec![];
    if list {
        fields.extend([
            ("state", "open".to_string()),
            ("sort", "created".to_string()),
            ("direction", "desc".to_string()),
            ("per_page", PAGE.to_string()),
        ]);
    }
    if source == Source::SecretScanning {
        fields.push(("hide_secret", "true".to_string()));
    }
    fields
}

/// Reads whether a repository is private, and its open alerts of every
/// kind.
pub async fn read(repo: &str, private: bool) -> Result<RepoSecurity, String> {
    validate_repo(repo)?;
    let mut read = RepoSecurity {
        repo: repo.to_string(),
        read_at: stamp(Utc::now()),
        private,
        ..Default::default()
    };
    for source in Source::ALL {
        let answer = crate::github::api(
            &format!("repos/{repo}/{}", source.path()),
            "GET",
            &fields(source, true),
            None,
        )
        .await;
        read.kinds.push(kind(repo, source, answer)?);
    }
    Ok(read)
}
/// Whether GitHub says now that a repository is private. Asked before
/// each read of its alerts, and again just before each write about them,
/// as only a private repository's agent hears of them.
pub async fn private_now(repo: &str) -> Result<bool, String> {
    validate_repo(repo)?;
    private(crate::github::api(&format!("repos/{repo}"), "GET", &[], None).await)
}
/// Whether GitHub's answer about a repository says it is private: an
/// internal one isn't. One it didn't give is an error, never taken as
/// either: it fails the read, and holds back any write about alerts.
pub fn private(answer: Result<Value, String>) -> Result<bool, String> {
    let v = answer?;
    Ok(v["private"] == true && v["visibility"].as_str().is_none_or(|w| w == "private"))
}
/// What GitHub's answer for one kind says, or the error to try again
/// after. A refusal is kept as why Orbit can't see them; a rate limit is
/// tried again.
pub fn kind(repo: &str, source: Source, answer: Result<Value, String>) -> Result<Kind, String> {
    let v = match answer {
        Ok(v) => v,
        Err(e) if logs::rate_limited(&e) => return Err(e),
        Err(e) if off(source, &e) => {
            return Ok(Kind {
                source,
                unseen: Some(off_text(repo, source)),
                off: true,
                ..Default::default()
            })
        }
        Err(e) if logs::refused(&e) => {
            return Ok(Kind {
                source,
                unseen: Some(hidden_text(repo, source)),
                ..Default::default()
            })
        }
        Err(e) => return Err(e),
    };
    let list = v
        .as_array()
        .ok_or_else(|| format!("GitHub didn't return the {}.", source.many()))?;
    let mut alerts: Vec<Alert> = list
        .iter()
        .filter(|a| a["state"].as_str().is_none_or(|s| s == "open"))
        .filter_map(|a| alert(source, a))
        .collect();
    sort(&mut alerts);
    Ok(Kind {
        source,
        alerts,
        more: list.len() >= PAGE,
        ..Default::default()
    })
}
/// The most severe first, then the newest.
pub fn sort(alerts: &mut [Alert]) {
    alerts.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.number.cmp(&a.number)));
}
/// Whether GitHub's refusal says the kind is off for the repository, as
/// opposed to hidden from Orbit's sign-in.
fn off(source: Source, error: &str) -> bool {
    let e = error.to_lowercase();
    let said = |words: &[&str]| words.iter().any(|w| e.contains(w));
    match source {
        Source::Dependabot => said(&[
            "dependabot alerts are disabled",
            "dependabot alerts are not enabled",
        ]),
        Source::CodeScanning => said(&[
            "no analysis found",
            "code scanning is not enabled",
            "advanced security must be enabled",
            "code security must be enabled",
        ]),
        Source::SecretScanning => said(&[
            "secret scanning is disabled",
            "secret scanning is not enabled",
        ]),
    }
}
/// Why Orbit sees no alerts of a kind that's off, in Orbit's words.
pub fn off_text(repo: &str, source: Source) -> String {
    match source {
        Source::Dependabot => format!("Dependabot alerts are off for {repo}."),
        Source::CodeScanning => format!("No code scanning runs on {repo}."),
        Source::SecretScanning => format!("Secret scanning is off for {repo}."),
    }
}
/// Why Orbit can't see alerts GitHub hides from its sign-in.
pub fn hidden_text(repo: &str, source: Source) -> String {
    format!(
        "GitHub doesn't show {repo}'s {} to Orbit's sign-in. It shows them to admins and security managers.",
        source.many()
    )
}

/// GitHub's or a tool's words, on one line and not too long.
fn plain(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    shorten(&text, MAX_TEXT)
}
fn str_of(v: &Value) -> String {
    v.as_str().map(plain).unwrap_or_default()
}
/// Only a link to GitHub is kept.
fn link(v: &Value) -> String {
    v.as_str()
        .filter(|u| u.starts_with("https://github.com/"))
        .map(str::to_string)
        .unwrap_or_default()
}
/// One alert from GitHub's list, or None when it has no number. A secret's
/// value is never read, even if GitHub sends it.
pub fn alert(source: Source, v: &Value) -> Option<Alert> {
    let number = v["number"].as_u64()?;
    let mut a = Alert {
        source,
        number,
        url: link(&v["html_url"]),
        created_at: str_of(&v["created_at"]),
        ..Default::default()
    };
    match source {
        Source::Dependabot => {
            let vulnerability = &v["security_vulnerability"];
            a.subject = str_of(&v["dependency"]["package"]["name"]);
            if a.subject.is_empty() {
                a.subject = str_of(&vulnerability["package"]["name"]);
            }
            a.title = str_of(&v["security_advisory"]["summary"]);
            a.path = str_of(&v["dependency"]["manifest_path"]);
            a.fixed_in = str_of(&vulnerability["first_patched_version"]["identifier"]);
            a.severity = [
                &vulnerability["severity"],
                &v["security_advisory"]["severity"],
            ]
            .iter()
            .find_map(|s| s.as_str().and_then(Severity::of))
            .unwrap_or_default();
        }
        Source::CodeScanning => {
            let rule = &v["rule"];
            let instance = &v["most_recent_instance"];
            a.subject = str_of(&rule["id"]);
            a.title = str_of(&rule["description"]);
            a.path = str_of(&instance["location"]["path"]);
            a.line = instance["location"]["start_line"].as_u64().unwrap_or(0);
            // A rule that isn't about security has no security severity:
            // an error counts as medium, anything else as low.
            a.severity = rule["security_severity_level"]
                .as_str()
                .and_then(Severity::of)
                .unwrap_or(match rule["severity"].as_str() {
                    Some("error") => Severity::Medium,
                    _ => Severity::Low,
                });
        }
        Source::SecretScanning => {
            a.subject = str_of(&v["secret_type"]);
            a.title = str_of(&v["secret_type_display_name"]);
            let found = &v["first_location_detected"];
            a.path = str_of(&found["path"]);
            a.line = found["start_line"].as_u64().unwrap_or(0);
            a.validity = match v["validity"].as_str() {
                Some(w @ ("active" | "inactive")) => w.to_string(),
                _ => String::new(),
            };
            a.bypassed = v["push_protection_bypassed"] == true;
            // Every secret needs you; it sorts with the most severe.
            a.severity = Severity::Critical;
        }
    }
    Some(a)
}

/// What became of an alert, from GitHub's answer for it alone.
pub fn outcome(source: Source, v: &Value) -> Outcome {
    let state = v["state"].as_str().unwrap_or("");
    let login = |who: &Value| {
        who["login"]
            .as_str()
            .filter(|l| {
                !l.is_empty()
                    && l.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-[]_".contains(c))
            })
            .map(str::to_string)
            .unwrap_or_default()
    };
    let words = |v: &Value| {
        let w = str_of(v).replace('_', " ");
        if w == "wont fix" {
            "won't fix".to_string()
        } else {
            w
        }
    };
    match (source, state) {
        (_, "open") => Outcome::Open,
        (Source::Dependabot | Source::CodeScanning, "fixed") => Outcome::Closed {
            fixed: true,
            by: String::new(),
            why: String::new(),
        },
        (Source::Dependabot, "auto_dismissed") => Outcome::Closed {
            fixed: false,
            by: String::new(),
            why: "dismissed by a Dependabot rule".into(),
        },
        (Source::Dependabot | Source::CodeScanning, "dismissed") => Outcome::Closed {
            fixed: false,
            by: login(&v["dismissed_by"]),
            why: words(&v["dismissed_reason"]),
        },
        (Source::SecretScanning, "resolved") => {
            let why = words(&v["resolution"]);
            Outcome::Closed {
                fixed: why == "revoked",
                by: login(&v["resolved_by"]),
                why,
            }
        }
        (_, other) => Outcome::Closed {
            fixed: false,
            by: String::new(),
            why: plain(other).replace('_', " "),
        },
    }
}
/// What became of an alert, for its problem's timeline: "Dependabot alert
/// 12, lodash: fixed, GitHub says."
pub fn outcome_text(a: &Alert, outcome: &Outcome) -> String {
    let label = a.label();
    match outcome {
        Outcome::Open => format!("{label}: still open."),
        Outcome::Gone => format!("{label}: GitHub no longer has it."),
        Outcome::Closed { fixed, by, why } => {
            let by = if by.is_empty() {
                String::new()
            } else {
                format!(" by @{by}")
            };
            match (fixed, a.source) {
                (true, Source::SecretScanning) => format!("{label}: closed as revoked{by}."),
                (true, _) => format!("{label}: fixed, GitHub says."),
                (false, _) if why.is_empty() => format!("{label}: dismissed{by}."),
                (false, _) if why.starts_with("dismissed") => format!("{label}: {why}{by}."),
                (false, _) => format!("{label}: dismissed{by} as {why}."),
            }
        }
    }
}

/// The alerts of a repository's open problems that its latest read no
/// longer lists, to look up, at most `LOOK_UPS`: those never looked up
/// first, then those looked up longest ago, so one GitHub still has open,
/// or one it didn't answer for, never keeps the others from their turn.
/// Kinds Orbit can't see now are left out, as their problems close.
pub fn to_look_up(
    problems: &[crate::problems::Problem],
    read: &RepoSecurity,
) -> Vec<(Source, u64)> {
    let mut out: Vec<(&str, Source, u64)> = vec![];
    for kind in read.kinds.iter().filter(|k| k.unseen.is_none()) {
        let key = crate::problems::security_key(&read.repo, kind.source);
        let Some(p) = problems.iter().find(|p| p.key == key && p.is_open()) else {
            continue;
        };
        for a in &p.security_alerts {
            if !kind.alerts.iter().any(|b| b.number == a.number) {
                out.push((&a.looked_at, kind.source, a.number));
            }
        }
    }
    // Stable, so alerts looked up together keep their order.
    out.sort_by(|a, b| a.0.cmp(b.0));
    out.into_iter()
        .take(LOOK_UPS)
        .map(|(_, source, number)| (source, number))
        .collect()
}
/// Asks GitHub what became of one alert.
pub async fn look_up(repo: &str, source: Source, number: u64) -> Result<Outcome, String> {
    validate_repo(repo)?;
    let answer = crate::github::api(
        &format!("repos/{repo}/{}/{number}", source.path()),
        "GET",
        &fields(source, false),
        None,
    )
    .await;
    match answer {
        Ok(v) => Ok(outcome(source, &v)),
        // Its list was just read, so the kind is on and Orbit can see it.
        Err(e) if e.contains("HTTP 404") => Ok(Outcome::Gone),
        Err(e) => Err(e),
    }
}

/// A list of names in a sentence: "A", "A and B", "A, B and 2 more".
fn names(all: &[String]) -> String {
    match all {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}
/// A kind's open alerts in a sentence: "2 open Dependabot alerts: lodash
/// (critical) and minimist (low)."
pub fn open_text(source: Source, alerts: &[Alert]) -> String {
    let all: Vec<String> = alerts.iter().map(Alert::name).collect();
    match alerts.len() {
        1 => format!("1 open {}: {}.", source.one(), names(&all)),
        n => format!("{n} open {}: {}.", source.many(), names(&all)),
    }
}
/// The alerts of one kind in a repository that need you, in a sentence, or
/// None when none do.
pub fn needs_you_text(repo: &str, source: Source, alerts: &[Alert]) -> Option<String> {
    let urgent: Vec<String> = alerts
        .iter()
        .filter(|a| a.needs_you())
        .map(Alert::name)
        .collect();
    let all = names(&urgent);
    Some(match (source, urgent.len()) {
        (_, 0) => return None,
        (Source::SecretScanning, 1) => format!(
            "GitHub found a secret in {repo}: {all}. Revoke it, then close its alert on GitHub."
        ),
        (Source::SecretScanning, n) => format!(
            "GitHub found {n} secrets in {repo}: {all}. Revoke each one, then close its alert on GitHub."
        ),
        (_, 1) => format!("{repo} has a high or critical {}: {all}.", source.one()),
        (_, n) => format!("{repo} has {n} high or critical {}: {all}.", source.many()),
    })
}

/// Records what Orbit read of a repository's alerts. After an error it
/// keeps what it read before and tries again after a wait, a minute
/// doubling up to `logs::MAX_RETRY`.
pub fn record(
    read: &mut Vec<RepoSecurity>,
    repo: &str,
    result: Result<RepoSecurity, String>,
    now: DateTime<Utc>,
) {
    let known = read.iter().position(|s| same_repo(&s.repo, repo));
    let security = match result {
        Ok(security) => security,
        Err(error) => {
            let mut before = known
                .map(|i| read[i].clone())
                .unwrap_or_else(|| RepoSecurity {
                    repo: repo.to_string(),
                    ..Default::default()
                });
            before.tries += 1;
            let wait = Duration::minutes(1i64 << (before.tries - 1).min(6)).min(logs::MAX_RETRY);
            before.error = Some(format!(
                "Orbit couldn't read the security alerts yet: {}. It tries again later.",
                logs::short(&error)
            ));
            before.retry_at = Some(stamp(now + wait));
            before
        }
    };
    match known {
        Some(i) => read[i] = security,
        None => read.push(security),
    }
}

/// Forgets repositories that are no longer connected.
pub fn prune(read: &mut Vec<RepoSecurity>, connected: &[String]) {
    read.retain(|s| connected.iter().any(|r| same_repo(r, &s.repo)));
}

#[cfg(test)]
#[path = "security_tests.rs"]
pub(crate) mod tests;
