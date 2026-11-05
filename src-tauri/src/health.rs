//! Orbit's own health: a short list of what is wrong with Orbit itself,
//! found by rules from what Orbit already knows, with no model. Each issue
//! is kept like a problem (`crate::problems`): one record from when Orbit
//! first sees it until it clears, with a short timeline, telling you once,
//! again after `problems::REMIND_AFTER` while it stays, and not again if it
//! comes straight back within `problems::FLAP_QUIET`.
//!
//! Unlike problems, these are about this computer and this Orbit, not a
//! repository, so nothing here reaches GitHub or an agent. Those you can do
//! something about (`HealthKind::needs_you`) are in Needs you; the rest,
//! which another item already covers or which only time fixes, are listed
//! on Operations and told to Bit.
use crate::{
    alerts::{raise, Raise, Standing},
    problems::{how_long, stamp, time, ProblemEvent, FLAP_QUIET, MAX_TIMELINE, REMIND_AFTER},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Less free space than this, on the disk that holds Orbit's data, is low.
pub const DISK_FREE: u64 = 2 * crate::actions_storage::GB;
/// So is less than this share of the disk.
pub const DISK_SHARE: f64 = 0.05;
/// A write to GitHub still on its way after this long hasn't settled.
pub const JOURNAL_STUCK: Duration = Duration::hours(1);
/// A post the second look held has waited too long after this.
pub const LOOK_WAITING: Duration = Duration::days(1);
/// Codex disconnected for this long, while something needs it.
pub const CODEX_DOWN: Duration = Duration::minutes(5);
/// Codex usage at this share of a limit or more, as write-ups pause then.
pub const USAGE_HIGH: f64 = 90.0;
/// How long closed issues are kept, and how many at most.
pub const KEEP_CLOSED: Duration = Duration::days(14);
pub const MAX_CLOSED: usize = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthKind {
    /// Little free space where Orbit keeps its data.
    #[default]
    Disk,
    /// Review worktrees Orbit couldn't remove.
    Leftovers,
    /// Writes to GitHub that haven't settled (`crate::ops::Write`).
    Journal,
    /// Posts the second look held that have waited long for you
    /// (`crate::looks`).
    SecondLook,
    /// Codex disconnected for a while, while something needs it.
    Codex,
    /// Codex usage near a limit.
    Usage,
}
impl HealthKind {
    pub const ALL: [HealthKind; 6] = [
        HealthKind::Disk,
        HealthKind::Leftovers,
        HealthKind::Journal,
        HealthKind::SecondLook,
        HealthKind::Codex,
        HealthKind::Usage,
    ];
    /// Whether it is in Needs you. Codex has its own item there already
    /// (`crate::guide::connections`), and usage only waits for its window.
    pub fn needs_you(self) -> bool {
        !matches!(self, HealthKind::Codex | HealthKind::Usage)
    }
    /// What Orbit says when it clears.
    fn cleared(self) -> &'static str {
        match self {
            HealthKind::Disk => "There's room on the disk again.",
            HealthKind::Leftovers => "No worktree is left to remove.",
            HealthKind::Journal => "Every write to GitHub has settled.",
            HealthKind::SecondLook => "No held post has waited more than a day.",
            HealthKind::Codex => "Codex is connected again.",
            HealthKind::Usage => "Codex usage is below 90% again.",
        }
    }
}

/// Something wrong with Orbit itself, from when Orbit first saw it until it
/// cleared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HealthIssue {
    pub id: String,
    pub kind: HealthKind,
    /// What is wrong, as Orbit last saw it.
    pub text: String,
    pub opened_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub closed_reason: Option<String>,
    /// Oldest first.
    pub timeline: Vec<ProblemEvent>,
    /// How many times it has told you; its Needs you item changes with it.
    pub alerts: u32,
    pub last_alert_at: Option<String>,
    /// When one that came back within `FLAP_QUIET` tells you, if it is
    /// still open then.
    pub alert_due: Option<String>,
}
impl HealthIssue {
    pub fn is_open(&self) -> bool {
        self.closed_at.is_none()
    }
    /// The key of its Needs you item, which changes with each alert.
    pub fn item_key(&self) -> String {
        format!("health:{}:{}", self.id, self.alerts)
    }
    fn note(&mut self, at: &str, text: String) {
        self.timeline.push(ProblemEvent {
            at: at.into(),
            text,
        });
        let excess = self.timeline.len().saturating_sub(MAX_TIMELINE);
        self.timeline.drain(..excess);
        self.updated_at = at.into();
    }
    fn alert(&mut self, at: &str) {
        self.alerts += 1;
        self.last_alert_at = Some(at.into());
        self.alert_due = None;
    }
}

/// Something a rule found wrong now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub kind: HealthKind,
    pub text: String,
}

