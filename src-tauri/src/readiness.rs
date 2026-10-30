//! Whether a pull request is clear to merge, as the reviewer of record
//! sees it: Orbit's loop is clean on its current head and base, every check
//! on that head passed, they ran with its base's latest commit, and GitHub
//! says it can merge. Otherwise, what is in the way, first things first.
//! Only pull requests Orbit reviews automatically have an answer. Orbit
//! never merges: this is what it tells you and, later, the agent.
//!
//! GitHub works out whether a pull request merges in the background and
//! only says so when asked about that one pull request, so Orbit asks
//! (`read`) only once nothing else is in the way, a few a pass, and again
//! when its head or its base's latest commit moves.
use crate::{
    ci::{short, CiState},
    logs,
    problems::{stamp, time},
    types::*,
    workflows,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pull requests Orbit asks GitHub about in one pass.
pub const PER_PASS: usize = 4;
/// How often Orbit asks again about a pull request that hasn't moved, as a
/// rule or a review on GitHub can change the answer.
pub const EVERY: Duration = Duration::minutes(10);

/// What is in the way of merging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    /// Orbit hasn't finished reviewing the current head and base.
    Review,
    /// Blocking findings wait on the agent.
    Findings,
    /// Orbit's loop needs a person, or was stopped.
    Loop,
    /// A check failed, is still running, or hasn't been read.
    Checks,
    /// The checks ran without the base's latest commit, or Orbit can't
    /// tell.
    Base,
    /// GitHub says it can't merge yet, or hasn't said.
    Merge,
}
/// One thing in the way, in Orbit's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wait {
    pub kind: WaitKind,
    pub text: String,
}
/// Whether a pull request is clear to merge at its current head.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub pr_id: String,
    pub repo: String,
    pub number: u64,
    /// The head it was worked out for.
    pub head_sha: String,
    /// The base branch, and the latest commit on it the checks ran with,
    /// once Orbit knows it.
    pub base: String,
    pub base_sha: String,
    /// Nothing is in the way.
    pub clear: bool,
    /// What is in the way, first things first.
    pub waits: Vec<Wait>,
}
impl Readiness {
    /// The answer in one line: "Clear to merge at abc1234 against main at
    /// def5678." or "Not clear to merge yet. Checks are still running on
    /// abc1234."
    pub fn line(&self) -> String {
        match self.waits.first() {
            None => format!(
                "Clear to merge at {} against {} at {}.",
                short(&self.head_sha),
                self.base,
                short(&self.base_sha)
            ),
            Some(w) => format!("Not clear to merge yet. {}", w.text),
        }
    }
}

/// What GitHub last said about whether a pull request can merge.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Merge {
    pub pr_id: String,
    /// The head Orbit asked about, and its base branch's latest commit
    /// then, which can make a conflict without the pull request changing.
    pub head_sha: String,
    pub base_sha: String,
    /// When Orbit last asked, by Orbit's clock.
    pub read_at: String,
    /// GitHub's `mergeable`: None while it is still working it out.
    pub mergeable: Option<bool>,
    /// GitHub's `mergeable_state`, such as clean, dirty, blocked or behind.
    pub state: String,
    /// Why the last read failed, how many reads in a row failed or found
    /// GitHub still working it out, and when Orbit asks again.
    pub error: Option<String>,
    pub tries: u32,
    pub retry_at: Option<String>,
}
impl Merge {
    /// GitHub hasn't worked it out yet.
    fn pending(&self) -> bool {
        self.mergeable.is_none() || self.state == "unknown"
    }
}

