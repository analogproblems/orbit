//! Replies Orbit recognises in the discussion since the previous round:
//! `Fixed F2 in abc1234` and `Declined F2: reason` in any comment from a
//! trusted account, and `Fixed in abc1234` or `Declined: reason` in a
//! finding's own thread. The reviewer still judges every earlier finding;
//! a claim tells it what the author says, and where a named commit is.
//!
//! Replies to Orbit's CI comments and issues name items instead (see
//! `ci_lines`): `Taking I2`, `Fixed I2 in abc1234`, `Declined I2: reason`
//! and `Need a person on I2: question`.
use super::{rounds::Comments, worktrees::git};
use crate::ops::{AnswerKind, Said};
use crate::types::*;
use regex::Regex;
use std::{path::Path, sync::LazyLock};

/// `Fixed`, then short IDs, then `in <commit>`, then anything.
static FIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^fixed\b(?P<ids>(?:\s*(?:,|&|\band\b)?\s*f\d+\b)*)(?:\s+in\s+(?:commit\s+)?(?P<sha>[0-9a-f]{7,40})\b)?(?P<rest>.*)$",
    )
    .unwrap()
});
/// `Declined`, then short IDs, then `: reason` or nothing more.
static DECLINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^declined\b(?P<ids>(?:\s*(?:,|&|\band\b)?\s*f\d+\b)*)\s*(?::\s*(?P<reason>.*?)|[.!]?)\s*$",
    )
    .unwrap()
});
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)f(\d+)").unwrap());

/// CI items, as in `I2`, `I1, I3` or `I1 and I2`; at least one.
const ITEMS: &str = r"(?P<ids>(?:\s*(?:,|&|\band\b)?\s*i\d+\b)+)";
/// `Taking`, then items, then anything.
static TAKING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)^taking\b{ITEMS}")).unwrap());
/// `Fixed`, then items, then `in <commit>` or nothing, then anything.
static CI_FIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^fixed\b{ITEMS}(?:\s+in\s+(?:commit\s+)?(?P<sha>[0-9a-f]{{7,40}})\b)?"
    ))
    .unwrap()
});
/// `Declined`, then items, then `: reason` or nothing more.
static CI_DECLINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^declined\b{ITEMS}\s*(?::\s*(?P<text>.*?)|[.!]?)\s*$"
    ))
    .unwrap()
});
/// `Need a person on`, then items, then `: question` or nothing more.
static NEED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^need\s+a\s+person\s+(?:on|for)\b{ITEMS}\s*(?::\s*(?P<text>.*?)|[.!?]?)\s*$"
    ))
    .unwrap()
});
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)i(\d+)").unwrap());

/// A line without bold, code marks, or list and quote markers.
fn plain(line: &str) -> String {
    let line: String = line.chars().filter(|c| !matches!(c, '*' | '`')).collect();
    line.trim()
        .trim_start_matches(['-', '+', '>', ' '])
        .trim()
        .to_string()
}

/// What each line of a reply to Orbit's CI comment or issue says about its
/// items. Lines that name no item, such as `Fixed F2 in abc1234`, are
/// left to the review's own replies.
pub(crate) fn ci_lines(body: &str) -> Vec<Said> {
    body.lines()
        .filter_map(|line| {
            let line = plain(line);
            let numbers = |ids: &str| -> Vec<u32> {
                ITEM.captures_iter(ids)
                    .filter_map(|c| c[1].parse().ok())
                    .collect()
            };
            let text = |c: &regex::Captures| {
                c.name("text")
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|t| !t.is_empty())
            };
            let said = |c: &regex::Captures, kind| Said {
                numbers: numbers(&c["ids"]),
                kind,
                commit: None,
                text: None,
            };
            if let Some(c) = CI_FIXED.captures(&line) {
                return Some(Said {
                    commit: c.name("sha").map(|m| m.as_str().to_ascii_lowercase()),
                    ..said(&c, AnswerKind::Fixed)
                });
            }
            if let Some(c) = TAKING.captures(&line) {
                return Some(said(&c, AnswerKind::Taking));
            }
            if let Some(c) = CI_DECLINED.captures(&line) {
                return Some(Said {
                    text: text(&c),
                    ..said(&c, AnswerKind::Declined)
                });
            }
            let c = NEED.captures(&line)?;
            Some(Said {
                text: text(&c),
                ..said(&c, AnswerKind::Question)
            })
        })
        .collect()
}