/// Free space on a disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disk {
    pub free: u64,
    pub total: u64,
}
/// The free space on the disk that holds `path`, where Orbit can tell: not
/// yet on Windows.
pub fn disk(path: &Path) -> Option<Disk> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a valid C string and `st` a writable statvfs.
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        let unit = st.f_frsize as u64;
        Some(Disk {
            free: st.f_bavail as u64 * unit,
            total: st.f_blocks as u64 * unit,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// What is wrong with Orbit at `now`, by the rules: the disk that holds its
/// data (`disk`, when Orbit could read it), and from the state, worktrees
/// it couldn't remove, writes to GitHub that haven't settled, held posts
/// that waited long, Codex disconnected since `codex_down` while something
/// needs it, and Codex usage near a limit.
pub fn find(
    s: &Snapshot,
    disk: Option<Disk>,
    codex_down: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Vec<Found> {
    let mut out = vec![];
    let mut found = |kind, text| out.push(Found { kind, text });
    if let Some(d) = disk.filter(|d| low(*d)) {
        found(
            HealthKind::Disk,
            format!(
                "Only {} of {} is free on the disk that holds Orbit's data. Reviews need room for their worktrees and what setup installs. Free some space, or remove worktrees under Settings → Storage.",
                crate::actions_storage::gb(d.free),
                crate::actions_storage::gb(d.total)
            ),
        );
    }
    let failed: Vec<&Review> = s
        .reviews
        .iter()
        // Only worktrees on this computer, which Settings → Storage can
        // remove again.
        .filter(|r| r.cleanup_error.is_some() && r.worktree.is_some())
        .collect();
    if let Some(first) = failed.first() {
        found(
            HealthKind::Leftovers,
            format!(
                "Orbit couldn't remove the worktree of {}: {} Retry under Settings → Storage.",
                count(failed.len(), "review", "reviews"),
                sentence(&shorten(
                    first.cleanup_error.as_deref().unwrap_or_default(),
                    200
                ))
            ),
        );
    }
    let stuck: Vec<&crate::ops::Write> = s
        .journal
        .iter()
        .filter(|w| {
            w.state == crate::ops::WriteState::Sending
                && time(&w.at).is_some_and(|at| now - at >= JOURNAL_STUCK)
        })
        .collect();
    if let Some(oldest) = stuck.iter().min_by(|a, b| a.at.cmp(&b.at)) {
        let place = match oldest.number {
            Some(n) => format!("{}#{n}", oldest.repo),
            None => oldest.repo.clone(),
        };
        found(
            HealthKind::Journal,
            format!(
                "{} to GitHub {} settled after an hour, the oldest for {place}. Orbit looks for each on GitHub before it tries again, so nothing posts twice; Operations lists them.",
                count(stuck.len(), "write", "writes"),
                if stuck.len() == 1 { "hasn't" } else { "haven't" }
            ),
        );
    }
    let held = s
        .ops
        .iter()
        .filter(|t| crate::looks::waiting(s, t))
        .filter(|t| {
            crate::looks::of(s, t)
                .and_then(|l| time(&l.asked_at))
                .is_some_and(|at| now - at >= LOOK_WAITING)
        })
        .count();
    if held > 0 {
        found(
            HealthKind::SecondLook,
            format!(
                "{} the second look held {} waited more than a day for you. Post or drop {} on Operations.",
                count(held, "post", "posts"),
                if held == 1 { "has" } else { "have" },
                if held == 1 { "it" } else { "them" }
            ),
        );
    }
    let codex = crate::guide::connections(s)
        .into_iter()
        .find(|c| c.key == "codex")
        .filter(|c| !c.ready && !c.needed_by.is_empty());
    if let (Some(c), Some(since)) = (codex, codex_down) {
        if now - since >= CODEX_DOWN {
            let base = format!(
                "Codex isn't connected, so {} can't be reviewed",
                crate::guide::some_repositories(&c.needed_by)
            );
            let text = match c.error.as_deref().filter(|e| !e.trim().is_empty()) {
                Some(e) => format!("{base}: {}", sentence(&shorten(e.trim(), 200))),
                None => format!("{base}."),
            };
            found(HealthKind::Codex, text);
        }
    }
    if let Some(text) = s.usage.as_ref().and_then(|u| usage_text(u, now)) {
        found(HealthKind::Usage, text);
    }
    out
}
fn low(d: Disk) -> bool {
    d.total > 0 && (d.free < DISK_FREE || (d.free as f64) < d.total as f64 * DISK_SHARE)
}
/// What Orbit says of Codex usage at or over `USAGE_HIGH`, or nothing.
fn usage_text(u: &crate::usage::Usage, now: DateTime<Utc>) -> Option<String> {
    let l = &u.rate_limits;
    let (window, reached) = [&l.primary, &l.secondary]
        .into_iter()
        .flatten()
        .map(|w| (w, w.used_percent >= 100.0))
        .filter(|(w, _)| w.used_percent >= USAGE_HIGH || l.rate_limit_reached_type.is_some())
        .max_by(|a, b| a.0.used_percent.total_cmp(&b.0.used_percent))?;
    let used = if reached || l.rate_limit_reached_type.is_some() {
        "Codex has reached a usage limit".to_string()
    } else {
        format!(
            "Codex has used {}% of a usage limit",
            window.used_percent.floor()
        )
    };
    let resets = window
        .resets_at
        .and_then(|t| DateTime::<Utc>::from_timestamp(t, 0))
        .filter(|t| *t > now)
        .map(|t| format!(", which starts over in {}", how_long(t - now)))
        .unwrap_or_default();
    Some(format!(
        "{used}{resets}. Write-ups and Bit's answers with Codex wait or may fail until then."
    ))
}
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}
fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?', '…']) {
        text.into()
    } else {
        format!("{text}.")
    }
}
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.into();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// Opens, updates or closes the issues from what the rules found at `now`.
/// Says whether anything changed.
pub fn see(issues: &mut Vec<HealthIssue>, found: &[Found], now: DateTime<Utc>) -> bool {
    let at = stamp(now);
    let mut changed = false;
    for kind in HealthKind::ALL {
        let wrong = found.iter().find(|f| f.kind == kind);
        let open = issues.iter_mut().find(|i| i.kind == kind && i.is_open());
        match (wrong, open) {
            (None, Some(i)) => {
                let reason = kind.cleared().to_string();
                i.note(&at, reason.clone());
                i.closed_at = Some(at.clone());
                i.closed_reason = Some(reason);
                i.alert_due = None;
                changed = true;
            }
            (None, None) => {}
            (Some(f), Some(i)) => {
                if i.text != f.text {
                    i.text = f.text.clone();
                    i.note(&at, f.text.clone());
                    changed = true;
                }
                let due = i.alert_due.as_deref().and_then(time);
                let last = i.last_alert_at.as_deref().and_then(time);
                let standing = match last {
                    Some(last_alert) => Standing::Alerted { last_alert },
                    None => Standing::New,
                };
                if due.is_some_and(|due| now >= due)
                    || raise(standing, false, now, REMIND_AFTER) == Raise::Alert
                {
                    i.alert(&at);
                    changed = true;
                }
            }
            (Some(f), None) => {
                changed = true;
                // One that cleared a moment ago is the same issue back.
                let back = issues
                    .iter_mut()
                    .filter(|i| {
                        i.kind == kind
                            && i.closed_at
                                .as_deref()
                                .and_then(time)
                                .is_some_and(|closed| now - closed < FLAP_QUIET)
                    })
                    .max_by(|a, b| a.opened_at.cmp(&b.opened_at));
                match back {
                    Some(i) => {
                        i.closed_at = None;
                        i.closed_reason = None;
                        i.text = f.text.clone();
                        i.note(&at, format!("Again: {}", f.text));
                        let standing = match i.last_alert_at.as_deref().and_then(time) {
                            Some(last_alert) => Standing::Returned { last_alert },
                            None => Standing::New,
                        };
                        match raise(standing, false, now, FLAP_QUIET) {
                            Raise::Alert => i.alert(&at),
                            Raise::AlertAfter(due) => i.alert_due = Some(stamp(due)),
                            Raise::Quiet => {}
                        }
                    }
                    None => {
                        let mut i = HealthIssue {
                            id: id(),
                            kind,
                            text: f.text.clone(),
                            opened_at: at.clone(),
                            ..Default::default()
                        };
                        i.note(&at, f.text.clone());
                        i.alert(&at);
                        issues.push(i);
                    }
                }
            }
        }
    }
    changed
}

