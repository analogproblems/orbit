//! The automatic review loop on a PR: Orbit's verdict for each round, the
//! loop's state worked out from the PR's automatic reviews, and when another
//! automatic round may start.
use crate::types::*;

/// Blocking findings left open by a review: its new findings at or above the
/// blocking priority, and earlier ones it found still open or disputed.
/// None for a run without structured findings.
pub fn blocking(r: &Review, blocking_priority: u8) -> Option<u32> {
    let findings = r.findings.as_ref()?;
    let new = findings
        .iter()
        .filter(|f| f.priority <= blocking_priority)
        .count();
    let earlier = r
        .previous_round
        .iter()
        .flat_map(|p| &p.findings)
        .filter(|f| {
            f.priority <= blocking_priority
                && r.prior_findings.iter().any(|p| {
                    p.id == f.id
                        && matches!(
                            p.status,
                            PriorStatus::StillOpen | PriorStatus::DeclinedDisputed
                        )
                })
        })
        .count();
    Some((new + earlier) as u32)
}
/// Records Orbit's verdict on a review whose findings were just extracted.
pub fn decide(r: &mut Review, blocking_priority: u8) {
    r.blocking = blocking(r, blocking_priority);
    r.orbit_verdict = r.blocking.map(|n| {
        if n == 0 {
            Verdict::Clean
        } else {
            Verdict::ChangesRequested
        }
    });
}

fn control<'a>(s: &'a Snapshot, pr_id: &str) -> Option<&'a LoopControl> {
    s.loops.iter().find(|c| c.pr_id == pr_id)
}
/// A PR's automatic reviews that count as rounds, newest first. Runs replaced
/// by a newer head and stopped runs are not rounds.
fn rounds_of<'a>(s: &'a Snapshot, pr_id: &str) -> Vec<&'a Review> {
    let mut rounds: Vec<&Review> = s
        .reviews
        .iter()
        .filter(|r| r.pr_id == pr_id && r.automatic && !r.superseded && r.status != "cancelled")
        .collect();
    rounds.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    rounds
}
/// Automatic rounds allowed for a PR: its repository's limit plus any rounds
/// granted by Continue loop.
pub fn round_limit(s: &Snapshot, pr: &PullRequest) -> u32 {
    let base = s
        .settings
        .repositories
        .iter()
        .find(|r| same_repo(&r.name, &pr.repo))
        .map_or(0, |r| r.round_limit);
    base + control(s, &pr.id).map_or(0, |c| c.extra_rounds)
}
/// Whether the head has stayed the same for a full poll interval.
fn settled(pr: &PullRequest, s: &Snapshot) -> bool {
    if !s.settings.settle_pushes {
        return true;
    }
    let Ok(seen) = chrono::DateTime::parse_from_rfc3339(&pr.head_seen_at) else {
        return true;
    };
    let age = chrono::Utc::now().signed_duration_since(seen);
    age.num_seconds() >= s.settings.poll_seconds as i64
}
/// Whether the loop may start another automatic round for this PR now.
pub fn allows_new_round(pr: &PullRequest, s: &Snapshot) -> bool {
    !control(s, &pr.id).is_some_and(|c| c.stopped)
        && (rounds_of(s, &pr.id).len() as u32) < round_limit(s, pr)
        && settled(pr, s)
}

/// The same blocking findings stayed open for two rounds in a row: every
/// blocking finding the latest round was given is still open and it found
/// no new ones. Without earlier-finding statuses, the two rounds' blocking
/// findings are compared by path and title.
fn stalled(latest: &Review, before: Option<&Review>, blocking_priority: u8) -> bool {
    let blocking_new = |r: &Review| -> Vec<(Option<String>, String)> {
        let mut keys: Vec<_> = r
            .findings
            .iter()
            .flatten()
            .filter(|f| f.priority <= blocking_priority)
            .map(|f| (f.path.clone(), f.title.to_lowercase()))
            .collect();
        keys.sort();
        keys
    };
    match &latest.previous_round {
        Some(round) if !latest.prior_findings.is_empty() => {
            let accepted = |id: &str| {
                round
                    .decisions
                    .iter()
                    .any(|d| d.id == id && d.status == PriorStatus::DeclinedAccepted)
            };
            let earlier: Vec<&Finding> = round
                .findings
                .iter()
                .filter(|f| f.priority <= blocking_priority && !accepted(&f.id))
                .collect();
            !earlier.is_empty()
                && blocking_new(latest).is_empty()
                && earlier.iter().all(|f| {
                    latest
                        .prior_findings
                        .iter()
                        .any(|p| p.id == f.id && p.status == PriorStatus::StillOpen)
                })
        }
        _ => before.is_some_and(|before| {
            let keys = blocking_new(latest);
            before.findings.is_some() && !keys.is_empty() && keys == blocking_new(before)
        }),
    }
}

