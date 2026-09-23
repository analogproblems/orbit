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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub created_at: String,
}
impl Message {
    pub fn new(role: &str, text: impl Into<String>) -> Self {
        Self {
            id: id(),
            role: role.into(),
            text: text.into(),
            created_at: now(),
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
        }
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
    pub scope: String,
    pub repositories: Vec<Repository>,
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
            scope: "involved".into(),
            repositories: vec![],
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
        if !["involved", "reviewRequested", "authored"].contains(&self.scope.as_str()) {
            return Err("Unknown inbox scope.".into());
        }
        let mut names = std::collections::HashSet::new();
        for repo in &self.repositories {
            validate_repo(&repo.name)?;
            if !names.insert(&repo.name) {
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
}
pub fn eligible(pr: &PullRequest, s: &Snapshot) -> bool {
    !pr.draft
        && s.settings
            .repositories
            .iter()
            .any(|r| r.name == pr.repo && r.automatic)
        && !s.reviews.iter().any(|r| {
            r.pr_id == pr.id
                && ((r.head_sha == pr.head_sha && r.base_sha == pr.base_sha) || active(&r.status))
        })
}
pub fn publishable(r: &Review, pr: &PullRequest) -> bool {
    r.status == "ready"
        && !r.report.trim().is_empty()
        && r.head_sha == pr.head_sha
        && r.base_sha == pr.base_sha
}
/// Whether a PR's report may be published without a person reading it first.
/// The agent can read files outside its worktree, so anyone able to plant
/// instructions in the diff could otherwise have local secrets posted publicly.
pub fn trusted_source(pr: &PullRequest) -> bool {
    pr.head_repo.eq_ignore_ascii_case(&pr.repo)
        && matches!(
            pr.author_association.as_str(),
            "OWNER" | "MEMBER" | "COLLABORATOR"
        )
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
        let p = pr();
        let mut s = Snapshot::default();
        s.settings.repositories.push(Repository {
            name: p.repo.clone(),
            path: "/tmp/repo".into(),
            automatic: true,
            setup_command: String::new(),
            network_access: false,
        });
        assert!(eligible(&p, &s));
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
        assert!(!trusted_source(&p));
        p.head_repo = "A/B".into();
        for association in ["OWNER", "MEMBER", "COLLABORATOR"] {
            p.author_association = association.into();
            assert!(trusted_source(&p), "{association}");
        }
        for association in ["CONTRIBUTOR", "FIRST_TIME_CONTRIBUTOR", "NONE", ""] {
            p.author_association = association.into();
            assert!(!trusted_source(&p), "{association}");
        }
        p.author_association = "MEMBER".into();
        p.head_repo = "someone/b".into();
        assert!(!trusted_source(&p));
        p.head_repo = String::new();
        assert!(!trusted_source(&p));
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
