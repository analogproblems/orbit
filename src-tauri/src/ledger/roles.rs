//! Who wrote a comment, judged from its text, because the builder, the
//! reviewer and the person can all post from the same GitHub account.
use super::Role;
use regex::Regex;
use std::sync::LazyLock;

static BUILDER_FOOTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)generated (?:by|with) \[claude code\]").unwrap());
static REVIEWER_MARKS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"<!--\s*orbit-review:",
        r"<!--\s*orbit-finding:",
        r"<!--\s*milo-pr-review",
        r"(?m)^##\s*Milo PR review",
        r"(?m)^##\s*Orbit\s*·",
        r"(?im)^(?:\*\*)?(?:re-?)?reviewed\s+(?:`[0-9a-f]{7,40}`|commit\b|head\b|the (?:latest )?update\b)",
        r"(?m)^\s*(?:\d+\.\s*)?\*\*\[P[0-3]\]",
        r"(?im)^(?:\*\*)?clear to merge\b",
        r"(?i)does not authorize milo to merge|no verdict authorizes milo",
        r"(?im)^(?:\*\*)?no (?:new|outstanding|actionable|remaining) findings",
    ]
    .iter()
    .map(|r| Regex::new(r).unwrap())
    .collect()
});
static BUILDER_MARKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)co-authored-by:\s*claude|claude\.ai/code/session_").unwrap());

/// Whether GitHub says the author can write to the repository (or is an
/// app installed on it). Comments stored before Orbit kept the association
/// have none and keep being read by their text.
fn may_review(login: &str, association: &str) -> bool {
    association.is_empty()
        || login.ends_with("[bot]")
        || matches!(association, "OWNER" | "MEMBER" | "COLLABORATOR")
}

pub(super) fn infer(body: &str, login: &str, association: &str) -> Role {
    if BUILDER_FOOTER.is_match(body) {
        return Role::Builder;
    }
    // Anyone can paste the reviewer's markers into a comment on a public
    // repository, so only someone who can write to it counts as the
    // reviewer.
    if REVIEWER_MARKS.iter().any(|r| r.is_match(body)) {
        return if may_review(login, association) {
            Role::Reviewer
        } else {
            Role::Human
        };
    }
    if BUILDER_MARKS.is_match(body) || !super::replies::exact_lines(body).is_empty() {
        return Role::Builder;
    }
    if login.ends_with("[bot]") {
        return Role::Unknown;
    }
    Role::Human
}