/// The loop's state for a PR, worked out from its automatic reviews. None when
/// Orbit has not reviewed the PR automatically and the loop is not stopped.
pub fn state(s: &Snapshot, pr: &PullRequest) -> Option<LoopState> {
    let rounds = rounds_of(s, &pr.id);
    let limit = round_limit(s, pr);
    let make = |status, reason: Option<&str>| {
        Some(LoopState {
            pr_id: pr.id.clone(),
            status,
            reason: reason.map(str::to_owned),
            rounds: rounds.len() as u32,
            round_limit: limit,
        })
    };
    if control(s, &pr.id).is_some_and(|c| c.stopped) {
        return make(LoopStatus::Stopped, None);
    }
    let running: Vec<&Review> = s
        .reviews
        .iter()
        .filter(|r| r.pr_id == pr.id && r.automatic && active(&r.status))
        .collect();
    if !running.is_empty() {
        if running
            .iter()
            .any(|r| s.approvals.iter().any(|a| a.review_id == r.id))
        {
            return make(LoopStatus::NeedsYou, Some("An approval is pending."));
        }
        return make(LoopStatus::Reviewing, None);
    }
    let latest = *rounds.first()?;
    let at_limit = rounds.len() as u32 >= limit;
    let new_head = latest.head_sha != pr.head_sha || latest.base_sha != pr.base_sha;
    // A newer head is next in line unless the loop is out of rounds.
    let next_round = || {
        if at_limit {
            make(LoopStatus::NeedsYou, Some("Round limit reached."))
        } else {
            make(LoopStatus::Reviewing, None)
        }
    };
    if latest.status == "failed" {
        return make(LoopStatus::NeedsYou, Some("The review failed."));
    }
    let Some(verdict) = latest.orbit_verdict else {
        return make(
            LoopStatus::NeedsYou,
            Some("The review has no structured findings."),
        );
    };
    if latest
        .prior_findings
        .iter()
        .any(|p| p.status == PriorStatus::DeclinedDisputed)
    {
        return make(
            LoopStatus::NeedsYou,
            Some("The reviewer disputes a declined finding."),
        );
    }
    if verdict == Verdict::Clean {
        return if new_head {
            next_round()
        } else {
            make(LoopStatus::Clean, None)
        };
    }
    // Changes requested: the author has to see the report.
    if latest.status == "ready" && !new_head {
        if latest.error.is_some() {
            return make(LoopStatus::NeedsYou, Some("Publication was held."));
        }
        if !s.settings.auto_post {
            return make(
                LoopStatus::NeedsYou,
                Some("The report is waiting for you to publish it."),
            );
        }
        // Automatic publication is on its way.
        return make(LoopStatus::Reviewing, None);
    }
    if new_head {
        return next_round();
    }
    if stalled(latest, rounds.get(1).copied(), s.settings.blocking_priority) {
        return make(LoopStatus::NeedsYou, Some("The loop is not converging."));
    }
    if at_limit {
        return make(LoopStatus::NeedsYou, Some("Round limit reached."));
    }
    make(LoopStatus::WaitingForAuthor, None)
}
/// Loop states for every PR in the inbox that has one.
pub fn states(s: &Snapshot) -> Vec<LoopState> {
    s.prs.iter().filter_map(|pr| state(s, pr)).collect()
}

/// A review's round number within its PR's loop. None for a run that is not
/// a round (manual, superseded or stopped).
pub fn round_number(s: &Snapshot, r: &Review) -> Option<u32> {
    let rounds = rounds_of(s, &r.pr_id);
    let at = rounds.iter().position(|x| x.id == r.id)?;
    Some((rounds.len() - at) as u32)
}

