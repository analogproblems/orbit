//! The security settings baseline: the settings each organization's
//! repositories should have, and where each connected repository drifts
//! from it. Each owner (an organization, or a person's account) has a
//! baseline in Settings, starting from Orbit's (`Setting::ORBIT`), and
//! Orbit reads each repository's settings every `EVERY`, a few
//! repositories a pass, while Watch security alerts is on.
//!
//! GitHub shows these settings only to a repository's admins, so for
//! another repository Orbit says it can't see them. Push protection for
//! secrets blocks pushes, so it's in a baseline only if you add it, and
//! branch protection, rulesets and required checks are never part of one.
//!
//! Orbit changes a setting only when you tap Fix on it (`ask`), as
//! changing settings always asks first. Each tap tries once: Orbit reads
//! the repository's settings again, and only if the setting is still off
//! does it record the change in the journal, save it, and send it
//! (`record_change`, `turn_on`). A change cut off by Orbit stopping is
//! never sent again on its own (`interrupted`): Orbit reads the setting
//! again instead, and you tap Fix again if it's still off.
use crate::{
    logs,
    ops::{Action, Write, WriteState},
    problems::{stamp, time},
    types::{same_repo, validate_repo},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Repositories Orbit reads in one pass.
pub const PER_PASS: usize = 2;
/// How often Orbit reads each repository's settings again.
pub const EVERY: Duration = Duration::hours(1);

/// A security setting a baseline can ask for.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    /// GitHub looks for secrets pushed to the repository.
    #[default]
    SecretScanning,
    /// GitHub blocks a push that holds a secret it knows.
    PushProtection,
    /// GitHub says when a dependency has a known weakness.
    DependabotAlerts,
    /// Dependabot opens pull requests that fix those.
    DependabotUpdates,
    /// Anyone can report a weakness privately, rather than in an issue.
    PrivateReporting,
    /// The `GITHUB_TOKEN` can only read, unless a workflow asks for more.
    ReadOnlyToken,
    /// GitHub Actions can't approve pull requests.
    NoActionsApproval,
    /// Workflows from a first-time contributor's fork wait for a
    /// maintainer's approval, not only those of someone new to GitHub.
    ForkApproval,
}
impl Setting {
    pub const ALL: [Setting; 8] = [
        Setting::SecretScanning,
        Setting::PushProtection,
        Setting::DependabotAlerts,
        Setting::DependabotUpdates,
        Setting::PrivateReporting,
        Setting::ReadOnlyToken,
        Setting::NoActionsApproval,
        Setting::ForkApproval,
    ];
    /// Orbit's baseline: all but push protection, which blocks pushes.
    pub const ORBIT: [Setting; 7] = [
        Setting::SecretScanning,
        Setting::DependabotAlerts,
        Setting::DependabotUpdates,
        Setting::PrivateReporting,
        Setting::ReadOnlyToken,
        Setting::NoActionsApproval,
        Setting::ForkApproval,
    ];
    /// As Orbit's state file and the frontend name it.
    pub fn key(self) -> &'static str {
        match self {
            Setting::SecretScanning => "secret_scanning",
            Setting::PushProtection => "push_protection",
            Setting::DependabotAlerts => "dependabot_alerts",
            Setting::DependabotUpdates => "dependabot_updates",
            Setting::PrivateReporting => "private_reporting",
            Setting::ReadOnlyToken => "read_only_token",
            Setting::NoActionsApproval => "no_actions_approval",
            Setting::ForkApproval => "fork_approval",
        }
    }
    /// Its name, as Settings shows it.
    pub fn name(self) -> &'static str {
        match self {
            Setting::SecretScanning => "Secret scanning",
            Setting::PushProtection => "Push protection for secrets",
            Setting::DependabotAlerts => "Dependabot alerts",
            Setting::DependabotUpdates => "Dependabot security updates",
            Setting::PrivateReporting => "Private vulnerability reporting",
            Setting::ReadOnlyToken => "Read-only Actions token",
            Setting::NoActionsApproval => "Actions can't approve pull requests",
            Setting::ForkApproval => "Approval for first-time contributors",
        }
    }
    /// Whether only a public repository has it: GitHub offers private
    /// reporting only there, and only there can anyone's fork run a
    /// workflow.
    pub fn public_only(self) -> bool {
        matches!(self, Setting::PrivateReporting | Setting::ForkApproval)
    }
    /// What a repository that drifts from it has, in a sentence.
    pub fn off_text(self) -> &'static str {
        match self {
            Setting::SecretScanning => "Secret scanning is off.",
            Setting::PushProtection => "Push protection for secrets is off.",
            Setting::DependabotAlerts => "Dependabot alerts are off.",
            Setting::DependabotUpdates => "Dependabot security updates are off.",
            Setting::PrivateReporting => "Private vulnerability reporting is off.",
            Setting::ReadOnlyToken => "The Actions token can write by default.",
            Setting::NoActionsApproval => "GitHub Actions can approve pull requests.",
            Setting::ForkApproval => {
                "Workflows from first-time contributors run without approval, unless they're new to GitHub."
            }
        }
    }
}