/// The pull requests Orbit is the reviewer of record for: open, not
/// drafts, from a source Orbit trusts, in a repository it reviews
/// automatically.
fn reviewed(s: &Snapshot) -> impl Iterator<Item = &PullRequest> {
    s.prs.iter().filter(|pr| {
        !pr.draft
            && trusted_source(pr, &s.settings.trusted_authors)
            && s.settings
                .repositories
                .iter()
                .any(|r| same_repo(&r.name, &pr.repo) && r.automatic)
    })
}
/// Every reviewed pull request's answer.
pub fn all(s: &Snapshot) -> Vec<Readiness> {
    reviewed(s).map(|pr| of(s, pr)).collect()
}
/// A pull request's answer.
pub fn of(s: &Snapshot, pr: &PullRequest) -> Readiness {
    let mut answer = Readiness {
        pr_id: pr.id.clone(),
        repo: pr.repo.clone(),
        number: pr.number,
        head_sha: pr.head_sha.clone(),
        base: pr.base_ref.clone(),
        base_sha: String::new(),
        clear: false,
        waits: vec![],
    };
    let mut wait = |kind, text: String| answer.waits.push(Wait { kind, text });
    if let Some((kind, text)) = review(s, pr) {
        wait(kind, text);
    }
    let checked = checks(s, pr);
    for (kind, text) in &checked.waits {
        wait(*kind, text.clone());
    }
    let base_sha = checked.base_sha.clone();
    if answer.waits.is_empty() {
        let merge = s.merges.iter().find(|m| asked(m, pr, &base_sha));
        if let Some(text) = merging(merge, pr) {
            answer.waits.push(Wait {
                kind: WaitKind::Merge,
                text,
            });
        }
    }
    answer.base_sha = base_sha;
    answer.clear = answer.waits.is_empty();
    answer
}

/// What Orbit's loop says, when it is in the way.
fn review(s: &Snapshot, pr: &PullRequest) -> Option<(WaitKind, String)> {
    let head = short(&pr.head_sha);
    let Some(state) = crate::review_loop::state(s, pr) else {
        return Some((
            WaitKind::Review,
            format!("Orbit hasn't reviewed {head} yet."),
        ));
    };
    let latest = state
        .round_ids
        .last()
        .and_then(|id| s.reviews.iter().find(|r| &r.id == id));
    let current = latest.is_some_and(|r| r.head_sha == pr.head_sha && r.base_sha == pr.base_sha);
    match state.status {
        LoopStatus::Clean => None,
        LoopStatus::Reviewing if current && latest.is_some_and(|r| active(&r.status)) => {
            Some((WaitKind::Review, format!("Orbit is reviewing {head}.")))
        }
        LoopStatus::Reviewing if current => Some((
            WaitKind::Review,
            format!("Orbit is publishing its review of {head}."),
        )),
        LoopStatus::Reviewing => Some((
            WaitKind::Review,
            format!("Orbit hasn't reviewed {head} yet."),
        )),
        LoopStatus::WaitingForAuthor => {
            let n = latest.and_then(|r| r.blocking).unwrap_or(0);
            let findings = match n {
                1 => "1 blocking finding".to_string(),
                n => format!("{n} blocking findings"),
            };
            let verb = if n == 1 { "waits" } else { "wait" };
            Some((
                WaitKind::Findings,
                format!("{findings} from Orbit's review of {head} {verb} on the author."),
            ))
        }
        LoopStatus::NeedsYou => Some((
            WaitKind::Loop,
            match &state.reason {
                Some(reason) => format!("Orbit's review loop needs you: {reason}"),
                None => "Orbit's review loop needs you.".into(),
            },
        )),
        LoopStatus::Stopped => Some((
            WaitKind::Loop,
            "Orbit's review loop on it is stopped.".into(),
        )),
    }
}