/// One claim a line of a comment makes: the short ID numbers it names (none
/// means the finding whose thread it is in), its kind, commit and reason.
#[derive(Debug, PartialEq)]
pub(crate) struct Line {
    pub numbers: Vec<u32>,
    pub kind: ClaimKind,
    pub commit: Option<String>,
    pub reason: Option<String>,
}

/// Reads each line of a comment. Bold, code marks and list or quote markers
/// around a claim are ignored.
pub(crate) fn lines(body: &str) -> Vec<Line> {
    body.lines()
        .filter_map(|line| {
            let line = plain(line);
            let line = line.as_str();
            let numbers = |ids: &str| -> Vec<u32> {
                ID.captures_iter(ids)
                    .filter_map(|c| c[1].parse().ok())
                    .collect()
            };
            if let Some(c) = FIXED.captures(line) {
                let numbers = numbers(&c["ids"]);
                let commit = c.name("sha").map(|m| m.as_str().to_ascii_lowercase());
                // "Fixed the tests" says nothing about which finding.
                let bare = matches!(c["rest"].trim(), "" | "." | "!");
                return (!numbers.is_empty() || commit.is_some() || bare).then_some(Line {
                    numbers,
                    kind: ClaimKind::Fixed,
                    commit,
                    reason: None,
                });
            }
            let c = DECLINED.captures(line)?;
            Some(Line {
                numbers: numbers(&c["ids"]),
                kind: ClaimKind::Declined,
                commit: None,
                reason: c
                    .name("reason")
                    .map(|m| m.as_str().trim().to_string())
                    .filter(|r| !r.is_empty()),
            })
        })
        .collect()
}

/// The latest claim about each finding of `round`, from the comments given
/// to the reviewer, oldest first. Claims naming a finding that is not in
/// play are left out; their comments still reach the reviewer.
pub(super) fn collect(comments: &Comments, round: &PreviousRound) -> Vec<AuthorClaim> {
    let mut out: Vec<AuthorClaim> = vec![];
    for comment in &comments.kept {
        for line in lines(&comment.body) {
            let ids: Vec<&str> = if line.numbers.is_empty() {
                comment.finding.as_deref().into_iter().collect()
            } else {
                line.numbers
                    .iter()
                    .filter_map(|n| {
                        round
                            .findings
                            .iter()
                            .find(|f| f.number == Some(*n))
                            .map(|f| f.id.as_str())
                    })
                    .collect()
            };
            for id in ids {
                if !round.findings.iter().any(|f| f.id == id) {
                    continue;
                }
                out.retain(|c| c.finding_id != id);
                out.push(AuthorClaim {
                    finding_id: id.to_string(),
                    author: comment.author.clone(),
                    created_at: comment.created_at.clone(),
                    kind: line.kind,
                    commit: line.commit.clone(),
                    commit_check: None,
                    reason: line.reason.clone(),
                });
            }
        }
    }
    out
}

/// Works out where each named commit is: one of the commits since the
/// previous round, already in the previous head, outside this head, or
/// missing. Commits are hex only, so they can't be read as Git options.
pub(super) async fn place_commits(
    claims: &mut [AuthorClaim],
    worktree: &Path,
    previous: &str,
    head: &str,
) {
    for claim in claims.iter_mut() {
        let Some(commit) = &claim.commit else {
            continue;
        };
        let Ok(full) = git(
            worktree,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{commit}^{{commit}}"),
            ],
        )
        .await
        else {
            claim.commit_check = Some(CommitPlace::NotFound);
            continue;
        };
        let full = full.trim();
        let ancestor = |of: &str| {
            let of = of.to_string();
            let full = full.to_string();
            async move {
                git(worktree, &["merge-base", "--is-ancestor", &full, &of])
                    .await
                    .is_ok()
            }
        };
        claim.commit_check = Some(if !ancestor(head).await {
            CommitPlace::NotInBranch
        } else if ancestor(previous).await {
            CommitPlace::Earlier
        } else {
            CommitPlace::InChange
        });
    }
}