/// The settings one owner's repositories should have. An owner with none
/// in Settings has Orbit's (`Setting::ORBIT`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Baseline {
    /// The organization, or the person's account.
    pub owner: String,
    pub settings: Vec<Setting>,
}
/// The settings `owner`'s repositories should have, in `Setting::ALL`'s
/// order.
pub fn wanted_for(baselines: &[Baseline], owner: &str) -> Vec<Setting> {
    let chosen = baselines
        .iter()
        .find(|b| b.owner.eq_ignore_ascii_case(owner))
        .map_or(&Setting::ORBIT[..], |b| &b.settings[..]);
    Setting::ALL
        .into_iter()
        .filter(|s| chosen.contains(s))
        .collect()
}
/// The owner of `repo`, such as `acme` for `acme/api`.
pub fn owner(repo: &str) -> &str {
    repo.split('/').next().unwrap_or_default()
}
/// Checks the baselines in Settings: each for a real owner name, once.
pub fn validate(baselines: &[Baseline]) -> Result<(), String> {
    for (i, b) in baselines.iter().enumerate() {
        if !crate::types::valid_login(&b.owner) || b.owner.ends_with("[bot]") {
            return Err(format!("{:?} isn't a GitHub owner's name.", b.owner));
        }
        if baselines[..i]
            .iter()
            .any(|o| o.owner.eq_ignore_ascii_case(&b.owner))
        {
            return Err(format!("{} has more than one baseline.", b.owner));
        }
    }
    Ok(())
}