/// The Needs you keys of issues that closed within `FLAP_QUIET`, so one
/// that comes back in that time isn't announced again.
pub fn quiet_keys(issues: &[HealthIssue], now: DateTime<Utc>) -> Vec<String> {
    issues
        .iter()
        .filter(|i| {
            i.kind.needs_you()
                && i.alerts > 0
                && i.closed_at
                    .as_deref()
                    .and_then(time)
                    .is_some_and(|closed| now - closed < FLAP_QUIET)
        })
        .map(HealthIssue::item_key)
        .collect()
}

/// Drops closed issues older than `KEEP_CLOSED`, keeping the newest
/// `MAX_CLOSED` of the rest. Says whether any went.
pub fn prune(issues: &mut Vec<HealthIssue>, now: DateTime<Utc>) -> bool {
    let before = issues.len();
    issues.retain(|i| {
        i.closed_at
            .as_deref()
            .and_then(time)
            .is_none_or(|closed| now - closed < KEEP_CLOSED)
    });
    let mut closed: Vec<(String, String)> = issues
        .iter()
        .filter_map(|i| i.closed_at.clone().map(|at| (at, i.id.clone())))
        .collect();
    if closed.len() > MAX_CLOSED {
        closed.sort_by(|a, b| b.cmp(a));
        let keep: std::collections::HashSet<String> = closed
            .into_iter()
            .take(MAX_CLOSED)
            .map(|(_, id)| id)
            .collect();
        issues.retain(|i| i.closed_at.is_none() || keep.contains(&i.id));
    }
    issues.len() != before
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