/// What the checks say, and the base branch's latest commit they ran
/// with, when Orbit knows it.
struct Checked {
    waits: Vec<(WaitKind, String)>,
    base_sha: String,
}
fn checks(s: &Snapshot, pr: &PullRequest) -> Checked {
    let head = short(&pr.head_sha);
    let waiting = |kind, text: String| Checked {
        waits: vec![(kind, text)],
        base_sha: String::new(),
    };
    if !s.settings.watch_ci {
        return waiting(
            WaitKind::Checks,
            "Orbit can't see its checks while Watch CI is off.".into(),
        );
    }
    let repo = s.ci.iter().find(|c| same_repo(&c.repo, &pr.repo));
    let pull = repo
        .and_then(|c| c.pull(pr.number))
        .filter(|p| p.ci.sha == pr.head_sha);
    let (Some(repo), Some(pull)) = (repo, pull) else {
        return waiting(
            WaitKind::Checks,
            format!("Orbit hasn't read the checks on {head} yet."),
        );
    };
    let ci = &pull.ci;
    let base = &pr.base_ref;
    let on_default = *base == repo.branch;
    match ci.state {
        CiState::Failing => {
            let verb = if ci.failing.len() == 1 {
                "fails"
            } else {
                "fail"
            };
            return waiting(
                WaitKind::Checks,
                format!("{} {verb} on {head}.", crate::problems::names(&ci.failing)),
            );
        }
        CiState::Running if ci.incomplete && ci.running == 0 => {
            return waiting(
                WaitKind::Checks,
                format!("Orbit couldn't read every check on {head}."),
            );
        }
        CiState::Running => {
            return waiting(
                WaitKind::Checks,
                format!("Checks are still running on {head}."),
            );
        }
        // Without CI on pull requests there are no checks to wait for, nor
        // a run to have missed the base.
        CiState::None if no_ci(s, &pr.repo) => {
            return Checked {
                waits: vec![],
                base_sha: if on_default {
                    repo.head.sha.clone()
                } else {
                    pr.base_sha.clone()
                },
            };
        }
        CiState::None => {
            return waiting(
                WaitKind::Checks,
                format!("No checks have reported on {head} yet."),
            );
        }
        CiState::Passing => {}
    }
    if !on_default {
        return waiting(
            WaitKind::Base,
            format!(
                "Orbit can't tell whether its checks ran with the latest {base}: it only follows the default branch."
            ),
        );
    }
    match crate::problems::behind(repo, pr, ci) {
        Some(false) => Checked {
            waits: vec![],
            base_sha: repo.head.sha.clone(),
        },
        Some(true) => waiting(
            WaitKind::Base,
            format!("{base} moved on since its checks ran: merge {base} in and let them run again."),
        ),
        None => waiting(
            WaitKind::Base,
            format!(
                "Orbit can't tell whether its checks ran with the latest {base}, as GitHub didn't say when they began."
            ),
        ),
    }
}
/// Whether Orbit read that no CI runs on the repository's pull requests,
/// and has never seen checks run there.
fn no_ci(s: &Snapshot, repo: &str) -> bool {
    let setup = s.setups.iter().find(|w| same_repo(&w.repo, repo));
    setup.is_some_and(|w| {
        w.ran.is_none()
            && matches!(
                w.verdict(),
                Some(workflows::Verdict::NoWorkflows | workflows::Verdict::NotOnPulls)
            )
    })
}

/// Whether Orbit's question to GitHub was about this head and base.
fn asked(m: &Merge, pr: &PullRequest, base_sha: &str) -> bool {
    m.pr_id == pr.id && m.head_sha == pr.head_sha && m.base_sha == base_sha
}
/// What GitHub's answer says, when it is in the way.
fn merging(merge: Option<&Merge>, pr: &PullRequest) -> Option<String> {
    let base = &pr.base_ref;
    let Some(m) = merge else {
        return Some("Orbit hasn't asked GitHub yet whether it merges.".into());
    };
    if m.read_at.is_empty() {
        if let Some(error) = &m.error {
            return Some(error.clone());
        }
    }
    if m.pending() {
        return Some("GitHub is still checking whether it merges.".into());
    }
    if m.mergeable == Some(false) || m.state == "dirty" {
        return Some(format!(
            "It conflicts with {base}: merge {base} in and fix the conflicts."
        ));
    }
    match m.state.as_str() {
        "clean" | "has_hooks" => None,
        "behind" => Some(format!(
            "GitHub wants it brought up to date with {base} first."
        )),
        "blocked" => {
            Some("GitHub says a rule blocks merging it, such as a required review.".into())
        }
        "unstable" => Some("GitHub says a check on it isn't passing.".into()),
        "draft" => Some("GitHub says it is a draft.".into()),
        other => Some(format!("GitHub says it can't merge yet ({other}).")),
    }
}