/// What Orbit read of one setting: on, off, or unknown (`None`), when
/// GitHub doesn't offer it here or wouldn't say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Found {
    pub setting: Setting,
    pub on: Option<bool>,
}
/// What Orbit read of a repository's security settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoSettings {
    pub repo: String,
    /// When Orbit last read them, by Orbit's clock.
    pub read_at: String,
    /// GitHub says only those it lets in can see the repository, as it's
    /// private or internal (`public`), so the settings only a public
    /// repository has aren't read.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    /// Each setting Orbit read, in `Setting::ALL`'s order.
    pub found: Vec<Found>,
    /// Why Orbit can't see them, when it can't.
    pub unseen: Option<String>,
    /// Why the last read failed, how many in a row did, and when Orbit
    /// tries again. What was read before is kept meanwhile.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
    /// The settings you tapped Fix on that Orbit hasn't tried to change
    /// yet. A read keeps them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<Setting>,
}
impl RepoSettings {
    /// Whether a setting is on, as Orbit last read it.
    pub fn on(&self, setting: Setting) -> Option<bool> {
        self.found
            .iter()
            .find(|f| f.setting == setting)
            .and_then(|f| f.on)
    }
    /// Whether this repository has the setting: those only public
    /// repositories have aren't asked of a private one.
    pub fn has(&self, setting: Setting) -> bool {
        !(self.private && setting.public_only())
    }
}
/// The settings in `wanted` that a repository has off: where it drifts
/// from its baseline. Nothing, while Orbit can't see them.
pub fn drift(read: &RepoSettings, wanted: &[Setting]) -> Vec<Setting> {
    if read.unseen.is_some() || read.read_at.is_empty() {
        return vec![];
    }
    wanted
        .iter()
        .copied()
        .filter(|&s| read.has(s) && read.on(s) == Some(false))
        .collect()
}
/// The settings in `wanted` Orbit couldn't tell are on or off, such as
/// secret scanning where GitHub doesn't offer it.
pub fn unknown(read: &RepoSettings, wanted: &[Setting]) -> Vec<Setting> {
    if read.unseen.is_some() || read.read_at.is_empty() {
        return vec![];
    }
    wanted
        .iter()
        .copied()
        .filter(|&s| read.has(s) && read.on(s).is_none())
        .collect()
}
/// A repository that drifts, in a sentence: "acme/api drifts from your
/// security settings: Dependabot alerts are off. The Actions token can
/// write by default."
pub fn drift_text(repo: &str, drift: &[Setting]) -> String {
    let what: Vec<&str> = drift.iter().map(|s| s.off_text()).collect();
    format!(
        "{repo} drifts from your security settings: {}",
        what.join(" ")
    )
}
/// Each watched repository that drifts from its owner's baseline, with
/// how: for Bit's morning brief.
pub fn drifting(read: &[RepoSettings], baselines: &[Baseline]) -> Vec<(String, Vec<Setting>)> {
    read.iter()
        .map(|r| {
            (
                r.repo.clone(),
                drift(r, &wanted_for(baselines, owner(&r.repo))),
            )
        })
        .filter(|(_, drift)| !drift.is_empty())
        .collect()
}
/// Why Orbit can't see a repository's settings.
pub fn unseen_text(repo: &str) -> String {
    format!(
        "GitHub shows {repo}'s security settings only to its admins, and Orbit's sign-in isn't one."
    )
}

