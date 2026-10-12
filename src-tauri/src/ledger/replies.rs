//! The builder's reply lines about findings: Orbit's own grammar
//! (`Fixed F2 in abc1234`, `Declined F2: reason`, read by the same parser
//! the next review round uses), plus the looser phrasings agents used before
//! that grammar existed, such as "Both findings are fixed in 0ab9947."
use super::ClaimScope;
use crate::{engine::claims, types::ClaimKind};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, PartialEq)]
pub(super) struct ReplyLine {
    pub numbers: Vec<u32>,
    pub kind: ClaimKind,
    pub commit: Option<String>,
    pub reason: Option<String>,
    pub scope: ClaimScope,
}

/// Lines in Orbit's own grammar.
pub(super) fn exact_lines(body: &str) -> Vec<ReplyLine> {
    claims::lines(body)
        .into_iter()
        .map(|l| ReplyLine {
            scope: if l.numbers.is_empty() {
                ClaimScope::Thread
            } else {
                ClaimScope::Named
            },
            numbers: l.numbers,
            kind: l.kind,
            commit: l.commit,
            reason: l.reason,
        })
        .collect()
}

static STARTS_FIXED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^fixed\b").unwrap());
/// Orbit's `Declined` line, so a line it reads is not read twice.
static DECLINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^declined\b(?:\s*(?:,|&|\band\b)?\s*f\d+\b)*\s*(?::\s*.*?|[.!]?)\s*$").unwrap()
});
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)f(\d+)").unwrap());
static SHA_AFTER_IN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bin\s+(?:commit\s+)?([0-9a-f]{7,40})\b").unwrap());
static NOT_FIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:not|un)fixed\b|\bnot (?:yet )?fixed\b|remains? unfixed").unwrap()
});
static FIXED_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bfixed\b").unwrap());
static ALL_FIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:all|both|all \w+|the \w+|these|those)\s+(?:\w+\s+)?(?:findings?|issues?|of them)\s+(?:are|is|were|was)\s+(?:now\s+)?fixed\b|\b(?:all|both)\s+(?:are|were)\s+(?:now\s+)?fixed\b|^(?:thanks,?\s*)?(?:all|both)\s+(?:\w+\s+)?(?:are|were)\s+fixed\b",
    )
    .unwrap()
});
static ADDRESSED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^addressed\b.*\bin\s+([0-9a-f]{7,40})\b").unwrap());
static STARTS_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^f\d+").unwrap());
static DECLINE_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:declin\w*|won'?t fix|will not fix|not fixing|disagree\w*|push(?:ing|ed)? back|leaving (?:it|this|as is)|intentional|by design|out of scope|not a bug|not an issue)\b",
    )
    .unwrap()
});

/// Removes bold and code marks and a list or quote marker, as Orbit does
/// before matching.
fn clean_line(line: &str) -> String {
    let line: String = line.chars().filter(|c| !matches!(c, '*' | '`')).collect();
    line.trim()
        .trim_start_matches(|c: char| matches!(c, '-' | '+' | '>') || c.is_whitespace())
        .trim()
        .to_string()
}
fn numbers_in(text: &str) -> Vec<u32> {
    ID.captures_iter(text)
        .filter_map(|c| c[1].parse().ok())
        .collect()
}
fn sha_after_in(line: &str) -> Option<String> {
    SHA_AFTER_IN
        .captures(line)
        .map(|c| c[1].to_ascii_lowercase())
}

/// Older phrasings: "F1 (…) is fixed in abc1234", "All three findings are
/// fixed in abc1234", "**F1 (P2), fixed.**", "Addressed … in abc1234". A
/// line naming no commit takes the first commit the comment names.
pub(super) fn legacy_lines(body: &str) -> Vec<ReplyLine> {
    let lines: Vec<String> = body.lines().map(clean_line).collect();
    let default_commit = lines.iter().find_map(|l| sha_after_in(l));
    let mut out = vec![];
    for line in lines.iter().filter(|l| !l.is_empty()) {
        if STARTS_FIXED.is_match(line) || DECLINED.is_match(line) {
            continue;
        }
        let lower = line.to_lowercase();
        let has_fixed = FIXED_WORD.is_match(&lower) && !NOT_FIXED.is_match(&lower);
        let commit = sha_after_in(line);
        let before_fixed = lower.split("fixed").next().unwrap_or("");
        let numbers = numbers_in(before_fixed);
        if let Some(c) = ADDRESSED.captures(line) {
            out.push(ReplyLine {
                numbers: vec![],
                kind: ClaimKind::Fixed,
                commit: Some(c[1].to_ascii_lowercase()),
                reason: None,
                scope: ClaimScope::All,
            });
            continue;
        }
        if has_fixed && ALL_FIXED.is_match(line) {
            out.push(ReplyLine {
                numbers: vec![],
                kind: ClaimKind::Fixed,
                commit: commit.or_else(|| default_commit.clone()),
                reason: None,
                scope: ClaimScope::All,
            });
            continue;
        }
        if has_fixed && !numbers.is_empty() && STARTS_ID.is_match(line) {
            out.push(ReplyLine {
                numbers,
                kind: ClaimKind::Fixed,
                commit: commit.or_else(|| default_commit.clone()),
                reason: None,
                scope: ClaimScope::Named,
            });
            continue;
        }
        if DECLINE_WORDS.is_match(line) && STARTS_ID.is_match(line) {
            let ids = numbers_in(line);
            if !ids.is_empty() {
                out.push(ReplyLine {
                    numbers: ids,
                    kind: ClaimKind::Declined,
                    commit: None,
                    reason: Some(line.clone()),
                    scope: ClaimScope::Named,
                });
            }
        }
    }
    out
}

/// Every claim a comment makes: Orbit's grammar first, then older phrasings.
pub(super) fn reply_lines(body: &str) -> Vec<ReplyLine> {
    let mut lines = exact_lines(body);
    lines.extend(legacy_lines(body));
    lines
}
