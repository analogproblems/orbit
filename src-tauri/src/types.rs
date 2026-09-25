pub use crate::models::ModelSelection;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub id: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub author: String,
    pub body: String,
    pub url: String,
    pub head_sha: String,
    pub base_sha: String,
    pub head_ref: String,
    pub base_ref: String,
    pub updated_at: String,
    pub draft: bool,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub review_requested: bool,
    pub labels: Vec<String>,
    /// `owner/name` of the head branch's repository; empty when unknown or the fork was deleted.
    #[serde(default)]
    pub head_repo: String,
    /// GitHub's `author_association`, e.g. OWNER, MEMBER, COLLABORATOR, CONTRIBUTOR, NONE.
    #[serde(default)]
    pub author_association: String,
    /// `owner/name` of the base repository as GitHub reports it now, which
    /// differs from `repo` after a rename or transfer.
    #[serde(default)]
    pub base_repo: String,
}
/// A repository the signed-in user can access, offered when connecting one.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySummary {
    pub name: String,
    pub private: bool,
    pub description: String,
    pub pushed_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub created_at: String,
    #[serde(default)]
    pub confirmed_selection: Option<ModelSelection>,
}
impl Message {
    pub fn new(role: &str, text: impl Into<String>) -> Self {
        Self {
            id: id(),
            role: role.into(),
            text: text.into(),
            created_at: now(),
            confirmed_selection: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: String,
    pub pr_id: String,
    pub head_sha: String,
    pub base_sha: String,
    pub status: String,
    pub model: String,
    pub effort: String,
    #[serde(default)]
    pub confirmed_review_selection: Option<ModelSelection>,
    #[serde(default)]
    pub legacy_chat_selection: Option<ModelSelection>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub worktree: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<Message>,
    pub report: String,
    pub error: Option<String>,
    pub comment_url: Option<String>,
    pub automatic: bool,
    pub is_review: bool,
    /// Kinds of credential-like text found in the report, shown before publication.
    #[serde(default)]
    pub sensitive: Vec<String>,
    /// The review's outcome, set aside while a follow-up question runs.
    #[serde(default)]
    pub before_follow_up: Option<Outcome>,
    #[serde(default)]
    pub cleanup_pending: bool,
    /// None in older state: conservatively recheck PR closure on recovery.
    #[serde(default)]
    pub cleanup_closed_only: Option<bool>,
    #[serde(default)]
    pub cleanup_error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub status: String,
    pub error: Option<String>,
}
pub fn active(status: &str) -> bool {
    matches!(
        status,
        "queued" | "preparing" | "reviewing" | "awaitingApproval"
    )
}
impl Review {
    pub fn new(pr: &PullRequest, settings: &Settings, automatic: bool) -> Self {
        Self {
            id: id(),
            pr_id: pr.id.clone(),
            head_sha: pr.head_sha.clone(),
            base_sha: pr.base_sha.clone(),
            status: "queued".into(),
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            confirmed_review_selection: None,
            legacy_chat_selection: None,
            thread_id: None,
            turn_id: None,
            worktree: None,
            created_at: now(),
            updated_at: now(),
            messages: vec![],
            report: String::new(),
            error: None,
            comment_url: None,
            automatic,
            is_review: true,
            sensitive: vec![],
            before_follow_up: None,
            cleanup_pending: false,
            cleanup_closed_only: None,
            cleanup_error: None,
        }
    }
    /// Ends a follow-up by restoring the outcome it set aside, so a stopped or
    /// failed chat never discards a report or publication. Returns false when
    /// no follow-up is running.
    pub fn end_follow_up(&mut self, error: Option<String>) -> bool {
        let Some(before) = self.before_follow_up.take() else {
            return false;
        };
        self.status = before.status;
        self.error = error.or(before.error);
        self.updated_at = now();
        true
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub name: String,
    pub path: String,
    pub automatic: bool,
    pub setup_command: String,
    pub network_access: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub automatic: bool,
    pub auto_post: bool,
    pub poll_seconds: u64,
    pub concurrency: usize,
    pub model: String,
    pub effort: String,
    pub repositories: Vec<Repository>,
    /// Extra GitHub logins, such as bots acting for the user, whose same-repository
    /// PRs are trusted like those of owners, members, and collaborators.
    pub trusted_authors: Vec<String>,
}
/// Only supplied fields change. Repository mutations have separate commands.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub automatic: Option<bool>,
    pub auto_post: Option<bool>,
    pub poll_seconds: Option<u64>,
    pub concurrency: Option<usize>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub trusted_authors: Option<Vec<String>>,
}
impl SettingsPatch {
    pub fn apply(self, settings: &mut Settings) {
        if let Some(v) = self.automatic {
            settings.automatic = v;
        }
        if let Some(v) = self.auto_post {
            settings.auto_post = v;
        }
        if let Some(v) = self.poll_seconds {
            settings.poll_seconds = v;
        }
        if let Some(v) = self.concurrency {
            settings.concurrency = v;
        }
        if let Some(v) = self.model {
            settings.model = v;
        }
        if let Some(v) = self.effort {
            settings.effort = v;
        }
        if let Some(v) = self.trusted_authors {
            settings.trusted_authors = v;
        }
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            automatic: false,
            auto_post: false,
            poll_seconds: 120,
            concurrency: 2,
            model: String::new(),
            effort: String::new(),
            repositories: vec![],
            trusted_authors: vec![],
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(30..=3600).contains(&self.poll_seconds) {
            return Err("Polling interval must be 30–3600 seconds.".into());
        }
        if !(1..=4).contains(&self.concurrency) {
            return Err("Choose between 1 and 4 concurrent reviews.".into());
        }
        if self.trusted_authors.len() > 50 {
            return Err("Trust at most 50 accounts.".into());
        }
        let mut logins = std::collections::HashSet::new();
        for login in &self.trusted_authors {
            if !valid_login(login) {
                return Err(format!("“{login}” is not a GitHub account name."));
            }
            if !logins.insert(login.to_ascii_lowercase()) {
                return Err(format!("{login} is already trusted."));
            }
        }
        let mut names = std::collections::HashSet::new();
        for repo in &self.repositories {
            validate_repo(&repo.name)?;
            if !names.insert(repo.name.to_ascii_lowercase()) {
                return Err("Each repository can only be configured once.".into());
            }
            if !std::path::Path::new(&repo.path).is_absolute() {
                return Err(format!("Use an absolute checkout path for {}.", repo.name));
            }
            if repo.setup_command.len() > 10000 {
                return Err("Setup command is too long.".into());
            }
        }
        Ok(())
    }
}
pub fn validate_repo(repo: &str) -> Result<(), String> {
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || p.starts_with('-')
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    {
        return Err("Repository must be owner/name on github.com.".into());
    }
    Ok(())
}
/// A GitHub user login, or an app's bot login such as `claude[bot]`.
pub fn valid_login(login: &str) -> bool {
    let name = login.strip_suffix("[bot]").unwrap_or(login);
    (1..=39).contains(&name.len())
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}
/// PR ids ignore case, like GitHub repository names, so changing a repository's
/// configured capitalization keeps its reviews attached to their PRs.
pub fn pr_id(repo: &str, number: u64) -> String {
    format!("{repo}#{number}").to_ascii_lowercase()
}
pub fn same_repo(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
pub fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: Value,
    pub review_id: String,
    pub method: String,
    pub params: Value,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snapshot {
    pub prs: Vec<PullRequest>,
    pub history_prs: Vec<PullRequest>,
    pub reviews: Vec<Review>,
    pub settings: Settings,
    pub models: Vec<Value>,
    pub approvals: Vec<Approval>,
    pub github_user: Option<String>,
    pub github_error: Option<String>,
    pub codex_error: Option<String>,
    pub last_poll: Option<String>,
    pub polling: bool,
    pub codex_ready: bool,
    pub model_catalog_ready: bool,
    pub model_configuration_error: Option<String>,
    pub model_selection_notice: Option<String>,
}
pub fn eligible(pr: &PullRequest, s: &Snapshot) -> bool {
    !pr.draft
        && trusted_source(pr, &s.settings.trusted_authors)
        && s.settings
            .repositories
            .iter()
            .any(|r| same_repo(&r.name, &pr.repo) && r.automatic)
        && !s.reviews.iter().any(|r| {
            r.pr_id == pr.id
                && ((r.head_sha == pr.head_sha && r.base_sha == pr.base_sha) || active(&r.status))
        })
}
/// Whether a review's report matches the PR as it is now. `publish()` checks
/// the review's status itself when it claims the review.
pub fn publishable(r: &Review, pr: &PullRequest) -> bool {
    !r.report.trim().is_empty() && r.head_sha == pr.head_sha && r.base_sha == pr.base_sha
}
/// Whether a PR may be reviewed and published without a person starting it.
/// The agent can read files outside its worktree and setup runs PR code, so
/// anyone able to plant instructions in the diff could otherwise have local
/// secrets posted publicly.
pub fn trusted_source(pr: &PullRequest, trusted_authors: &[String]) -> bool {
    let base = if pr.base_repo.is_empty() {
        &pr.repo
    } else {
        &pr.base_repo
    };
    pr.head_repo.eq_ignore_ascii_case(base)
        && (matches!(
            pr.author_association.as_str(),
            "OWNER" | "MEMBER" | "COLLABORATOR"
        ) || trusted_authors
            .iter()
            .any(|login| login.eq_ignore_ascii_case(&pr.author)))
}
static CREDENTIAL_PATTERNS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    [
        (
            "GitHub token",
            r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})",
        ),
        ("private key", r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----"),
        ("AWS access key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
        ("OpenAI or Anthropic API key", r"\bsk-[A-Za-z0-9_-]{20,}"),
        ("Slack token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
        ("Google API key", r"\bAIza[0-9A-Za-z_-]{35}"),
        ("Stripe key", r"\b[rs]k_live_[0-9A-Za-z]{20,}"),
        ("npm token", r"\bnpm_[A-Za-z0-9]{36}\b"),
        (
            "JSON web token",
            r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
        ),
    ]
    .into_iter()
    .map(|(label, pattern)| (label, Regex::new(pattern).unwrap()))
    .collect()
});
/// Kinds of well-known credential formats found in `text`. A best-effort net
/// behind `trusted_source`, not a guarantee: arbitrary secrets have no format.
pub fn credential_kinds(text: &str) -> Vec<String> {
    CREDENTIAL_PATTERNS
        .iter()
        .filter(|(_, pattern)| pattern.is_match(text))
        .map(|(label, _)| label.to_string())
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn pr() -> PullRequest {
        serde_json::from_value(serde_json::json!({"id":"a/b#1","repo":"a/b","number":1,"title":"Fix","author":"a","body":"","url":"https://github.com/a/b/pull/1","headSha":"a".repeat(40),"baseSha":"b".repeat(40),"headRef":"fix","baseRef":"main","updatedAt":now(),"draft":false,"additions":1,"deletions":0,"changedFiles":1,"reviewRequested":true,"labels":[]})).unwrap()
    }
    #[test]
    fn validates_repository_names() {
        for bad in ["../repo", "a/../../b", "-x/b", "a/b/c", "a/b;curl", "a/b\n"] {
            assert!(validate_repo(bad).is_err(), "{bad}")
        }
        assert!(validate_repo("acme/my-repo.js").is_ok());
    }
    #[test]
    fn automatic_jobs_are_deduplicated_by_commit_and_base() {
        let mut p = pr();
        p.head_repo = p.repo.clone();
        p.author_association = "OWNER".into();
        let mut s = Snapshot::default();
        s.settings.repositories.push(Repository {
            name: p.repo.clone(),
            path: "/tmp/repo".into(),
            automatic: true,
            setup_command: String::new(),
            network_access: false,
        });
        assert!(eligible(&p, &s));
        let mut fork = p.clone();
        fork.head_repo = "someone/b".into();
        assert!(!eligible(&fork, &s));
        let mut r = Review::new(&p, &s.settings, true);
        r.status = "failed".into();
        s.reviews.push(r);
        assert!(!eligible(&p, &s));
        let mut p2 = p.clone();
        p2.head_sha = "c".repeat(40);
        assert!(eligible(&p2, &s));
        s.reviews[0].status = "reviewing".into();
        assert!(!eligible(&p2, &s));
    }
    #[test]
    fn prevents_stale_or_empty_publication() {
        let mut p = pr();
        let mut r = Review::new(&p, &Settings::default(), false);
        r.status = "ready".into();
        assert!(!publishable(&r, &p));
        r.report = "Finding".into();
        assert!(publishable(&r, &p));
        p.head_sha = "c".repeat(40);
        assert!(!publishable(&r, &p));
        p.head_sha = r.head_sha.clone();
        p.base_sha = "d".repeat(40);
        assert!(!publishable(&r, &p));
    }
    #[test]
    fn only_repository_insiders_are_trusted_for_automatic_publication() {
        let mut p = pr();
        // State saved before provenance was recorded deserializes as untrusted.
        assert!(!trusted_source(&p, &[]));
        p.head_repo = "A/B".into();
        for association in ["OWNER", "MEMBER", "COLLABORATOR"] {
            p.author_association = association.into();
            assert!(trusted_source(&p, &[]), "{association}");
        }
        for association in ["CONTRIBUTOR", "FIRST_TIME_CONTRIBUTOR", "NONE", ""] {
            p.author_association = association.into();
            assert!(!trusted_source(&p, &[]), "{association}");
        }
        p.author_association = "MEMBER".into();
        p.head_repo = "someone/b".into();
        assert!(!trusted_source(&p, &[]));
        p.head_repo = String::new();
        assert!(!trusted_source(&p, &[]));
    }
    #[test]
    fn trusted_accounts_extend_trust_only_to_same_repository_prs() {
        let mut p = pr();
        p.head_repo = p.repo.clone();
        p.author = "claude[bot]".into();
        p.author_association = "NONE".into();
        let trusted = ["Claude[bot]".to_string()];
        assert!(!trusted_source(&p, &[]));
        assert!(trusted_source(&p, &trusted));
        assert!(!trusted_source(&p, &["codex[bot]".to_string()]));
        p.head_repo = "someone/b".into();
        assert!(!trusted_source(&p, &trusted));
    }
    #[test]
    fn renamed_repositories_are_not_mistaken_for_forks() {
        let mut p = pr();
        p.author_association = "OWNER".into();
        p.base_repo = "a/renamed".into();
        p.head_repo = "a/renamed".into();
        assert!(trusted_source(&p, &[]));
        p.head_repo = "someone/renamed".into();
        assert!(!trusted_source(&p, &[]));
    }
    #[test]
    fn validates_trusted_account_names() {
        for good in ["claude[bot]", "dependabot[bot]", "JW-JCLS", "a"] {
            assert!(valid_login(good), "{good}");
        }
        for bad in [
            "",
            "[bot]",
            "-lead",
            "has space",
            "a/b",
            "x[bot][bot]",
            &"a".repeat(40),
        ] {
            assert!(!valid_login(bad), "{bad}");
        }
        let mut s = Settings {
            trusted_authors: vec!["claude[bot]".into(), "Claude[bot]".into()],
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        s.trusted_authors.pop();
        assert!(s.validate().is_ok());
    }
    #[test]
    fn detects_credential_shaped_text() {
        // Built at runtime so secret scanners do not flag this file.
        let github = format!("token: ghp_{}", "a1".repeat(18));
        let key = format!("-----BEGIN OPENSSH {} KEY-----", "PRIVATE");
        let aws = format!("AKIA{}", "ABCDEFGH23456789");
        assert_eq!(credential_kinds(&github), ["GitHub token"]);
        assert_eq!(credential_kinds(&key), ["private key"]);
        assert_eq!(
            credential_kinds(&format!("{aws} and {key}")),
            ["private key", "AWS access key"]
        );
        let ordinary = format!(
            "### [P2] Token refresh skips expiry\n`src/auth/task-scheduler.rs:42` uses \
             `generate_token()`; reviewed head `{}`. Rotate the api_key in tests.",
            "a".repeat(40)
        );
        assert!(credential_kinds(&ordinary).is_empty());
    }
    #[test]
    fn validates_settings_bounds() {
        let mut s = Settings {
            poll_seconds: 1,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
        s.poll_seconds = 60;
        s.concurrency = 5;
        assert!(s.validate().is_err());
        s.concurrency = 2;
        assert!(s.validate().is_ok());
    }
}
