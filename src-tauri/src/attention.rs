//! Everything that is waiting on the developer, gathered in one list: the
//! app counts it in the sidebar and lists it, and each item has a stable key
//! so it is announced once while it stays open. Where one of Orbit's own
//! controls resolves an item, the item names it as its fix.
use crate::{buddy::Fix, types::*};

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
        fix: None,
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
/// Whether Bit may publish a report: one automatic publication wouldn't
/// hold, so current, free of credentials, and from a PR whose source Orbit
/// trusts.
pub fn may_publish(s: &Snapshot, pr: &PullRequest, r: &Review) -> bool {
    r.status == "ready"
        && r.before_follow_up.is_none()
        && publishable(r, pr)
        && r.sensitive.is_empty()
        && trusted_source(pr, &s.settings.trusted_authors)
}
fn publish_fix(s: &Snapshot, pr: &PullRequest, r: &Review) -> Option<Fix> {
    may_publish(s, pr, r).then_some(Fix::Publish)
}
/// Why the PR's latest automatic run needs the developer, if it does.
fn run_item(s: &Snapshot, pr: &PullRequest) -> Option<Attention> {
    let r = latest_automatic(s, pr)?;
    // A failed or stopped run is settled once a later review of the same
    // commits finished, such as the one Review again started.
    let redone = || {
        s.reviews.iter().any(|n| {
            n.pr_id == pr.id
                && n.created_at > r.created_at
                && n.head_sha == r.head_sha
                && n.base_sha == r.base_sha
                && matches!(n.status.as_str(), "ready" | "posted")
        })
    };
    if matches!(r.status.as_str(), "failed" | "cancelled") && redone() {
        return None;
    }
    let fix = match r.status.as_str() {
        // Review again starts a review, so not while one is running.
        "failed" | "cancelled" => (!s
            .reviews
            .iter()
            .any(|r| r.pr_id == pr.id && active(&r.status)))
        .then_some(Fix::ReviewAgain),
        _ => publish_fix(s, pr, r),
    };
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
    Some(Attention {
        fix,
        ..item(
            format!("{key}:{}", r.id),
            kind,
            Some(&pr.id),
            Some(&r.id),
            reason,
        )
    })
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
/// Text as a sentence, ending in a full stop.
fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}
/// When Orbit syncs again, such as "Orbit checks again every 2 minutes."
fn checks_again(s: &Snapshot) -> String {
    let n = s.settings.poll_seconds;
    let every = match n {
        60 => "minute".to_string(),
        n if n % 60 == 0 => format!("{} minutes", n / 60),
        n => format!("{n} seconds"),
    };
    format!("Orbit checks again every {every}.")
}
/// Every open item, oldest kind of problem first: the workspace, then each
/// PR in inbox order.
pub fn items(s: &Snapshot) -> Vec<Attention> {
    let mut out = vec![];
    if let Some(error) = &s.state_write_error {
        if !s.settings.automatic {
            out.push(Attention {
                fix: Some(Fix::TurnOnAutopilot),
                ..item(
                    "autopilot-paused".into(),
                    AttentionKind::AutopilotPaused,
                    None,
                    None,
                    format!(
                        "Autopilot was turned off because Orbit could not save its state: {error}"
                    ),
                )
            });
        }
    }
    // A missing or signed-out gh won't fix itself, so it shows at once.
    let lasting = s
        .github_error
        .as_deref()
        .is_some_and(|e| e == crate::github::GH_MISSING || e == crate::github::GH_SIGNED_OUT);
    if s.poll_failures >= SYNC_FAILURES || (s.poll_failures > 0 && lasting) {
        out.push(Attention {
            fix: Some(Fix::SyncNow),
            ..item(
                "sync:github".into(),
                AttentionKind::SyncFailing,
                None,
                None,
                format!(
                    "Orbit can't sync with GitHub. {} {}",
                    sentence(s.github_error.as_deref().unwrap_or("Unknown error")),
                    checks_again(s)
                ),
            )
        });
    }
    for f in s.sync_failures.iter().filter(|f| f.count >= SYNC_FAILURES) {
        out.push(Attention {
            repo: Some(f.repo.clone()),
            fix: Some(Fix::SyncNow),
            ..item(
                format!("sync:{}", f.repo.to_ascii_lowercase()),
                AttentionKind::SyncFailing,
                None,
                None,
                format!(
                    "{} can't sync. {} {}",
                    f.repo,
                    sentence(&f.error),
                    checks_again(s)
                ),
            )
        });
    }
    // GitHub is the sync items' to report.
    for c in crate::guide::connections(s) {
        let (Some(error), false) = (&c.error, c.ready || c.needed_by.is_empty()) else {
            continue;
        };
        if c.key == "github" {
            continue;
        }
        let what = if c.key == "dev-containers" {
            "can't run setup and checks in a dev container"
        } else {
            "can't be reviewed"
        };
        out.push(Attention {
            fix: Some(Fix::CheckConnections),
            ..item(
                format!("connection:{}", c.key),
                AttentionKind::Disconnected,
                None,
                None,
                format!(
                    "{} isn't connected, so {} {what}: {}",
                    c.name,
                    crate::guide::some_repositories(&c.needed_by),
                    error.trim()
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
        if let Some(f) = s
            .queue_failures
            .iter()
            .find(|f| f.pr_id == pr.id && f.head_sha == pr.head_sha)
        {
            out.push(item(
                format!("unqueued:{}:{}", pr.id, f.head_sha),
                AttentionKind::Failed,
                Some(&pr.id),
                None,
                format!(
                    "Orbit couldn't start the automatic review. {} Orbit tries again every minute.",
                    sentence(&f.error)
                ),
            ));
        }
        let Some(reason) = state
            .filter(|l| l.status == LoopStatus::NeedsYou)
            .and_then(|l| l.reason)
            .filter(|r| !COVERED.contains(&r.as_str()))
        else {
            continue;
        };
        let latest = latest_automatic(s, pr);
        let fix = match reason.as_str() {
            "Round limit reached." => Some(Fix::ContinueLoop),
            "The report is waiting for you to publish it." => {
                latest.and_then(|r| publish_fix(s, pr, r))
            }
            _ => None,
        };
        out.push(Attention {
            fix,
            ..item(
                format!("loop:{}:{reason}", pr.id),
                AttentionKind::Loop,
                Some(&pr.id),
                latest.map(|r| r.id.as_str()),
                reason,
            )
        });
    }
    out
}
/// A desktop notification for an item.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub item: Attention,
    pub title: String,
    pub body: String,
}
pub fn notice(s: &Snapshot, a: Attention) -> Notice {
    let title = match a.kind {
        AttentionKind::Approval => "A review needs your approval",
        AttentionKind::Held => "A report was held back",
        AttentionKind::Failed => "An automatic review stopped",
        AttentionKind::Loop => "A review loop needs you",
        AttentionKind::AutopilotPaused => "Autopilot was turned off",
        AttentionKind::SyncFailing => "Orbit can't sync",
        AttentionKind::Disconnected => "A connection needs you",
    };
    let pr = s.prs.iter().find(|p| Some(&p.id) == a.pr_id.as_ref());
    let body = match pr {
        Some(pr) => format!("{} · {}\n{}", pr.id, pr.title, a.reason),
        None => a.reason.clone(),
    };
    Notice {
        title: title.into(),
        body,
        item: a,
    }
}
/// The snapshot without what hides a review's outcome for a while: a
/// follow-up runs a finished review again, then restores its outcome; a
/// publication claims a ready report, then posts it or leaves it ready.
fn settled(s: &Snapshot) -> std::borrow::Cow<'_, Snapshot> {
    if !s
        .reviews
        .iter()
        .any(|r| r.before_follow_up.is_some() || r.status == "publishing")
    {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut s = s.clone();
    for r in &mut s.reviews {
        if let Some(before) = &r.before_follow_up {
            r.status = before.status.clone();
            r.error = before.error.clone();
        } else if r.status == "publishing" {
            r.status = "ready".into();
        }
    }
    std::borrow::Cow::Owned(s)
}
/// Records which open items have been announced and returns a notice for
/// each one that is new, with whether the record changed. A resolved item
/// leaves the record, so it is announced again if it opens again later. An
/// item a follow-up chat or a publication hides only while it runs stays in
/// the record, so a publication that fails doesn't announce it again.
pub fn announce(s: &mut Snapshot) -> (Vec<Notice>, bool) {
    let open = items(&settled(s));
    let keys: Vec<String> = open.iter().map(|a| a.key.clone()).collect();
    let changed = keys != s.announced;
    let fresh: Vec<Attention> = open
        .into_iter()
        .filter(|a| !s.announced.contains(&a.key))
        .collect();
    s.announced = keys;
    let notices = fresh.into_iter().map(|a| notice(s, a)).collect();
    (notices, changed)
}
/// The snapshot as the app shows it, with the derived lists filled in.
pub fn shown(s: &Snapshot) -> Snapshot {
    let mut shown = s.clone();
    shown.loop_states = crate::review_loop::states(s);
    shown.attention = items(s);
    shown.mood = Some(crate::buddy::mood(s, &shown.attention, &shown.loop_states));
    shown
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
