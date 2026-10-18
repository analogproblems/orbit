//! Keeps the saved state from growing forever: the whole file is written
//! on every change, so old reviews of closed pull requests are let go.
//! Everything about open pull requests is kept.
use super::*;

/// How many of a closed pull request's reviews are kept, newest first.
pub(super) const KEEP_ROUNDS: usize = 3;
/// How long a closed pull request's review keeps its chat after it last
/// changed.
pub(super) const KEEP_CHAT_DAYS: i64 = 30;

/// Removes all but the newest `KEEP_ROUNDS` reviews of each closed pull
/// request, and the chat of those kept once `KEEP_CHAT_DAYS` have passed.
/// A review still running, waiting on an approval or a Needs you item, or
/// with a worktree to clean up is never touched. Says whether anything
/// changed.
pub(super) fn prune(s: &mut Snapshot, now: chrono::DateTime<chrono::Utc>) -> bool {
    let open: HashSet<&str> = s.prs.iter().map(|p| p.id.as_str()).collect();
    let pinned: HashSet<String> = s
        .approvals
        .iter()
        .map(|a| a.review_id.clone())
        .chain(s.attention.iter().filter_map(|a| a.review_id.clone()))
        .collect();
    let keep = |r: &Review| {
        open.contains(r.pr_id.as_str())
            || active(&r.status)
            || r.worktree.is_some()
            || r.cleanup_pending
            || pinned.contains(&r.id)
    };
    // Each closed pull request's reviews, newest first.
    let mut by_pr: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for r in &s.reviews {
        if !open.contains(r.pr_id.as_str()) {
            by_pr
                .entry(&r.pr_id)
                .or_default()
                .push((&r.created_at, &r.id));
        }
    }
    let mut old: HashSet<String> = HashSet::new();
    for reviews in by_pr.values_mut() {
        reviews.sort_by(|a, b| b.cmp(a));
        old.extend(
            reviews
                .iter()
                .skip(KEEP_ROUNDS)
                .map(|(_, id)| id.to_string()),
        );
    }
    let before = s.reviews.len();
    s.reviews.retain(|r| keep(r) || !old.contains(&r.id));
    let mut changed = s.reviews.len() != before;
    let quiet = |r: &Review| {
        chrono::DateTime::parse_from_rfc3339(&r.updated_at)
            .is_ok_and(|at| now.signed_duration_since(at).num_days() >= KEEP_CHAT_DAYS)
    };
    for r in &mut s.reviews {
        if !keep(r) && !r.messages.is_empty() && quiet(r) {
            r.messages.clear();
            changed = true;
        }
    }
    changed
}

impl Engine {
    /// Prunes the saved state, saving only when something was let go.
    pub(super) async fn prune_state(&self) {
        let mut s = self.state.lock().await;
        if prune(&mut s, chrono::Utc::now()) {
            self.dirty.store(true, Ordering::SeqCst);
        }
    }
}
