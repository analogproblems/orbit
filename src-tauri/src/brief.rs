//! Bit's morning brief: once a day, at the time you choose in Bit's
//! settings, Bit sums up the pull requests clear to merge, the stuck ones
//! and whose move it is, the open CI problems, and what it notes about
//! runners and storage. It marks each line that is new since the last brief
//! and counts what settled since then. On a day with nothing new and
//! nothing settled, nothing is sent. A brief shows in Bit's conversation
//! and in Needs you until you mark it read or the next brief's time comes.
//! Nothing here calls a model or reaches GitHub.
use crate::{
    ci::short,
    problems::{stamp, ProblemKind},
    readiness::{Turn, WaitKind},
    types::*,
};
use chrono::{DateTime, NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

/// When the brief comes, unless you choose another time.
pub const DEFAULT_AT: &str = "09:00";

/// One thing a brief lists.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Line {
    /// The same while the thing stays the same, so the next brief can tell
    /// what is new and what settled.
    pub key: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_id: Option<String>,
    /// Not in the brief before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub new: bool,
}
/// A brief Orbit sent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Brief {
    /// The day it is for, by this computer's clock: `2026-10-31`.
    pub day: String,
    /// When Orbit made it, by Orbit's clock.
    pub at: String,
    /// What Bit says first: how many of each, what is new and what settled.
    pub text: String,
    /// Clear to merge first, then stuck, CI problems, and runners and
    /// storage.
    pub lines: Vec<Line>,
    /// How many lines of the brief before are gone.
    pub settled: u32,
    /// In Needs you, until you mark it read or the next brief's time comes.
    pub open: bool,
}
/// What Orbit keeps about briefs, saved with the state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Briefs {
    /// The latest day Orbit looked at whether to send a brief, whether it
    /// sent one or not, so it looks once a day.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub looked: String,
    /// The latest brief Orbit sent, which the next one is compared with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<Brief>,
}

/// A time of day as Bit's settings keep it, `HH:MM` on a 24-hour clock.
pub fn time_of(at: &str) -> Option<NaiveTime> {
    let b = at.as_bytes();
    let digits = [0, 1, 3, 4]
        .iter()
        .all(|&i| b.get(i).is_some_and(u8::is_ascii_digit));
    if b.len() != 5 || b[2] != b':' || !digits {
        return None;
    }
    NaiveTime::parse_from_str(at, "%H:%M").ok()
}
/// The day a brief is due for at `now`, this computer's time, if one is:
/// once `at` has come on a day later than the one Orbit last looked on.
pub fn due(looked: &str, at: &str, now: NaiveDateTime) -> Option<String> {
    let day = now.date().format("%Y-%m-%d").to_string();
    (now.time() >= time_of(at)? && day.as_str() > looked).then_some(day)
}

/// The key of a brief's Needs you item.
pub fn item_key(b: &Brief) -> String {
    format!("brief:{}", b.day)
}

