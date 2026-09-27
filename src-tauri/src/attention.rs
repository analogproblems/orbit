//! Everything that is waiting on the developer, gathered in one list: the
//! app counts it in the sidebar and lists it, and each item has a stable key
//! so it is announced once while it stays open.
use crate::types::*;

/// Consecutive failed syncs before a repository needs the developer.
pub const SYNC_FAILURES: u32 = 3;

/// Loop reasons that an item of another kind already covers.
const COVERED: [&str; 3] = [
    "An approval is pending.",
    "The review failed.",
    "Publication was held.",
];

fn item(
    key: String,
    kind: AttentionKind,
    pr_id: Option<&str>,
    review_id: Option<&str>,
    reason: String,
) -> Attention {
    Attention {
        key,
        kind,
        pr_id: pr_id.map(str::to_owned),
        review_id: review_id.map(str::to_owned),
        approval_id: None,
        repo: None,
        reason,
    }
}
fn approval_reason(method: &str) -> &'static str {
    if method.contains("commandExecution") {
        "The reviewer asks to run a command."
    } else if method.contains("fileChange") {
        "The reviewer asks to change files."
    } else {
        "The reviewer is waiting for your approval."
    }
}
/// The newest automatic run on a PR that was not replaced by a newer head.
fn latest_automatic<'a>(s: &'a Snapshot, pr: &PullRequest) -> Option<&'a Review> {
    s.reviews
        .iter()
        .filter(|r| r.pr_id == pr.id && r.automatic && !r.superseded)
        .max_by(|a, b| a.created_at.cmp(&b.created_at))
}
/// Why the PR's latest automatic run needs the developer, if it does.
fn run_item(s: &Snapshot, pr: &PullRequest) -> Option<Attention> {
    let r = latest_automatic(s, pr)?;
    let (kind, key, reason) = match r.status.as_str() {
        "failed" => (
            AttentionKind::Failed,
            "failed",
            r.error
                .clone()
                .unwrap_or_else(|| "The automatic review failed.".into()),
        ),
        // A stop by the developer leaves no error; a restart mid-run does.
        "cancelled" => (AttentionKind::Failed, "interrupted", r.error.clone()?),
        "ready" if r.head_sha == pr.head_sha && r.base_sha == pr.base_sha => {
            (AttentionKind::Held, "held", r.error.clone()?)
        }
        _ => return None,
    };
    Some(item(
        format!("{key}:{}", r.id),
        kind,
        Some(&pr.id),
        Some(&r.id),
        reason,
    ))
}

/// Counts each repository's failed syncs in a row. A sync that succeeds, or
/// a repository that was removed, clears its count.
pub fn count_sync_failures(s: &mut Snapshot, failed: &[(String, String)]) {
    let before = std::mem::take(&mut s.sync_failures);
    s.sync_failures = failed
        .iter()
        .map(|(repo, error)| SyncFailure {
            repo: repo.clone(),
            count: before
                .iter()
                .find(|f| same_repo(&f.repo, repo))
                .map_or(0, |f| f.count)
                + 1,
            error: error.clone(),
        })
        .collect();
}
/// Every open item, oldest kind of problem first: the workspace, then each
/// PR in inbox order.
pub fn items(s: &Snapshot) -> Vec<Attention> {
    let mut out = vec![];
    if let Some(error) = &s.state_write_error {
        if !s.settings.automatic {
            out.push(item(
                "autopilot-paused".into(),
                AttentionKind::AutopilotPaused,
                None,
                None,
                format!("Autopilot was turned off because Orbit could not save its state: {error}"),
            ));
        }
    }
    if s.poll_failures >= SYNC_FAILURES {
        out.push(item(
            "sync:github".into(),
            AttentionKind::SyncFailing,
            None,
            None,
            format!(
                "Orbit could not sync with GitHub {} times in a row: {}",
                s.poll_failures,
                s.github_error.as_deref().unwrap_or("unknown error")
            ),
        ));
    }
    for f in s.sync_failures.iter().filter(|f| f.count >= SYNC_FAILURES) {
        out.push(Attention {
            repo: Some(f.repo.clone()),
            ..item(
                format!("sync:{}", f.repo.to_ascii_lowercase()),
                AttentionKind::SyncFailing,
                None,
                None,
                format!(
                    "{} failed to sync {} times in a row: {}",
                    f.repo, f.count, f.error
                ),
            )
        });
    }
    for a in &s.approvals {
        let pr_id = s
            .reviews
            .iter()
            .find(|r| r.id == a.review_id)
            .map(|r| r.pr_id.as_str());
        out.push(Attention {
            approval_id: Some(a.id.clone()),
            ..item(
                format!("approval:{}:{}", a.review_id, a.id),
                AttentionKind::Approval,
                pr_id,
                Some(&a.review_id),
                approval_reason(&a.method).into(),
            )
        });
    }
    for pr in &s.prs {
        let state = crate::review_loop::state(s, pr);
        // A stopped loop is in the developer's hands already.
        if state
            .as_ref()
            .is_some_and(|l| l.status == LoopStatus::Stopped)
        {
            continue;
        }
        out.extend(run_item(s, pr));
        let Some(reason) = state
            .filter(|l| l.status == LoopStatus::NeedsYou)
            .and_then(|l| l.reason)
            .filter(|r| !COVERED.contains(&r.as_str()))
        else {
            continue;
        };
        let latest = latest_automatic(s, pr).map(|r| r.id.as_str());
        out.push(item(
            format!("loop:{}:{reason}", pr.id),
            AttentionKind::Loop,
            Some(&pr.id),
            latest,
            reason,
        ));
    }
    out
}
/// The snapshot as the app shows it, with the derived lists filled in.
pub fn shown(s: &Snapshot) -> Snapshot {
    let mut shown = s.clone();
    shown.loop_states = crate::review_loop::states(s);
    shown.attention = items(s);
    shown
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
