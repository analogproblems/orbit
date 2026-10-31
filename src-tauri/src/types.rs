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
    /// When Orbit first saw the current head; empty when unknown (older
    /// state), which counts as settled.
    #[serde(default)]
    pub head_seen_at: String,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageDelivery {
    Pending,
    Sent,
    Failed,
    Interrupted,
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
    #[serde(default)]
    pub delivery: Option<MessageDelivery>,
}
impl Message {
    pub fn new(role: &str, text: impl Into<String>) -> Self {
        Self {
            id: id(),
            role: role.into(),
            text: text.into(),
            created_at: now(),
            confirmed_selection: None,
            delivery: None,
        }
    }
}
/// Who reviews a repository's pull requests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    #[default]
    Codex,
    /// The `claude` CLI, signed in with the developer's own Claude plan.
    Claude,
    /// The Antigravity CLI (`agy`), signed in with the developer's own
    /// Google account.
    Antigravity,
}
impl Provider {
    /// The reviewer's name in messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Antigravity => "Antigravity",
        }
    }
}
/// Whether Orbit can start a CLI reviewer's reviews, from the latest check
/// of the CLI.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CliStatus {
    pub ready: bool,
    pub error: Option<String>,
    pub version: Option<String>,
}
/// Your API keys, by provider.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApiKeys {
    pub openai: ApiKey,
    pub anthropic: ApiKey,
    pub google: ApiKey,
}
impl ApiKeys {
    pub fn get(&self, api: crate::api::Api) -> &ApiKey {
        match api {
            crate::api::Api::OpenAi => &self.openai,
            crate::api::Api::Anthropic => &self.anthropic,
            crate::api::Api::Google => &self.google,
        }
    }
    pub fn get_mut(&mut self, api: crate::api::Api) -> &mut ApiKey {
        match api {
            crate::api::Api::OpenAi => &mut self.openai,
            crate::api::Api::Anthropic => &mut self.anthropic,
            crate::api::Api::Google => &mut self.google,
        }
    }
}
/// One API key: never the key, only whether it is saved and, from its
/// latest check, whether the provider takes it and its models.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApiKey {
    /// Kept between runs, so Orbit only opens your password store when a
    /// key is used.
    pub saved: bool,
    pub status: CliStatus,
    pub models: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: String,
    pub pr_id: String,
    pub head_sha: String,
    pub base_sha: String,
    pub status: String,
    /// Who runs this review. Older state was always Codex.
    #[serde(default)]
    pub provider: Provider,
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
    /// The findings Codex restated as data after the review, or None when the
    /// run is unstructured (not extracted yet, extraction failed, older state).
    #[serde(default)]
    pub findings: Option<Vec<Finding>>,
    /// The verdict Codex reported with the findings.
    #[serde(default)]
    pub verdict: Option<Verdict>,
    #[serde(default)]
    pub summary: Option<String>,
    /// What the reviewer said it could not read, run or check.
    #[serde(default, alias = "testsNotRun")]
    pub not_checked: Vec<String>,
    /// False when the reviewer said it could not read or check the change.
    /// None before extraction and in older state.
    #[serde(default)]
    pub completed: Option<bool>,
    /// The findings-extraction turn is running; its output is not chat.
    #[serde(default)]
    pub extracting: bool,
    /// Why the extraction turn's latest answer couldn't be read, for the
    /// error if no answer could be.
    #[serde(skip)]
    pub extraction_problem: Option<String>,
    /// Confirmed model and effort of the extraction turn.
    #[serde(default)]
    pub extraction_selection: Option<ModelSelection>,
    /// The earlier round this re-review started from, or None for a first
    /// review of the PR.
    #[serde(default)]
    pub previous_round: Option<PreviousRound>,
    /// Custom review instructions carrying the previous round (for Claude
    /// and Antigravity, the notes their extraction turn classifies the
    /// earlier findings with).
    /// None reviews against the base exactly as a first round does.
    #[serde(default)]
    pub review_prompt: Option<String>,
    /// What the reviewer decided about each finding in `previous_round`.
    #[serde(default)]
    pub prior_findings: Vec<PriorFinding>,
    /// Blocking findings left open by this review, counted with the blocking
    /// priority in force when its findings were extracted. None without
    /// structured findings.
    #[serde(default)]
    pub blocking: Option<u32>,
    /// The blocking priority `blocking` was counted with, so each finding
    /// can say whether it blocks. None without structured findings and in
    /// older state.
    #[serde(default)]
    pub blocking_priority: Option<u8>,
    /// How many findings the reviewer's own report lists, when Orbit can
    /// count them, to notice a finding the restated list left out.
    #[serde(default)]
    pub report_findings: Option<u32>,
    /// Orbit's verdict from `blocking`; `verdict` is the model's own.
    #[serde(default)]
    pub orbit_verdict: Option<Verdict>,
    /// An automatic review whose PR moved to a new head while it was queued
    /// or running. It is never published automatically and is not a round.
    #[serde(default)]
    pub superseded: bool,
    /// The GitHub PR review a structured run was published as. Unstructured
    /// runs are published as an issue comment and leave this None.
    #[serde(default)]
    pub review_id: Option<u64>,
    /// The review's inline comments, one per finding placed on its lines.
    #[serde(default)]
    pub inline_comments: Vec<InlineComment>,
    /// Requests Orbit declined because no one approved them in time.
    #[serde(default)]
    pub declined_approvals: Vec<String>,
    /// The setup and checks Orbit ran in the repository's dev container
    /// before the review, which the reviewer is given. None when the
    /// repository doesn't use its dev container.
    #[serde(default)]
    pub checks: Option<ContainerChecks>,
}
/// What running the setup and checks in a dev container produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerChecks {
    /// The image they ran in, named after the recipe it was built from.
    pub image: String,
    pub network: bool,
    pub setup: Option<CheckStep>,
    /// None when there is no check command, or setup failed.
    pub check: Option<CheckStep>,
}
/// One command run in a dev container.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckStep {
    pub command: String,
    /// None when it was stopped at its time limit or couldn't report one.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub seconds: u64,
    /// The end of what it printed, standard output and error together.
    pub output: String,
}
impl CheckStep {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }
}
/// Where publication will put each finding of a structured run, shown before
/// it is published.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPlan {
    pub placements: Vec<Placement>,
    /// The review's own text, as it will be posted.
    pub body: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub finding_id: String,
    /// On its lines in the diff; otherwise in the review text.
    pub inline: bool,
    /// Why it is not inline.
    pub reason: Option<String>,
}
/// A finding published as an inline comment on the pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineComment {
    pub finding_id: String,
    pub comment_id: u64,
}
/// A GitHub commit status in the `orbit/review` context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitStatus {
    /// `pending`, `success`, `failure` or `error`.
    pub state: String,
    pub description: String,
    /// Why GitHub did not accept it. It is not sent again until it changes.
    #[serde(default)]
    pub error: Option<String>,
}
/// What Orbit knows of a commit's `orbit/review` status. GitHub keeps one
/// such status per commit, whichever PR or round set it, so it is tracked per
/// repository and commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitCheck {
    pub repo: String,
    pub sha: String,
    /// The PR whose round the status was last sent for.
    pub pr_id: String,
    /// The last status GitHub accepted, which is what it shows.
    #[serde(default)]
    pub accepted: Option<CommitStatus>,
    /// The last status GitHub refused, with why; not sent again until the
    /// wanted status changes.
    #[serde(default)]
    pub refused: Option<CommitStatus>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Clean,
    ChangesRequested,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable within Orbit: the review ID and the finding's index.
    pub id: String,
    pub title: String,
    /// 0 (P0, most severe) to 3 (P3).
    pub priority: u8,
    /// Where the finding is, and where its inline comment goes. Relative to
    /// the repository root; None when the reviewer reported a path outside
    /// the review's worktree.
    pub path: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    /// What is wrong. Older findings also fold the failure scenario and the
    /// fix in here.
    pub body: String,
    pub confidence: f64,
    /// None in findings from before categories.
    #[serde(default)]
    pub category: Option<Category>,
    /// Other places the finding involves, such as the caller of a broken
    /// function.
    #[serde(default)]
    pub other_locations: Vec<Location>,
    /// A concrete input or state and the wrong result it produces.
    #[serde(default)]
    pub failure_scenario: Option<String>,
    /// How the reviewer confirmed it: files read, commands run.
    #[serde(default)]
    pub evidence: Option<String>,
    /// True when the reviewer verified it by reading or running the code,
    /// false when it is likely but unverified. None in older findings.
    #[serde(default)]
    pub confirmed: Option<bool>,
    #[serde(default)]
    pub suggested_fix: Option<String>,
    /// The test or check that would show it is fixed.
    #[serde(default)]
    pub done_when: Option<String>,
    /// Anything from the reviewer's report that fits no other field.
    #[serde(default)]
    pub reviewer_notes: Option<String>,
    /// The number in its short ID (F2). Unique within its pull request and
    /// never reused, so a reply can name it in any round. None in findings
    /// from before short IDs and in security scans.
    #[serde(default)]
    pub number: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Bug,
    Security,
    DataLoss,
    Compatibility,
    TestGap,
    Performance,
    Other,
}
impl Category {
    pub const ALL: [Category; 7] = [
        Category::Bug,
        Category::Security,
        Category::DataLoss,
        Category::Compatibility,
        Category::TestGap,
        Category::Performance,
        Category::Other,
    ];
    /// The name the reviewer's JSON uses.
    pub fn key(self) -> &'static str {
        match self {
            Category::Bug => "bug",
            Category::Security => "security",
            Category::DataLoss => "data_loss",
            Category::Compatibility => "compatibility",
            Category::TestGap => "test_gap",
            Category::Performance => "performance",
            Category::Other => "other",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Category::Bug => "Bug",
            Category::Security => "Security",
            Category::DataLoss => "Data loss",
            Category::Compatibility => "Compatibility",
            Category::TestGap => "Test gap",
            Category::Performance => "Performance",
            Category::Other => "Other",
        }
    }
}
/// A place in the repository, relative to its root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// None for a path outside the review's worktree.
    pub path: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
}
impl Location {
    /// `src/a.rs:4-9`, or a note that it is outside the repository.
    pub fn label(&self) -> String {
        match &self.path {
            None => "(outside the repository)".into(),
            Some(path) if self.start_line == self.end_line => format!("{path}:{}", self.start_line),
            Some(path) => format!("{path}:{}-{}", self.start_line, self.end_line),
        }
    }
}
impl Finding {
    /// The finding's first location.
    pub fn location(&self) -> Location {
        Location {
            path: self.path.clone(),
            start_line: self.start_line,
            end_line: self.end_line,
        }
    }
    /// Its short ID, such as F2.
    pub fn short_id(&self) -> Option<String> {
        self.number.map(|n| format!("F{n}"))
    }
    /// Whether it blocks the change under `blocking_priority`, or None when
    /// that is not known.
    pub fn blocking(&self, blocking_priority: Option<u8>) -> Option<bool> {
        blocking_priority.map(|p| self.priority <= p)
    }
    /// The bold first line of a published finding, such as
    /// `**F2 · P1 · Blocking · Bug** Off-by-one in first()`.
    pub fn heading(&self, blocking_priority: Option<u8>) -> String {
        let label = [
            self.short_id(),
            Some(format!("P{}", self.priority)),
            self.blocking(blocking_priority)
                .map(|b| if b { "Blocking" } else { "Not blocking" }.to_string()),
            self.category.map(|c| c.label().to_string()),
            (self.confirmed == Some(false)).then(|| "Likely, not verified".to_string()),
        ];
        let label: Vec<String> = label.into_iter().flatten().collect();
        format!("**{}** {}", label.join(" · "), self.title.trim())
    }
    /// Everything but the title and the problem, as Markdown lines: the
    /// failure scenario, evidence, fix, other locations and so on. Empty for
    /// older findings, whose body holds it all.
    pub fn details(&self) -> String {
        let kind = [
            self.category.map(Category::label),
            self.confirmed.map(|c| {
                if c {
                    "Confirmed"
                } else {
                    "Likely, not verified"
                }
            }),
        ];
        let kind: Vec<&str> = kind.into_iter().flatten().collect();
        let mut lines = vec![];
        if !kind.is_empty() {
            lines.push(format!("_{}_", kind.join(" · ")));
        }
        lines.extend(self.detail_lines());
        lines.join("\n")
    }
    /// The labeled parts of the form, one Markdown line each, such as
    /// `**Breaks when:** …`.
    pub fn detail_lines(&self) -> Vec<String> {
        let mut lines = vec![];
        let mut line = |label: &str, text: Option<&str>| {
            if let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) {
                lines.push(format!("**{label}:** {text}"));
            }
        };
        line("Breaks when", self.failure_scenario.as_deref());
        line("Evidence", self.evidence.as_deref());
        line("Suggested fix", self.suggested_fix.as_deref());
        line("Done when", self.done_when.as_deref());
        let also: Vec<String> = self
            .other_locations
            .iter()
            .map(|l| format!("`{}`", l.label()))
            .collect();
        line("Also at", Some(&also.join(", ")));
        line("Reviewer's notes", self.reviewer_notes.as_deref());
        lines
    }
    /// Its free-text parts, for the credential scan.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        [&self.title, &self.body]
            .into_iter()
            .map(String::as_str)
            .chain(self.path.as_deref())
            .chain(
                self.other_locations
                    .iter()
                    .filter_map(|l| l.path.as_deref()),
            )
            .chain(
                [
                    &self.failure_scenario,
                    &self.evidence,
                    &self.suggested_fix,
                    &self.done_when,
                    &self.reviewer_notes,
                ]
                .into_iter()
                .filter_map(|t| t.as_deref()),
            )
    }
}
/// The findings a re-review was asked to check again, and what it was told.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousRound {
    pub review_id: String,
    pub head_sha: String,
    pub verdict: Option<Verdict>,
    /// The previous round's findings, then any earlier ones it did not
    /// report fixed. Each keeps the ID of the review that first found it.
    pub findings: Vec<Finding>,
    /// The latest classification of each of `findings` that has one, with
    /// its reason, so a later round does not reopen it without cause.
    #[serde(default)]
    pub decisions: Vec<PriorFinding>,
    /// What the author said about each of `findings` in a reply Orbit
    /// recognised, such as "Fixed F2 in abc1234": the latest one per finding.
    #[serde(default)]
    pub claims: Vec<AuthorClaim>,
    /// Comments from trusted accounts given to the reviewer.
    pub comments_included: usize,
    /// Comments from other accounts, left out.
    pub comments_omitted: usize,
}
/// A reply from the author or a trusted account saying an earlier finding
/// is fixed or declined. The reviewer still judges the finding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorClaim {
    pub finding_id: String,
    pub author: String,
    pub created_at: String,
    pub kind: ClaimKind,
    /// The commit named in "Fixed F2 in abc1234", lowercased.
    pub commit: Option<String>,
    /// Where that commit is, when Orbit could check.
    pub commit_check: Option<CommitPlace>,
    /// The reason in "Declined F2: reason".
    pub reason: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Fixed,
    Declined,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitPlace {
    /// One of the commits since the previous round.
    InChange,
    /// Already part of the head the previous round reviewed.
    Earlier,
    /// In the repository but not in this head's history.
    NotInBranch,
    NotFound,
}
impl AuthorClaim {
    /// What the author said, as a short phrase: "fixed in `abc1234`" or
    /// "declined: intended".
    pub fn summary(&self) -> String {
        match self.kind {
            ClaimKind::Fixed => match &self.commit {
                Some(c) => format!("fixed in `{}`", &c[..c.len().min(7)]),
                None => "fixed".into(),
            },
            ClaimKind::Declined => match &self.reason {
                Some(r) => format!("declined: {r}"),
                None => "declined".into(),
            },
        }
    }
    /// Where the named commit is, when that tells the reviewer something.
    pub fn commit_note(&self) -> Option<&'static str> {
        Some(match self.commit_check? {
            CommitPlace::InChange => "the commit is one of the commits since the previous round",
            CommitPlace::Earlier => {
                "the commit was already part of the head the previous round reviewed"
            }
            CommitPlace::NotInBranch => "the commit is not part of this pull request's head",
            CommitPlace::NotFound => "Orbit could not find the commit",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriorStatus {
    Fixed,
    StillOpen,
    /// The author declined it for a reason the reviewer accepts.
    DeclinedAccepted,
    /// The author declined it, but the reviewer still believes it is a defect.
    DeclinedDisputed,
}
impl PriorStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::StillOpen => "still open",
            Self::DeclinedAccepted => "declined (accepted)",
            Self::DeclinedDisputed => "declined (disputed)",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorFinding {
    /// The earlier finding's ID.
    pub id: String,
    pub status: PriorStatus,
    pub note: String,
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
        let mut review = Self::blank(pr, settings, automatic);
        let selection = settings.selection(review.provider);
        review.model = selection.model;
        review.effort = selection.effort;
        review
    }
    fn blank(pr: &PullRequest, settings: &Settings, automatic: bool) -> Self {
        Self {
            id: id(),
            pr_id: pr.id.clone(),
            head_sha: pr.head_sha.clone(),
            base_sha: pr.base_sha.clone(),
            status: "queued".into(),
            provider: settings.provider(&pr.repo),
            model: String::new(),
            effort: String::new(),
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
            findings: None,
            verdict: None,
            summary: None,
            not_checked: vec![],
            completed: None,
            extracting: false,
            extraction_problem: None,
            extraction_selection: None,
            previous_round: None,
            review_prompt: None,
            prior_findings: vec![],
            blocking: None,
            blocking_priority: None,
            report_findings: None,
            orbit_verdict: None,
            superseded: false,
            review_id: None,
            inline_comments: vec![],
            declined_approvals: vec![],
            checks: None,
        }
    }
    /// Credential-like text in the report and in the structured findings.
    pub fn credential_scan(&self) -> Vec<String> {
        let mut text = self.report.clone();
        for f in self.findings.iter().flatten() {
            for part in f.texts() {
                text.push('\n');
                text.push_str(part);
            }
        }
        // Earlier findings' titles, and what the author said about them, are
        // repeated in the "Since the last round" table and the hidden data.
        if let Some(p) = &self.previous_round {
            let claims = p.claims.iter().flat_map(|c| {
                [Some(&c.author), c.commit.as_ref(), c.reason.as_ref()]
                    .into_iter()
                    .flatten()
            });
            for part in p.findings.iter().map(|f| &f.title).chain(claims) {
                text.push('\n');
                text.push_str(part);
            }
        }
        let notes = self.prior_findings.iter().map(|p| &p.note);
        for part in self
            .summary
            .iter()
            .chain(&self.not_checked)
            .chain(&self.declined_approvals)
            .chain(notes)
        {
            text.push('\n');
            text.push_str(part);
        }
        credential_kinds(&text)
    }
    /// A warning when the reviewer's report lists more findings than its
    /// restated list and the earlier findings it still holds open, so one may
    /// have been left out.
    pub fn count_note(&self) -> Option<String> {
        let reported = self.report_findings?;
        let listed = self.findings.as_ref()?.len() as u32
            + self
                .prior_findings
                .iter()
                .filter(|p| {
                    matches!(
                        p.status,
                        PriorStatus::StillOpen | PriorStatus::DeclinedDisputed
                    )
                })
                .count() as u32;
        let plural = |n: u32| if n == 1 { "finding" } else { "findings" };
        (reported > listed).then(|| {
            format!(
                "{}’s report lists {reported} {}, but Orbit’s list has {listed}, so some may have been left out. Check the report.",
                self.provider.label(),
                plural(reported)
            )
        })
    }
    /// Ends a findings extraction. The review turn already produced a report,
    /// so the run becomes ready (or stays published) either way; without
    /// findings (`error`) it is an unstructured run. Returns false when no
    /// extraction is running.
    pub fn end_extraction(&mut self, error: Option<String>) -> bool {
        if !self.extracting {
            return false;
        }
        self.extracting = false;
        let problem = self.extraction_problem.take();
        let error = error.or_else(|| {
            self.findings.is_none().then(|| {
                let mut error = format!(
                    "{} did not restate its findings in the expected format.",
                    self.provider.label()
                );
                if let Some(problem) = problem {
                    error.push_str(&format!(" The answer was refused: {problem}"));
                }
                error
            })
        });
        if let Some(error) = error {
            self.findings = None;
            self.verdict = None;
            self.summary = None;
            self.not_checked.clear();
            self.completed = None;
            self.report_findings = None;
            self.prior_findings.clear();
            self.error = Some(format!(
                "Findings were not extracted, so this run is unstructured; its report is kept. {error}"
            ));
        }
        self.sensitive = self.credential_scan();
        self.status = if self.comment_url.is_some() {
            "posted"
        } else {
            "ready"
        }
        .into();
        self.updated_at = now();
        true
    }
    /// Ends a follow-up by restoring the outcome it set aside, so a stopped or
    /// failed chat never discards a report or publication. Returns false when
    /// no follow-up is running.
    pub fn end_follow_up(&mut self, error: Option<String>) -> bool {
        let Some(before) = self.before_follow_up.take() else {
            return false;
        };
        for message in &mut self.messages {
            if matches!(message.delivery, Some(MessageDelivery::Pending)) {
                message.delivery = Some(if error.is_some() {
                    MessageDelivery::Failed
                } else {
                    MessageDelivery::Interrupted
                });
            }
        }
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
    /// Automatic rounds per PR before the loop needs a person.
    #[serde(default = "default_round_limit")]
    pub round_limit: u32,
    /// Minutes an approval on an automatic review may wait before Orbit
    /// declines it. 0 waits for the developer however long it takes.
    #[serde(default)]
    pub approval_minutes: u32,
    /// Fast-forward the clone's checked-out branch to its upstream on each
    /// sync, when that loses nothing.
    #[serde(default)]
    pub keep_updated: bool,
    /// Who reviews this repository's pull requests.
    #[serde(default)]
    pub provider: Provider,
    /// Run the setup and check commands in the repository's dev container.
    #[serde(default)]
    pub dev_container: DevContainer,
}
/// Running a repository's setup and checks in its dev container, built from
/// the base commit's `.devcontainer/` (see `crate::devcontainer`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DevContainer {
    pub enabled: bool,
    /// Runs after the setup command, such as `npm test`. Empty runs only
    /// the setup command.
    pub check_command: String,
    /// Keeps the container for the review and lets the reviewer run its own
    /// commands there (see `crate::boxtool`).
    pub reviewer_tool: bool,
}
pub const MAX_APPROVAL_MINUTES: u32 = 1440;
fn default_round_limit() -> u32 {
    4
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
    /// The model and effort of Claude reviews. Empty uses Claude's default.
    pub claude_model: String,
    pub claude_effort: String,
    /// The model and effort of Antigravity reviews. The model must be
    /// chosen; an empty effort uses high.
    pub antigravity_model: String,
    pub antigravity_effort: String,
    pub repositories: Vec<Repository>,
    /// Extra GitHub logins, such as bots acting for the user, whose same-repository
    /// PRs are trusted like those of owners, members, and collaborators.
    pub trusted_authors: Vec<String>,
    /// Findings at this priority or more severe (0 = P0) block a clean verdict.
    pub blocking_priority: u8,
    /// Whether findings can block a clean verdict at all. When off, every
    /// review with structured findings is clean.
    pub block_findings: bool,
    /// Queue a new head automatically only once it is a poll interval old.
    pub settle_pushes: bool,
    /// Show each automatic round's result on its commit as the `orbit/review`
    /// status. Only while automatic publication is on.
    pub commit_status: bool,
    /// Show a desktop notification when something new needs the developer.
    pub notify: bool,
    /// Keep a security scan's bulky working files (`artifacts/`) after its
    /// findings are imported.
    pub keep_scan_artifacts: bool,
    /// Bit, Orbit's buddy.
    pub buddy: crate::buddy::Buddy,
    /// Plain-language write-ups of each pull request on the Decisions page.
    pub write_ups: WriteUps,
    /// Read the checks on each repository's default branch and open pull
    /// requests after each sync, and keep track of what fails.
    pub watch_ci: bool,
}
/// Who writes up each pull request's decisions for the Decisions page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Writer {
    #[default]
    Codex,
    Claude,
}
impl Writer {
    pub fn label(self) -> &'static str {
        match self {
            Writer::Codex => "Codex",
            Writer::Claude => "Claude",
        }
    }
}
/// Plain-language write-ups of each pull request's decisions, made one at a
/// time in the background with the writer's model and effort from
/// Settings. Off until you turn them on, as each one uses the writer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WriteUps {
    pub enabled: bool,
    pub writer: Writer,
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
    pub claude_model: Option<String>,
    pub claude_effort: Option<String>,
    pub antigravity_model: Option<String>,
    pub antigravity_effort: Option<String>,
    pub trusted_authors: Option<Vec<String>>,
    pub blocking_priority: Option<u8>,
    pub block_findings: Option<bool>,
    pub settle_pushes: Option<bool>,
    pub commit_status: Option<bool>,
    pub notify: Option<bool>,
    pub keep_scan_artifacts: Option<bool>,
    pub buddy: Option<crate::buddy::BuddyPatch>,
    pub write_ups: Option<WriteUps>,
    pub watch_ci: Option<bool>,
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
        if let Some(v) = self.claude_model {
            settings.claude_model = v;
        }
        if let Some(v) = self.claude_effort {
            settings.claude_effort = v;
        }
        if let Some(v) = self.antigravity_model {
            settings.antigravity_model = v;
        }
        if let Some(v) = self.antigravity_effort {
            settings.antigravity_effort = v;
        }
        if let Some(v) = self.trusted_authors {
            settings.trusted_authors = v;
        }
        if let Some(v) = self.blocking_priority {
            settings.blocking_priority = v;
        }
        if let Some(v) = self.block_findings {
            settings.block_findings = v;
        }
        if let Some(v) = self.settle_pushes {
            settings.settle_pushes = v;
        }
        if let Some(v) = self.commit_status {
            settings.commit_status = v;
        }
        if let Some(v) = self.notify {
            settings.notify = v;
        }
        if let Some(v) = self.keep_scan_artifacts {
            settings.keep_scan_artifacts = v;
        }
        if let Some(v) = self.buddy {
            v.apply(&mut settings.buddy);
        }
        if let Some(v) = self.write_ups {
            settings.write_ups = v;
        }
        if let Some(v) = self.watch_ci {
            settings.watch_ci = v;
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
            claude_model: String::new(),
            claude_effort: String::new(),
            antigravity_model: String::new(),
            antigravity_effort: String::new(),
            repositories: vec![],
            trusted_authors: vec![],
            blocking_priority: 1,
            block_findings: true,
            settle_pushes: true,
            commit_status: true,
            notify: true,
            keep_scan_artifacts: false,
            buddy: crate::buddy::Buddy::default(),
            write_ups: WriteUps::default(),
            watch_ci: true,
        }
    }
}
impl Settings {
    /// The priority at or above which findings block a clean verdict, or
    /// None when findings never block.
    pub fn blocking(&self) -> Option<u8> {
        self.block_findings.then_some(self.blocking_priority)
    }
    /// The model and effort new reviews by `provider` use.
    pub fn selection(&self, provider: Provider) -> ModelSelection {
        match provider {
            Provider::Codex => ModelSelection::workspace(self),
            Provider::Claude => crate::models::claude_selection(self),
            Provider::Antigravity => crate::models::antigravity_selection(self),
        }
    }
    /// Who reviews a repository's pull requests.
    pub fn provider(&self, repo: &str) -> Provider {
        self.repositories
            .iter()
            .find(|r| same_repo(&r.name, repo))
            .map_or_else(Provider::default, |r| r.provider)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(30..=3600).contains(&self.poll_seconds) {
            return Err("Polling interval must be 30–3600 seconds.".into());
        }
        if !(1..=4).contains(&self.concurrency) {
            return Err("Choose between 1 and 4 concurrent reviews.".into());
        }
        if self.blocking_priority > 3 {
            return Err("Choose a blocking priority from P0 to P3.".into());
        }
        self.buddy.validate()?;
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
            repo.check()?;
        }
        Ok(())
    }
}
impl Repository {
    /// A repository connected with Orbit's defaults: autopilot off, no setup
    /// command, no network, reviewed by Codex.
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            automatic: false,
            setup_command: String::new(),
            network_access: false,
            round_limit: default_round_limit(),
            approval_minutes: 0,
            keep_updated: false,
            provider: Provider::default(),
            dev_container: DevContainer::default(),
        }
    }
    /// Checks the repository's setup, apart from its name and checkout.
    pub fn check(&self) -> Result<(), String> {
        if self.setup_command.len() > 10000 {
            return Err("Setup command is too long.".into());
        }
        if self.dev_container.check_command.len() > 10000 {
            return Err("Check command is too long.".into());
        }
        if self.provider != Provider::Codex
            && !self.dev_container.enabled
            && !self.setup_command.trim().is_empty()
        {
            return Err(format!(
                "{} reviews can't run a setup command outside a dev container. Clear the setup command for {}, run it in the repository's dev container, or review it with Codex.",
                self.provider.label(),
                self.name
            ));
        }
        if self.dev_container.enabled
            && !self.dev_container.reviewer_tool
            && self.setup_command.trim().is_empty()
            && self.dev_container.check_command.trim().is_empty()
        {
            return Err(format!(
                "Add a setup or check command for {}'s dev container, let the reviewer run commands there, or turn it off.",
                self.name
            ));
        }
        if self.approval_minutes > MAX_APPROVAL_MINUTES {
            return Err("Approval time limits can be at most 24 hours.".into());
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
    /// When Codex asked, for the time limit on automatic reviews.
    #[serde(default)]
    pub requested_at: String,
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
    /// The Claude CLI, as last checked.
    pub claude: CliStatus,
    /// The Claude models Orbit offers, in the shape of `models`.
    pub claude_models: Vec<Value>,
    /// The Antigravity CLI, as last checked.
    pub antigravity: CliStatus,
    /// Docker and the Dev Container CLI, as last checked.
    pub dev_containers: CliStatus,
    /// The models Antigravity offered when last checked, in the shape of
    /// `models`.
    pub antigravity_models: Vec<Value>,
    /// Ollama at Bit's address, as last checked while it answers Bit.
    pub ollama: CliStatus,
    /// The models Ollama had when last checked.
    pub ollama_models: Vec<String>,
    /// Your API keys for OpenAI, Anthropic and Google: whether each is
    /// saved and, once checked, what it can use. The keys themselves are
    /// only in your system's password store.
    #[serde(default)]
    pub api_keys: ApiKeys,
    /// Per-PR controls of the automatic review loop.
    pub loops: Vec<LoopControl>,
    /// Commit statuses Orbit has set, so each is sent only when it changes
    /// and one left pending can be closed.
    #[serde(default)]
    pub commit_checks: Vec<CommitCheck>,
    /// Derived for display when the snapshot is emitted; never saved.
    #[serde(skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub loop_states: Vec<LoopState>,
    /// Repositories whose latest syncs failed, with how many in a row.
    pub sync_failures: Vec<SyncFailure>,
    /// Pull requests whose automatic review Orbit couldn't start, and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_failures: Vec<QueueFailure>,
    /// Polls in a row that failed before any repository could sync, such as
    /// when GitHub rejects the credential. The error is in `github_error`.
    pub poll_failures: u32,
    /// Why saving local state failed, when that turned autopilot off.
    pub state_write_error: Option<String>,
    /// Keys of the open items already announced, so each is announced once.
    pub announced: Vec<String>,
    /// Everything waiting on the developer. Derived like `loop_states`.
    #[serde(skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub attention: Vec<Attention>,
    /// The latest update of each clone kept up to date.
    pub clone_updates: Vec<CloneUpdate>,
    /// The Codex account's usage limits, when it has any. Read from Codex
    /// while Orbit runs; shown in the app but never saved (see `persist`).
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::usage::Usage>,
    /// Repository security scans, newest first.
    pub scans: Vec<SecurityScan>,
    /// Bit's notes and what it last saw of each loop.
    pub buddy_feed: crate::buddy::Feed,
    /// How Bit feels. Derived like `loop_states`.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub mood: Option<crate::buddy::Mood>,
    /// Reading each repository's pull-request history for the Decisions
    /// page.
    pub ledger: LedgerStatus,
    /// Each connected repository's CI, as last read.
    pub ci: Vec<crate::ci::RepoCi>,
    /// What Orbit saw go wrong, open or closed lately (see
    /// `crate::problems`).
    pub problems: Vec<crate::problems::Problem>,
    /// Orbit's comments and issues that tell agents about failing CI (see
    /// `crate::ops`).
    pub ops: Vec<crate::ops::Thread>,
    /// Each write Orbit made on GitHub for them, recorded before it was
    /// sent; the newest last.
    pub journal: Vec<crate::ops::Write>,
    /// Why the jobs still failing on open problems failed, as Orbit read
    /// them (see `crate::logs`).
    pub failures: Vec<crate::logs::Failure>,
    /// What each connected repository's files say about its CI, as Orbit
    /// last read them (see `crate::workflows`).
    pub setups: Vec<crate::workflows::Setup>,
    /// The self-hosted runners of each connected repository, and of each
    /// organization that owns one, as Orbit last read them (see
    /// `crate::runners`).
    pub runners: Vec<crate::runners::Place>,
    /// Each connected repository's GitHub Actions caches and artifacts, as
    /// Orbit last read them (see `crate::actions_storage`).
    pub actions_storage: Vec<crate::actions_storage::Storage>,
    /// What GitHub last said about whether each pull request Orbit reviews
    /// can merge (see `crate::readiness`).
    pub merges: Vec<crate::readiness::Merge>,
    /// Whether each pull request Orbit reviews is clear to merge. Derived
    /// like `loop_states`.
    #[serde(skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub readiness: Vec<crate::readiness::Readiness>,
}
/// How reading the pull-request history is going.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LedgerStatus {
    /// A read is running.
    pub refreshing: bool,
    /// The pull request being read and how many are left, while a read runs.
    pub progress: Option<LedgerProgress>,
    /// Each connected repository's latest read.
    pub repos: Vec<LedgerRepo>,
    /// Goes up whenever stored history or a write-up changes, so the page
    /// knows to load it again.
    pub version: u64,
    /// The pull request being written up and how many are left, while
    /// write-ups run.
    pub writing: Option<LedgerProgress>,
    /// Why write-ups stopped or are waiting, when they did.
    pub write_error: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LedgerProgress {
    pub repo: String,
    /// The pull request being written up.
    pub number: Option<u64>,
    /// Pull requests read or written up so far, and how many need it.
    pub done: u32,
    pub total: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LedgerRepo {
    pub repo: String,
    /// When it was last read in full.
    pub refreshed_at: Option<String>,
    /// Why the latest read stopped, when it did.
    pub error: Option<String>,
}
/// How far a security scan has come, as the Codex Security plugin reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanProgress {
    /// preflight, threat_model, discovery, validation, attack_path or reporting.
    pub phase: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    /// What `completed` and `total` count, such as files or reviews.
    pub unit: Option<String>,
    /// Findings worth reporting so far.
    pub findings: Option<u64>,
    pub updated_at: String,
}
/// A whole-repository security scan with the Codex Security plugin, run on a
/// detached worktree at one commit of the repository's default branch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecurityScan {
    pub id: String,
    pub repo: String,
    /// "standard" or "deep".
    pub mode: String,
    /// preparing, running, cancelling (stopping: waiting for the plugin to
    /// confirm its scan stopped), importing, completed, failed, cancelled or
    /// interrupted (Orbit or Codex stopped while it ran).
    pub status: String,
    /// While stopping, the status to record once the plugin confirms the
    /// stop (cancelled when absent), and the reason to show with it.
    pub stop_outcome: Option<String>,
    pub stop_reason: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
    /// The default branch and commit scanned, once fetched.
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub model: String,
    pub effort: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub worktree: Option<String>,
    /// The plugin's own ID and folder for the scan, once its tools report them.
    pub plugin_scan_id: Option<String>,
    pub scan_dir: Option<String>,
    pub progress: Option<ScanProgress>,
    /// The deep scan's Daybreak access check: granted, not_granted or unknown.
    pub daybreak: Option<String>,
    pub findings: Vec<Finding>,
    pub has_report: bool,
    /// Bytes the scan's kept files and leftover working files use.
    pub disk_bytes: Option<u64>,
    pub error: Option<String>,
    /// Problems that did not fail the scan, such as files left behind.
    pub warnings: Vec<String>,
    /// Codex's latest message in the scan thread.
    pub last_message: Option<String>,
}
pub fn scan_active(status: &str) -> bool {
    matches!(status, "preparing" | "running" | "cancelling" | "importing")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneUpdateState {
    /// The branch was fast-forwarded.
    Updated,
    /// The branch already matched its upstream.
    Current,
    /// Updating would lose or move local work, so the clone was left alone.
    Skipped,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneUpdate {
    pub repo: String,
    pub state: CloneUpdateState,
    pub message: String,
    pub at: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFailure {
    pub repo: String,
    pub count: u32,
    pub error: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueFailure {
    pub pr_id: String,
    pub head_sha: String,
    pub error: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    Approval,
    Held,
    Failed,
    Loop,
    AutopilotPaused,
    SyncFailing,
    /// A reviewer, or dev containers, that a repository needs isn't
    /// connected.
    Disconnected,
    /// CI fails on a repository's default branch.
    CiFailing,
    /// An agent asks for a person about a failing check.
    CiQuestion,
    /// A repository's runs have waited a long time for a runner.
    RunsWaiting,
    /// A pull request has waited on a person, such as a reviewer, for a
    /// long time (`crate::ops::NUDGE_PERSON`).
    Stuck,
}
/// Something only the developer can resolve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    /// Stays the same while the item is open, so it is announced once.
    pub key: String,
    pub kind: AttentionKind,
    pub pr_id: Option<String>,
    pub review_id: Option<String>,
    /// The request to answer, for an approval.
    pub approval_id: Option<Value>,
    pub repo: Option<String>,
    pub reason: String,
    /// The control that resolves it, which Bit offers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<crate::buddy::Fix>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopControl {
    pub pr_id: String,
    /// The developer paused the loop for this PR.
    pub stopped: bool,
    /// Rounds granted beyond the repository's round limit by Continue loop.
    pub extra_rounds: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Reviewing,
    WaitingForAuthor,
    Clean,
    NeedsYou,
    Stopped,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopState {
    pub pr_id: String,
    pub status: LoopStatus,
    /// Why the loop needs a person.
    pub reason: Option<String>,
    /// Automatic rounds so far, and how many are allowed.
    pub rounds: u32,
    pub round_limit: u32,
    /// The rounds' review IDs, oldest first.
    pub round_ids: Vec<String>,
    /// The PR has left the inbox (merged, closed, or no longer requested),
    /// so the loop is over whatever its last state was.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub closed: bool,
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
        && crate::review_loop::allows_new_round(pr, s)
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
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Default::default(),
            dev_container: Default::default(),
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
    #[test]
    fn refuses_a_setup_command_for_claude_reviews() {
        let mut s = Settings::default();
        s.repositories.push(Repository {
            name: "acme/app".into(),
            path: "/tmp/app".into(),
            automatic: false,
            setup_command: "npm ci".into(),
            network_access: false,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Provider::Claude,
            dev_container: Default::default(),
        });
        assert!(s.validate().unwrap_err().contains("setup command"));
        s.repositories[0].setup_command = " ".into();
        assert!(s.validate().is_ok());
        s.repositories[0].setup_command = "npm ci".into();
        s.repositories[0].provider = Provider::Codex;
        assert!(s.validate().is_ok());
    }
    #[test]
    fn a_dev_container_runs_setup_for_any_reviewer_but_needs_a_command() {
        let mut s = Settings::default();
        s.repositories.push(Repository {
            name: "acme/app".into(),
            path: "/tmp/app".into(),
            automatic: false,
            setup_command: "npm ci".into(),
            network_access: false,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Provider::Antigravity,
            dev_container: DevContainer {
                enabled: true,
                check_command: String::new(),
                reviewer_tool: false,
            },
        });
        assert!(s.validate().is_ok());
        s.repositories[0].setup_command = String::new();
        let error = s.validate().unwrap_err();
        assert!(error.contains("Add a setup or check command"), "{error}");
        // The reviewer's own commands are reason enough for the container.
        s.repositories[0].dev_container.reviewer_tool = true;
        assert!(s.validate().is_ok());
        s.repositories[0].dev_container.reviewer_tool = false;
        s.repositories[0].dev_container.check_command = "npm test".into();
        assert!(s.validate().is_ok());
        s.repositories[0].dev_container.check_command = "x".repeat(10001);
        assert!(s.validate().unwrap_err().contains("too long"));
        // Older settings have no dev container, and stay as they were.
        let mut saved = serde_json::to_value(&s.repositories[0]).unwrap();
        saved.as_object_mut().unwrap().remove("devContainer");
        let repo: Repository = serde_json::from_value(saved).unwrap();
        assert_eq!(repo.dev_container, DevContainer::default());
    }
}