/// What a brief would list now, none of it marked new.
pub fn lines(s: &Snapshot) -> Vec<Line> {
    let mut clear = vec![];
    let mut stuck = vec![];
    for r in crate::readiness::all(s) {
        let Some(pr) = s.prs.iter().find(|p| p.id == r.pr_id) else {
            continue;
        };
        let label = crate::buddy::label(pr);
        if r.clear {
            clear.push(line(
                format!("clear:{}:{}:{}", pr.id, r.head_sha, r.base_sha),
                format!("{label}: {}", r.line()),
                Some(&pr.id),
            ));
        } else if let Some(text) = stuck_text(s, &r) {
            let since = ready_of(s, &pr.id).map_or("", |r| r.since.as_str());
            stuck.push(line(
                format!("stuck:{}:{since}", pr.id),
                format!("{label} {text}"),
                Some(&pr.id),
            ));
        }
    }
    let mut failing = vec![];
    let mut notes = vec![];
    for p in s.problems.iter().filter(|p| p.is_open()) {
        let key = format!("problem:{}", p.id);
        let pr = || {
            p.pr_id.as_deref().map_or_else(
                || p.repo.clone(),
                |id| crate::buddy::pr_label(s, Some(id)).unwrap_or_else(|| id.to_string()),
            )
        };
        let text = match p.kind {
            ProblemKind::Waiting => {
                notes.push(line(
                    format!("runs:{}", p.id),
                    crate::problems::waiting_text(&p.repo, &p.waiting),
                    None,
                ));
                continue;
            }
            ProblemKind::DefaultBranch => format!(
                "CI is failing on {} in {} since {}: {}.",
                if p.branch.is_empty() {
                    "the default branch"
                } else {
                    &p.branch
                },
                p.repo,
                short(&p.since_sha),
                crate::problems::names(&p.failing)
            ),
            ProblemKind::PullRequest => format!(
                "CI is failing on {} at {}: {}.",
                pr(),
                short(&p.sha),
                crate::problems::names(&p.failing)
            ),
            ProblemKind::BaseMoved => format!(
                "{}'s checks ran without {}'s latest commit, {}.",
                pr(),
                p.base,
                short(&p.base_sha)
            ),
            ProblemKind::NoCi => format!("No CI runs on the pull requests of {}.", p.repo),
        };
        failing.push(Line {
            pr_id: p.pr_id.clone(),
            ..line(key, text, None)
        });
    }
    // Runners and storage are read only while Watch CI is on.
    if s.settings.watch_ci {
        notes.extend(offline(s));
        use crate::actions_storage::{gb, CACHE_LIMIT, GB, NEAR};
        for st in &s.actions_storage {
            if st.unseen.is_none() && st.cache_bytes >= NEAR {
                let mut text = format!(
                    "{}'s caches hold {}, near GitHub's usual {} GB limit, so GitHub may soon drop the ones used least.",
                    st.repo,
                    gb(st.cache_bytes),
                    CACHE_LIMIT / GB
                );
                if st.error.is_some() {
                    text.push_str(
                        " Orbit's latest read of its storage failed, so this may be out of date.",
                    );
                }
                notes.push(line(
                    format!("caches:{}", st.repo.to_ascii_lowercase()),
                    text,
                    None,
                ));
            }
        }
    }
    [clear, stuck, failing, notes].concat()
}
fn line(key: String, text: String, pr_id: Option<&str>) -> Line {
    Line {
        key,
        text,
        pr_id: pr_id.map(str::to_owned),
        new: false,
    }
}
/// The answer Orbit keeps for a pull request on its comment.
fn ready_of<'a>(s: &'a Snapshot, pr_id: &str) -> Option<&'a crate::ops::Ready> {
    let key = format!("pr:{pr_id}");
    s.ops.iter().find(|t| t.key == key)?.ready.as_ref()
}
/// How long a pull request has waited and on whom, once Orbit nudged them
/// (`crate::ops::ready`), while the first thing in the way is still theirs
/// at the same head: "has waited on its author for more than 2 hours: …".
fn stuck_text(s: &Snapshot, now: &crate::readiness::Readiness) -> Option<String> {
    let r = ready_of(s, &now.pr_id)?;
    let w = now.waits.first()?;
    if r.clear || r.nudged.is_none() || r.turn != Some(w.turn) || r.head_sha != now.head_sha {
        return None;
    }
    let (whom, long) = match w.turn {
        Turn::Author => (
            "its author",
            format!("{} hours", crate::ops::NUDGE_AGENT.num_hours()),
        ),
        // As in Needs you (`crate::attention`): a reviewer or a rule.
        Turn::Person if matches!(w.kind, WaitKind::Merge | WaitKind::Reviews) => {
            ("a person", "a day".to_string())
        }
        _ => return None,
    };
    Some(format!(
        "has waited on {whom} for more than {long}: {}",
        w.text
    ))
}
/// A line for each place whose self-hosted runners Orbit sees are all
/// offline, saying when Orbit read only some of them or its latest read
/// failed, as Operations does (`crate::runners`).
fn offline(s: &Snapshot) -> Vec<Line> {
    use crate::runners::PlaceKind;
    s.runners
        .iter()
        .filter(|p| p.unseen.is_none() && !p.runners.is_empty())
        .filter(|p| p.runners.iter().all(|r| !r.online))
        .filter_map(|p| {
            let (whose, of) = match p.kind {
                PlaceKind::Repository => (p.name.clone(), p.name.clone()),
                PlaceKind::Organization => (
                    format!("The {} organization", p.name),
                    format!("the {} organization", p.name),
                ),
                _ => return None,
            };
            // GitHub may have more than Orbit read, so only those are
            // said to be offline.
            let n = p.runners.len();
            let mut text = match (n, (p.total as usize).max(n)) {
                (1, 1) => format!("{whose}'s one self-hosted runner is offline."),
                (n, total) if n == total => {
                    format!("{whose}'s {n} self-hosted runners are all offline.")
                }
                (1, total) => {
                    format!("The one self-hosted runner Orbit read of {of}'s {total} is offline.")
                }
                (n, total) => format!(
                    "The {n} self-hosted runners Orbit read of {of}'s {total} are all offline."
                ),
            };
            if p.error.is_some() {
                text.push_str(" Orbit's latest read of them failed, so this may be out of date.");
            }
            Some(line(
                format!("offline:{}", p.name.to_ascii_lowercase()),
                text,
                None,
            ))
        })
        .collect()
}