/// The repositories Orbit should read now, at most `PER_PASS`: those never
/// read first, then those read longest ago. One is read again `EVERY`
/// after it was, or after its wait once a read failed.
pub fn wanted(repos: &[String], read: &[RepoSettings], now: DateTime<Utc>) -> Vec<String> {
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

/// Reads a repository's security settings: first whether Orbit's sign-in
/// is an admin there, as only an admin may see them, then each setting.
pub async fn read(repo: &str) -> Result<RepoSettings, String> {
    validate_repo(repo)?;
    let api = |path: &str| {
        let endpoint = format!("repos/{repo}{path}");
        async move { crate::github::api(&endpoint, "GET", &[], None).await }
    };
    let about = api("").await?;
    let mut read = RepoSettings {
        repo: repo.to_string(),
        read_at: stamp(Utc::now()),
        private: !public(&about),
        ..Default::default()
    };
    if about["permissions"]["admin"] != true {
        read.unseen = Some(unseen_text(repo));
        return Ok(read);
    }
    let analysis = |key: &str| match about["security_and_analysis"][key]["status"].as_str() {
        Some("enabled") => Some(true),
        Some("disabled") => Some(false),
        _ => None,
    };
    let mut found = vec![
        (Setting::SecretScanning, analysis("secret_scanning")),
        (
            Setting::PushProtection,
            analysis("secret_scanning_push_protection"),
        ),
    ];
    // GitHub answers with no body when they're on, and 404 when they're off.
    let alerts = crate::github::api_text(&format!("repos/{repo}/vulnerability-alerts"))
        .await
        .map(|_| Value::Null);
    found.push((
        Setting::DependabotAlerts,
        setting(alerts, Some(false), |_| Some(true))?,
    ));
    found.push((
        Setting::DependabotUpdates,
        setting(api("/automated-security-fixes").await, Some(false), |v| {
            v["enabled"].as_bool()
        })?,
    ));
    if !read.private {
        found.push((
            Setting::PrivateReporting,
            setting(api("/private-vulnerability-reporting").await, None, |v| {
                v["enabled"].as_bool()
            })?,
        ));
    }
    let workflow = api("/actions/permissions/workflow").await;
    found.push((
        Setting::ReadOnlyToken,
        setting(workflow.clone(), None, |v| {
            match v["default_workflow_permissions"].as_str() {
                Some("read") => Some(true),
                Some("write") => Some(false),
                _ => None,
            }
        })?,
    ));
    found.push((
        Setting::NoActionsApproval,
        setting(workflow, None, |v| {
            v["can_approve_pull_request_reviews"].as_bool().map(|b| !b)
        })?,
    ));
    if !read.private {
        found.push((
            Setting::ForkApproval,
            setting(
                api("/actions/permissions/fork-pr-contributor-approval").await,
                None,
                |v| match v["approval_policy"].as_str() {
                    Some("first_time_contributors" | "all_external_contributors") => Some(true),
                    Some("first_time_contributors_new_to_github") => Some(false),
                    _ => None,
                },
            )?,
        ));
    }
    read.found = found
        .into_iter()
        .map(|(setting, on)| Found { setting, on })
        .collect();
    read.found.sort_by_key(|f| f.setting);
    Ok(read)
}
/// Whether GitHub shows the repository to everyone. An internal one, which
/// only its enterprise sees, isn't public, so it has no settings only
/// public repositories have.
pub fn public(about: &Value) -> bool {
    match about["visibility"].as_str() {
        Some(visibility) => visibility == "public",
        None => about["private"] == false,
    }
}
/// What GitHub's answer about one setting says: what `seen` reads in it,
/// `missing` for a 404 when that says it's off, and unknown when GitHub
/// refuses or doesn't offer it here. A rate limit or any other error fails
/// the read, to try again later.
pub fn setting(
    answer: Result<Value, String>,
    missing: Option<bool>,
    seen: impl Fn(&Value) -> Option<bool>,
) -> Result<Option<bool>, String> {
    match answer {
        Ok(v) => Ok(seen(&v)),
        Err(e) if logs::rate_limited(&e) => Err(e),
        Err(e) if missing.is_some() && e.contains("HTTP 404") => Ok(missing),
        Err(e) if logs::refused(&e) || e.contains("HTTP 422") => Ok(None),
        Err(e) => Err(e),
    }
}

/// Records what Orbit read of a repository's settings. After an error it
/// keeps what it read before and tries again after a wait, a minute
/// doubling up to `logs::MAX_RETRY`.
pub fn record(
    read: &mut Vec<RepoSettings>,
    repo: &str,
    result: Result<RepoSettings, String>,
    now: DateTime<Utc>,
) {
    let known = read.iter().position(|s| same_repo(&s.repo, repo));
    let settings = match result {
        Ok(mut settings) => {
            settings.asked = known.map(|i| read[i].asked.clone()).unwrap_or_default();
            settings
        }
        Err(error) => {
            let mut before = known
                .map(|i| read[i].clone())
                .unwrap_or_else(|| RepoSettings {
                    repo: repo.to_string(),
                    ..Default::default()
                });
            before.tries += 1;
            let wait = Duration::minutes(1i64 << (before.tries - 1).min(6)).min(logs::MAX_RETRY);
            before.error = Some(format!(
                "Orbit couldn't read the security settings yet: {}. It tries again later.",
                logs::short(&error)
            ));
            before.retry_at = Some(stamp(now + wait));
            before
        }
    };
    match known {
        Some(i) => read[i] = settings,
        None => read.push(settings),
    }
}

/// Forgets repositories that are no longer connected.
pub fn prune(read: &mut Vec<RepoSettings>, connected: &[String]) {
    read.retain(|s| connected.iter().any(|r| same_repo(r, &s.repo)));
}

/// Why a change cut off by Orbit stopping isn't sent again.
pub const INTERRUPTED: &str = "Orbit stopped before GitHub answered. It read the setting again rather than send the change twice.";

/// Notes your tap on Fix for a setting `repo` drifts on, for Orbit to
/// change. Refused unless Orbit last read it off where its baseline wants
/// it on, and while a change to it waits or is on its way.
pub fn ask(
    read: &mut [RepoSettings],
    journal: &[Write],
    baselines: &[Baseline],
    repo: &str,
    setting: Setting,
) -> Result<(), String> {
    let r = read
        .iter_mut()
        .find(|r| same_repo(&r.repo, repo) && !r.read_at.is_empty())
        .ok_or_else(|| format!("Orbit hasn't read {repo}'s security settings yet."))?;
    if let Some(unseen) = &r.unseen {
        return Err(unseen.clone());
    }
    if r.asked.contains(&setting) || sending(journal, &r.repo, setting) {
        return Err(format!(
            "Orbit is already changing {} in {}.",
            setting.name(),
            r.repo
        ));
    }
    let (name, repo) = (setting.name(), &r.repo);
    if !wanted_for(baselines, owner(repo)).contains(&setting) || !r.has(setting) {
        return Err(format!("{repo}'s baseline doesn't ask for {name}."));
    }
    match r.on(setting) {
        Some(false) => {}
        Some(true) => return Err(format!("{name} is already on in {repo}.")),
        None => {
            return Err(format!(
                "Orbit can't tell whether {name} is on in {repo}, so it changes nothing."
            ))
        }
    }
    r.asked.push(setting);
    Ok(())
}
/// Whether a change to `setting` in `repo` is on its way to GitHub.
pub fn sending(journal: &[Write], repo: &str, setting: Setting) -> bool {
    journal.iter().any(|w| {
        w.action == Action::ChangeSetting
            && w.state == WriteState::Sending
            && w.setting == Some(setting)
            && same_repo(&w.repo, repo)
    })
}
/// The next change you asked for, in a repository in `watched`. Asks in
/// any other repository are dropped, as Orbit no longer watches it.
pub fn next_ask(read: &mut [RepoSettings], watched: &[String]) -> Option<(String, Setting)> {
    for r in read.iter_mut() {
        if !watched.iter().any(|w| same_repo(w, &r.repo)) {
            r.asked.clear();
        }
    }
    read.iter()
        .find_map(|r| Some((r.repo.clone(), *r.asked.first()?)))
}
/// Takes your ask for a change once Orbit has read the repository again
/// for it, and says whether the setting still needs it: still off, as
/// GitHub showed just now (`fresh`), where its baseline wants it on.
pub fn take(
    read: &mut [RepoSettings],
    baselines: &[Baseline],
    repo: &str,
    setting: Setting,
    fresh: bool,
) -> bool {
    let Some(r) = read.iter_mut().find(|r| same_repo(&r.repo, repo)) else {
        return false;
    };
    r.asked.retain(|&s| s != setting);
    fresh && drift(r, &wanted_for(baselines, owner(&r.repo))).contains(&setting)
}
/// Whether any change you asked for waits.
pub fn any_asked(read: &[RepoSettings]) -> bool {
    read.iter().any(|r| !r.asked.is_empty())
}

/// Changes left on their way to GitHub when Orbit stopped: GitHub may or
/// may not have made them. None is sent again on its own; Orbit reads each
/// repository again instead. Run before a change is sent, while none is.
/// Returns their repositories.
pub fn interrupted(journal: &mut [Write], now: DateTime<Utc>) -> Vec<String> {
    let mut repos: Vec<String> = vec![];
    for w in journal
        .iter_mut()
        .filter(|w| w.action == Action::ChangeSetting && w.state == WriteState::Sending)
    {
        w.state = WriteState::Failed;
        w.done_at = Some(stamp(now));
        w.error = Some(INTERRUPTED.into());
        if !repos.iter().any(|r| same_repo(r, &w.repo)) {
            repos.push(w.repo.clone());
        }
    }
    repos
}
/// Records a change before it is sent.
pub fn record_change(
    journal: &mut Vec<Write>,
    repo: &str,
    setting: Setting,
    now: DateTime<Utc>,
) -> String {
    let id = crate::types::id();
    journal.push(Write {
        id: id.clone(),
        thread: format!("settings:{}", repo.to_lowercase()),
        action: Action::ChangeSetting,
        repo: repo.to_string(),
        number: None,
        state: WriteState::Sending,
        at: stamp(now),
        done_at: None,
        url: String::new(),
        error: None,
        setting: Some(setting),
    });
    id
}
/// Records what GitHub said to a change: done, or why not, in a sentence
/// (`change_error`).
pub fn settle(journal: &mut [Write], id: &str, result: Result<(), String>, now: DateTime<Utc>) {
    let Some(w) = journal.iter_mut().find(|w| w.id == id) else {
        return;
    };
    w.done_at = Some(stamp(now));
    match result {
        Ok(()) => {
            w.state = WriteState::Done;
            w.url = w.setting.map(|s| page(&w.repo, s)).unwrap_or_default();
        }
        Err(error) => {
            w.state = WriteState::Failed;
            w.error = Some(error);
        }
    }
}
/// Where GitHub shows a repository's setting.
pub fn page(repo: &str, setting: Setting) -> String {
    let page = match setting {
        Setting::ReadOnlyToken | Setting::NoActionsApproval | Setting::ForkApproval => "actions",
        _ => "security_analysis",
    };
    format!("https://github.com/{repo}/settings/{page}")
}
/// Why GitHub didn't make a change, in a sentence.
pub fn change_error(error: &str) -> String {
    if logs::rate_limited(error) {
        "GitHub's rate limit held this back. Tap Fix again later.".into()
    } else if error.contains("HTTP 409") {
        "The organization's own settings don't let a repository change this, so an owner of the organization changes it there.".into()
    } else if logs::refused(error) {
        "GitHub didn't let Orbit change this. Orbit's sign-in needs to be an admin of the repository.".into()
    } else {
        format!("GitHub didn't make the change: {}.", logs::short(error))
    }
}

/// Turns a setting on in a repository. Anything GitHub sets along with it
/// is sent as Orbit just read it (`read`), so it stays as it is.
pub async fn turn_on(repo: &str, setting: Setting, read: &RepoSettings) -> Result<(), String> {
    validate_repo(repo)?;
    let (path, method, body) = change(setting, read);
    crate::github::api_write(&format!("repos/{repo}{path}"), method, body).await
}
/// The call that turns a setting on: the path under the repository, the
/// method, and what's sent.
pub fn change(
    setting: Setting,
    read: &RepoSettings,
) -> (&'static str, &'static str, Option<Value>) {
    let analysis = |key: &str| {
        let mut status = Map::new();
        status.insert(key.into(), json!({ "status": "enabled" }));
        Some(json!({ "security_and_analysis": status }))
    };
    match setting {
        Setting::SecretScanning => ("", "PATCH", analysis("secret_scanning")),
        Setting::PushProtection => ("", "PATCH", analysis("secret_scanning_push_protection")),
        Setting::DependabotAlerts => ("/vulnerability-alerts", "PUT", None),
        Setting::DependabotUpdates => ("/automated-security-fixes", "PUT", None),
        Setting::PrivateReporting => ("/private-vulnerability-reporting", "PUT", None),
        Setting::ReadOnlyToken | Setting::NoActionsApproval => {
            // GitHub sets both together; one Orbit doesn't know is left out.
            let on = |s: Setting| if s == setting { Some(true) } else { read.on(s) };
            let mut body = Map::new();
            if let Some(read_only) = on(Setting::ReadOnlyToken) {
                let token = if read_only { "read" } else { "write" };
                body.insert("default_workflow_permissions".into(), json!(token));
            }
            if let Some(no_approval) = on(Setting::NoActionsApproval) {
                body.insert(
                    "can_approve_pull_request_reviews".into(),
                    json!(!no_approval),
                );
            }
            (
                "/actions/permissions/workflow",
                "PUT",
                Some(Value::Object(body)),
            )
        }
        Setting::ForkApproval => (
            "/actions/permissions/fork-pr-contributor-approval",
            "PUT",
            Some(json!({ "approval_policy": "first_time_contributors" })),
        ),
    }
}

#[cfg(test)]
#[path = "baseline_tests.rs"]
mod tests;