/// The pull requests Orbit should ask GitHub about now, at most
/// `PER_PASS`, with the base branch's latest commit to ask at: those
/// with nothing else in the way whose answer is for another head or base,
/// or `EVERY` old, or, while GitHub was still working it out or the read
/// failed, whose wait is over. Never asked first, then the oldest.
pub fn wanted(s: &Snapshot, now: DateTime<Utc>) -> Vec<(String, u64, String, String)> {
    let mut due: Vec<(String, (String, u64, String, String))> = reviewed(s)
        .filter_map(|pr| {
            let answer = of(s, pr);
            if !answer.waits.iter().all(|w| w.kind == WaitKind::Merge) {
                return None;
            }
            let ask = (
                pr.repo.clone(),
                pr.number,
                pr.head_sha.clone(),
                answer.base_sha.clone(),
            );
            let Some(m) = s.merges.iter().find(|m| asked(m, pr, &answer.base_sha)) else {
                return Some((String::new(), ask));
            };
            let ready = if m.error.is_some() || m.pending() {
                m.retry_at
                    .as_deref()
                    .and_then(time)
                    .is_none_or(|retry| now >= retry)
            } else {
                time(&m.read_at).is_none_or(|at| now - at >= EVERY)
            };
            ready.then(|| (m.read_at.clone(), ask))
        })
        .collect();
    due.sort_by(|a, b| a.0.cmp(&b.0));
    due.into_iter().take(PER_PASS).map(|(_, ask)| ask).collect()
}

/// Asks GitHub whether a pull request can merge. An answer about another
/// head is an error, as the pull request moved since Orbit last synced.
pub async fn read(
    repo: &str,
    number: u64,
    head_sha: &str,
    base_sha: &str,
) -> Result<Merge, String> {
    validate_repo(repo)?;
    let v = crate::github::api(&format!("repos/{repo}/pulls/{number}"), "GET", &[], None).await?;
    understand(&v, &pr_id(repo, number), head_sha, base_sha, Utc::now())
}
/// GitHub's answer about one pull request, as a `Merge`.
pub fn understand(
    v: &Value,
    pr_id: &str,
    head_sha: &str,
    base_sha: &str,
    now: DateTime<Utc>,
) -> Result<Merge, String> {
    if v["head"]["sha"] != head_sha {
        return Err("GitHub's answer was about another commit.".into());
    }
    Ok(Merge {
        pr_id: pr_id.to_string(),
        head_sha: head_sha.to_string(),
        base_sha: base_sha.to_string(),
        read_at: stamp(now),
        mergeable: v["mergeable"].as_bool(),
        state: v["mergeable_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        ..Default::default()
    })
}

/// Records what GitHub said about a pull request, or that asking failed.
/// While GitHub is still working it out, or after an error, Orbit asks
/// again after a wait, a minute doubling up to `logs::MAX_RETRY`; an error
/// keeps what GitHub said before about the same head and base.
pub fn record(
    merges: &mut Vec<Merge>,
    pr_id: &str,
    head_sha: &str,
    base_sha: &str,
    result: Result<Merge, String>,
    now: DateTime<Utc>,
) {
    let known = merges.iter().position(|m| m.pr_id == pr_id);
    let same = known
        .map(|i| &merges[i])
        .filter(|m| m.head_sha == head_sha && m.base_sha == base_sha);
    let tries = same.map_or(0, |m| m.tries) + 1;
    let wait = Duration::minutes(1i64 << (tries - 1).min(6)).min(logs::MAX_RETRY);
    let merge = match result {
        Ok(m) if m.pending() => Merge {
            tries,
            retry_at: Some(stamp(now + wait)),
            ..m
        },
        Ok(m) => m,
        Err(error) => {
            let before = same.cloned().unwrap_or_else(|| Merge {
                pr_id: pr_id.to_string(),
                head_sha: head_sha.to_string(),
                base_sha: base_sha.to_string(),
                ..Default::default()
            });
            Merge {
                error: Some(format!(
                    "Orbit couldn't ask GitHub whether it merges: {}. It tries again later.",
                    logs::short(&error)
                )),
                tries,
                retry_at: Some(stamp(now + wait)),
                ..before
            }
        }
    };
    match known {
        Some(i) => merges[i] = merge,
        None => merges.push(merge),
    }
}

/// Forgets what GitHub said about pull requests Orbit no longer reviews.
pub fn prune(s: &mut Snapshot) {
    let kept: Vec<String> = reviewed(s).map(|pr| pr.id.clone()).collect();
    s.merges.retain(|m| kept.contains(&m.pr_id));
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