/// GitHub allows commit status descriptions of up to 140 characters.
fn short(text: String) -> String {
    if text.chars().count() <= 140 {
        return text;
    }
    let mut cut: String = text.chars().take(139).collect();
    cut.push('…');
    cut
}
/// The status for a finished round: its verdict, or why it has none.
fn verdict_status(r: &Review, round: Option<u32>) -> (&'static str, String) {
    let round = round.map_or_else(|| "Review".to_string(), |n| format!("Round {n}"));
    if r.status == "failed" {
        return ("error", format!("{round} · the review failed"));
    }
    match r.orbit_verdict {
        Some(Verdict::Clean) => ("success", format!("{round} · no blocking findings")),
        Some(Verdict::ChangesRequested) => {
            let n = r.blocking.unwrap_or(0);
            let noun = if n == 1 { "finding" } else { "findings" };
            ("failure", format!("{round} · {n} blocking {noun}"))
        }
        None => ("error", format!("{round} · no structured findings")),
    }
}
fn status(state: &str, description: String) -> CommitStatus {
    CommitStatus {
        state: state.into(),
        description: short(description),
        error: None,
    }
}
/// The `orbit/review` status the PR's latest round should show on the PR's
/// current head. None when statuses are off or no round is on this head yet.
pub fn wanted_status(s: &Snapshot, pr: &PullRequest) -> Option<CommitStatus> {
    if !(s.settings.commit_status && s.settings.auto_post) {
        return None;
    }
    let rounds = rounds_of(s, &pr.id);
    let latest = *rounds.first()?;
    if latest.head_sha != pr.head_sha || latest.base_sha != pr.base_sha {
        return None;
    }
    let loop_state = state(s, pr)?;
    let n = rounds.len() as u32;
    Some(match loop_state.status {
        LoopStatus::Reviewing => status("pending", format!("Round {n} · reviewing")),
        LoopStatus::Clean | LoopStatus::WaitingForAuthor | LoopStatus::Stopped => {
            let (state, description) = verdict_status(latest, Some(n));
            status(state, description)
        }
        LoopStatus::NeedsYou => status(
            "error",
            format!(
                "Round {n} · needs you: {}",
                loop_state
                    .reason
                    .unwrap_or_default()
                    .trim_end_matches('.')
                    .to_lowercase()
            ),
        ),
    })
}
/// The final status for a commit whose check was left pending and that no
/// current round owns, from the newest automatic run on it. None while
/// that run is still going and has not been replaced.
fn closing_status(s: &Snapshot, check: &CommitCheck) -> Option<CommitStatus> {
    let r = s
        .reviews
        .iter()
        .filter(|r| r.pr_id == check.pr_id && r.head_sha == check.sha && r.automatic)
        .max_by(|a, b| a.created_at.cmp(&b.created_at));
    let Some(r) = r else {
        return Some(status("error", "Review stopped".into()));
    };
    if r.superseded && (active(&r.status) || r.status == "cancelled") {
        return Some(status("error", "Replaced by newer commits".into()));
    }
    if r.status == "cancelled" {
        return Some(status("error", "Review stopped".into()));
    }
    if active(&r.status) {
        return None;
    }
    let (state, description) = verdict_status(r, round_number(s, r));
    Some(status(state, description))
}
/// Every commit status Orbit should set now: each PR's current head, and
/// commits whose check Orbit left pending with no round to finish it (stopped,
/// replaced, or finished just before a push), which are closed even if
/// statuses were turned off since. One status per commit, so the newest round wins.
pub fn wanted_commit_checks(s: &Snapshot) -> Vec<CommitCheck> {
    let mut wanted: Vec<CommitCheck> = s
        .prs
        .iter()
        .filter_map(|pr| {
            Some(CommitCheck {
                pr_id: pr.id.clone(),
                repo: pr.repo.clone(),
                sha: pr.head_sha.clone(),
                status: wanted_status(s, pr)?,
            })
        })
        .collect();
    for check in &s.commit_checks {
        let open = check.status.state == "pending" && check.status.error.is_none();
        if !open
            || wanted
                .iter()
                .any(|w| w.pr_id == check.pr_id && w.sha == check.sha)
        {
            continue;
        }
        if let Some(status) = closing_status(s, check) {
            wanted.push(CommitCheck {
                status,
                ..check.clone()
            });
        }
    }
    wanted
}

#[cfg(test)]
#[path = "review_loop_tests.rs"]
mod tests;
