//! The history behind the Decisions page: each pull request's description
//! and review conversation as Orbit last read it from GitHub, and what Orbit
//! works out from it without any model. From the comments it rebuilds the
//! reviewer's rounds (Orbit's own published reviews and Milo's), the
//! builder's `Fixed F2 in abc1234` and `Declined F2: reason` replies, where
//! each finding ended up, a few numbers per pull request, and the things a
//! person should glance at.
//!
//! Comment text is untrusted: it is only parsed here, never run, and the
//! app shows it as text.
use crate::types::{Category, ClaimKind, PriorStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod assemble;
mod milo;
mod narrative;
mod orbit;
mod replies;
mod roles;
mod store;
pub mod writeup;

pub use assemble::derive;
pub use narrative::parse as parse_narrative;
pub use store::{
    from_github, load, load_repo, load_writeups, prune, save, save_writeup, stored_path, REVISIONS,
};

#[cfg(test)]
mod assemble_tests;
#[cfg(test)]
mod milo_tests;
#[cfg(test)]
mod narrative_tests;
#[cfg(test)]
mod orbit_tests;
#[cfg(test)]
mod replies_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod writeup_tests;

/// The version of the stored form; a file of another version is read again
/// from GitHub.
pub const STORED_VERSION: u32 = 1;

/// One pull request and its conversation, as saved in Orbit's data folder.
/// Only the fields Orbit reads are kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredPr {
    pub version: u32,
    pub repo: String,
    /// When Orbit last read it from GitHub.
    pub fetched_at: String,
    pub pr: PrInfo,
    pub comments: Vec<StoredComment>,
    pub commits: Vec<CommitRef>,
    /// Paths of the changed files.
    pub files: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// `open` or `closed`.
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub merged_at: Option<String>,
    pub merged_by: Option<String>,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentKind {
    /// A comment in the pull request's conversation.
    #[default]
    Issue,
    /// A pull request review's own text.
    Review,
    /// A comment on the changed lines.
    ReviewComment,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredComment {
    pub id: u64,
    pub kind: CommentKind,
    pub author: String,
    pub body: String,
    pub url: String,
    /// When it was posted; a review's `submitted_at`.
    pub created_at: String,
    pub updated_at: String,
    /// For a comment on the changed lines: the file and lines.
    pub path: Option<String>,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    /// The first comment of the thread a reply is in.
    pub in_reply_to: Option<u64>,
    /// The pull request review a comment on the changed lines belongs to.
    pub review_id: Option<u64>,
    /// Earlier texts of the comment that Orbit saw before it was edited,
    /// oldest first. A reviewer that edits its comment in place for a new
    /// round would otherwise lose the earlier round.
    pub revisions: Vec<Revision>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Revision {
    pub body: String,
    /// The comment's `updatedAt` while it had this text.
    pub updated_at: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitRef {
    pub sha: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundVerdict {
    Clean,
    ChangesRequested,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Orbit's published review.
    Orbit,
    /// Milo, the reviewer that posts "## Milo PR review" comments, in any
    /// of the shapes its reviews took over time.
    Milo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The coding agent that wrote the change.
    Builder,
    Reviewer,
    /// A person.
    Human,
    Unknown,
}
/// A place in the repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub path: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
}
/// A problem a reviewer raised. Orbit's own reviews fill every part; Milo's
/// fill what can be read from its text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    /// `F2`, when the reviewer numbered it.
    pub short_id: Option<String>,
    pub number: Option<u32>,
    pub title: String,
    /// 0 (P0, most severe) to 3.
    pub priority: u8,
    pub blocking: Option<bool>,
    pub category: Option<Category>,
    pub locations: Vec<Place>,
    pub problem: String,
    pub failure_scenario: Option<String>,
    pub evidence: Option<String>,
    /// Whether the reviewer verified it: false when it said "likely, not
    /// verified", None when it didn't say.
    pub confirmed: Option<bool>,
    pub suggested_fix: Option<String>,
    pub done_when: Option<String>,
    pub reviewer_notes: Option<String>,
    pub confidence: Option<f64>,
    /// Whether it was posted on the changed lines.
    pub inline: Option<bool>,
}
impl Finding {
    fn new(id: String, title: String, priority: u8) -> Self {
        Self {
            id,
            short_id: None,
            number: None,
            title,
            priority,
            blocking: None,
            category: None,
            locations: vec![],
            problem: String::new(),
            failure_scenario: None,
            evidence: None,
            confirmed: None,
            suggested_fix: None,
            done_when: None,
            reviewer_notes: None,
            confidence: None,
            inline: None,
        }
    }
}
/// What the builder said about a finding, as the reviewer recorded it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorSaid {
    pub kind: ClaimKind,
    pub commit: Option<String>,
    pub reason: Option<String>,
}
/// A finding from an earlier round, with the reviewer's ruling this round.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Earlier {
    pub id: String,
    pub short_id: Option<String>,
    pub title: String,
    pub priority: Option<u8>,
    pub status: Option<PriorStatus>,
    pub note: Option<String>,
    pub author_said: Option<AuthorSaid>,
}
/// One review of one head commit.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Round {
    pub review_id: Option<String>,
    pub format: Format,
    pub round: Option<u32>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub verdict: RoundVerdict,
    pub blocking: Option<u32>,
    pub blocking_priority: Option<u8>,
    pub reviewer: Option<String>,
    pub summary: Option<String>,
    /// Findings raised for the first time in this round.
    pub findings: Vec<Finding>,
    pub earlier_findings: Vec<Earlier>,
    pub not_checked: Vec<String>,
    /// False when the reviewer said it couldn't read or check the change.
    pub completed: Option<bool>,
    pub comment_id: u64,
    pub url: String,
    pub created_at: String,
    /// The review's text, for the write-up; not sent to the app.
    #[serde(skip)]
    pub body: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimScope {
    /// The line names the findings: `Fixed F2 in abc1234`.
    Named,
    /// A reply in a finding's own thread: `Fixed in abc1234`.
    Thread,
    /// An older "all findings are fixed" line.
    All,
}
/// A builder's reply line about findings.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub comment_id: u64,
    pub url: String,
    pub created_at: String,
    pub author: String,
    /// The short ID numbers named; empty for a thread or "all" line.
    pub numbers: Vec<u32>,
    pub kind: ClaimKind,
    pub commit: Option<String>,
    pub reason: Option<String>,
    pub scope: ClaimScope,
    /// The finding whose thread the reply is in.
    pub thread_finding_id: Option<String>,
}
/// The builder's own account of the change, from the description.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Narrative {
    pub before: Option<String>,
    pub after: Option<String>,
    pub how: Option<String>,
    pub decisions: Option<String>,
    pub checks: Option<String>,
    pub epic: Option<u64>,
    pub stage: Option<String>,
    pub closes: Vec<u64>,
    /// Every section by its lowercased heading or label, in order.
    pub sections: Vec<(String, String)>,
    /// Text before the first heading or labelled paragraph.
    pub lead: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalStatus {
    Fixed,
    StillOpen,
    DeclinedAccepted,
    DeclinedDisputed,
    /// No ruling yet.
    Open,
}
impl From<PriorStatus> for FinalStatus {
    fn from(s: PriorStatus) -> Self {
        match s {
            PriorStatus::Fixed => Self::Fixed,
            PriorStatus::StillOpen => Self::StillOpen,
            PriorStatus::DeclinedAccepted => Self::DeclinedAccepted,
            PriorStatus::DeclinedDisputed => Self::DeclinedDisputed,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ruling {
    pub round: u32,
    pub at: String,
    pub status: PriorStatus,
    pub note: Option<String>,
}
/// One finding's way from the round that raised it to its last ruling.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trajectory {
    pub finding_id: String,
    pub short_id: Option<String>,
    pub opened_round: u32,
    pub opened_at: String,
    pub priority: u8,
    pub blocking: Option<bool>,
    pub category: Option<Category>,
    pub title: String,
    /// The builder's latest word on it.
    pub claim: Option<Claim>,
    pub rulings: Vec<Ruling>,
    pub final_status: FinalStatus,
    /// Minutes from the finding to the builder's "fixed" reply.
    pub minutes_to_claim: Option<i64>,
    /// Where it stood when the pull request merged, from the rounds up to
    /// then. None when it didn't merge or the finding came after.
    pub status_at_merge: Option<FinalStatus>,
    /// Merged while it was open, still open or disputed.
    pub open_at_merge: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub rounds: u32,
    pub findings: u32,
    pub blocking_findings: u32,
    pub fixed: u32,
    pub declined: u32,
    pub disputed: u32,
    pub open_at_merge: u32,
    /// Minutes from opening to the first clean round.
    pub minutes_to_clean: Option<i64>,
    pub minutes_to_merge: Option<i64>,
    pub first_verdict: Option<RoundVerdict>,
    pub final_verdict: Option<RoundVerdict>,
    /// Merged within 45 minutes of a clean last round, which is how the
    /// loop merges. An inference from timing, shown as one.
    pub merged_by_loop: Option<bool>,
    /// What the last round said it could not check.
    pub verification_gaps: u32,
    /// Findings the reviewer called likely but never verified.
    pub likely_not_verified: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    MergedWithOpenFindings,
    DisputedDecline,
    RepeatBlocking,
    NoReviewBeforeMerge,
    ReviewerCouldNotCheck,
    LikelyNotVerified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Glance {
    /// Someone should decide something.
    Act,
    /// Good to know.
    Look,
}
/// Something worth a person's glance, found without any model.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    pub kind: FlagKind,
    pub attention: Glance,
    pub text: String,
    pub finding_id: Option<String>,
    pub round: Option<u32>,
    pub url: Option<String>,
}
/// A comment, with who Orbit thinks wrote it. In this workflow the builder,
/// the reviewer and the person often post from the same GitHub account, so
/// the role comes from the text.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentRef {
    pub id: u64,
    pub kind: CommentKind,
    pub author: String,
    pub role: Role,
    pub created_at: String,
    pub url: String,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub in_reply_to: Option<u64>,
    pub review_id: Option<u64>,
    /// The finding an inline comment raises: Orbit's marker, or `rc<id>`
    /// for one of Milo's.
    pub finding_marker: Option<String>,
    /// For an earlier text of a reviewer's comment edited in place for a
    /// new head, which text it is, oldest first.
    pub edit: Option<usize>,
}
/// Everything Orbit works out about one pull request without a model.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub merged_at: Option<String>,
    pub head_ref: String,
    pub base_ref: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub fetched_at: String,
    pub narrative: Narrative,
    pub rounds: Vec<Round>,
    pub claims: Vec<Claim>,
    pub trajectories: Vec<Trajectory>,
    pub metrics: Metrics,
    pub flags: Vec<Flag>,
    /// What the writer made of it, when write-ups are on and it has run.
    pub writeup: Option<writeup::WriteUp>,
    /// The conversation and commits, for the write-up; not sent to the app.
    #[serde(skip)]
    pub comments: Vec<CommentRef>,
    #[serde(skip)]
    pub commits: Vec<CommitRef>,
    #[serde(skip)]
    pub files: Vec<String>,
}
/// What the Decisions page shows: every connected repository's stored pull
/// requests, and a note for each stored file that couldn't be read.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decisions {
    pub prs: Vec<History>,
    pub problems: Vec<String>,
}

/// A string field of a GitHub item, or "".
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}
/// A non-blank string, or None.
fn some(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}
/// Minutes from `a` to `b`, when both are times.
fn minutes_between(a: &str, b: &str) -> Option<i64> {
    let a = chrono::DateTime::parse_from_rfc3339(a).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(b).ok()?;
    let seconds = (b - a).num_seconds() as f64;
    Some((seconds / 60.0).round() as i64)
}
/// `text` cut to at most `max` characters.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => text[..i].to_string(),
        None => text.to_string(),
    }
}