/// Whether a read the brief sums up failed last time, so what Orbit kept
/// from before may no longer be true: syncing any repository's pull
/// requests, reading a repository's CI, or asking GitHub whether a pull
/// request merges. Runners and storage are read less often, so their lines say
/// instead when their latest read failed.
fn unread(s: &Snapshot) -> bool {
    s.poll_failures > 0
        || s.github_error.is_some()
        || s.ci.iter().any(|c| c.error.is_some())
        || s.merges.iter().any(|m| m.error.is_some())
}

/// Looks once a day, at the time Bit's settings give, whether there is a
/// brief to send, and makes it if there is. `local` is this computer's
/// time and `now` Orbit's clock. Looking takes yesterday's brief out of
/// Needs you. Nothing happens while Bit or the brief is off, or while a
/// read it sums up failed (`unread`), so a brief is never made from what
/// Orbit couldn't read, and the day waits for a pass that reads it all.
/// Returns whether anything changed.
pub fn look(s: &mut Snapshot, local: NaiveDateTime, now: DateTime<Utc>) -> bool {
    let buddy = &s.settings.buddy;
    if !buddy.enabled || !buddy.brief || unread(s) {
        return false;
    }
    let Some(day) = due(&s.brief.looked, &buddy.brief_at, local) else {
        return false;
    };
    let mut current = lines(s);
    let before: Vec<String> = s
        .brief
        .latest
        .as_ref()
        .map_or(vec![], |b| b.lines.iter().map(|l| l.key.clone()).collect());
    for l in &mut current {
        l.new = !before.contains(&l.key);
    }
    let settled = before
        .iter()
        .filter(|k| !current.iter().any(|l| &&l.key == k))
        .count() as u32;
    s.brief.looked = day.clone();
    if let Some(b) = s.brief.latest.as_mut() {
        b.open = false;
    }
    if settled == 0 && current.iter().all(|l| !l.new) {
        return true;
    }
    let first = s.brief.latest.is_none();
    let text = say(&s.settings.buddy.tone, &current, settled, first);
    s.brief.latest = Some(Brief {
        day,
        at: stamp(now),
        text,
        lines: current,
        settled,
        open: true,
    });
    true
}
/// Takes the brief for `day` out of Needs you, as you read it. Returns
/// whether it was there.
pub fn close(s: &mut Snapshot, day: &str) -> bool {
    match s.brief.latest.as_mut() {
        Some(b) if b.day == day && b.open => {
            b.open = false;
            true
        }
        _ => false,
    }
}
/// Takes the brief out of Needs you once Bit or the brief is turned off, so
/// turning it back on doesn't tell it again.
pub fn settings_changed(s: &mut Snapshot) {
    let buddy = &s.settings.buddy;
    if !buddy.enabled || !buddy.brief {
        if let Some(b) = s.brief.latest.as_mut() {
            b.open = false;
        }
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
fn and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
/// What Bit says first, in its tone: how many of each kind of line, how
/// many are new, unless this is the first brief, and how many settled.
fn say(tone: &str, lines: &[Line], settled: u32, first: bool) -> String {
    let count = |prefix: &str| lines.iter().filter(|l| l.key.starts_with(prefix)).count();
    let (clear, stuck, failing) = (count("clear:"), count("stuck:"), count("problem:"));
    let notes = lines.len() - clear - stuck - failing;
    let parts: Vec<String> = [
        (
            clear,
            "pull request is clear to merge",
            "pull requests are clear to merge",
        ),
        (stuck, "pull request is stuck", "pull requests are stuck"),
        (failing, "CI problem is open", "CI problems are open"),
        (
            notes,
            "note is about runners or storage",
            "notes are about runners or storage",
        ),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, one, many)| plural(n, one, many))
    .collect();
    let mut out = vec![];
    if parts.is_empty() {
        out.push("Nothing is clear to merge, stuck or failing.".to_string());
    } else {
        out.push(format!("{}.", and(&parts)));
    }
    let new = lines.iter().filter(|l| l.new).count();
    if !first && new > 0 {
        out.push(match new {
            1 if lines.len() == 1 => "It is new since the last brief.".to_string(),
            n if n == lines.len() => "All of it is new since the last brief.".into(),
            n => format!("{} new since the last brief.", plural(n, "is", "are")),
        });
    }
    if settled > 0 {
        out.push(plural(
            settled as usize,
            "thing from the last brief has settled.",
            "things from the last brief have settled.",
        ));
    }
    let summary = out.join(" ");
    match tone {
        "calm" => format!("Your morning brief. {summary}"),
        "dry" => format!("Morning brief. {summary} That's the lot."),
        _ => format!("Here's your morning brief! {summary}"),
    }
}

#[cfg(test)]
#[path = "brief_tests.rs"]
mod tests;
