//! Bit's own questions: when a problem that matters opens, or the morning
//! brief comes, Bit may ask its answerer one question about it, as if you
//! had typed it, with the same summary of Orbit, the same buttons and the
//! same tap to use one. Which subjects, how often and when are rules; only
//! the answer comes from the model. Nothing it answers runs or posts by
//! itself.
//!
//! `Buddy::own_questions` is one of `crate::buddy::CALL_MODES`: off asks
//! nothing; watch asks and keeps the answer here, for Operations, without
//! putting it in Bit's feed; on puts it in Bit's feed with its buttons.
//! Each subject is asked about once, and at most `PER_DAY` a day.
//!
//! How each answer turned out is settled by rules too (`settle`): whether
//! you used one of its buttons, and, for a problem, whether it closed
//! within `SETTLE_AFTER` of the answer and stayed closed.
use crate::{
    ask::Proposal,
    brief::Brief,
    problems::{stamp, time, ProblemKind},
    types::*,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// The most questions Bit asks itself in a day.
pub const PER_DAY: usize = 6;
/// How new a problem or brief must be for Bit to ask about it, so turning
/// this on, or Orbit starting again, doesn't ask about old ones.
pub const FRESH: Duration = Duration::hours(1);
/// How long records are kept, and how many.
pub const KEEP_DAYS: i64 = 30;
pub const KEEP: usize = 100;
/// How long after the answer Orbit looks for the problem closing, and for
/// a button of the answer used.
pub const SETTLE_AFTER: Duration = Duration::days(1);

/// How an answer to Bit's own question turned out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Turned {
    /// You used one of its buttons, and the problem closed within a day.
    UsedAndClosed,
    /// You used one of its buttons, and the problem was still open a day
    /// later, or the question was about the brief.
    Used,
    /// The problem closed within a day, with none of its buttons used.
    Closed,
    /// None of its buttons was used within a day, and the problem, if it
    /// was about one, was still open.
    Unused,
    /// The problem's record was gone, so Orbit couldn't tell.
    Unknown,
}

/// A question Bit asked itself, and its answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OwnQuestion {
    /// What it was about, never asked about twice: `problem:<id>@<opened>`
    /// or `brief:<day>`.
    pub subject: String,
    /// The problem it was about, if it was about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub question: String,
    /// `watch` or `on`, when it was asked.
    pub mode: String,
    pub asked_at: String,
    /// Its note in Bit's feed while it is answered, and after when on.
    pub note: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub answer: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<Proposal>,
    /// Why there is no answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When you first used one of its buttons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_at: Option<String>,
    /// How it turned out, once Orbit could tell, and in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turned: Option<Turned>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub how: String,
}
impl OwnQuestion {
    pub fn pending(&self) -> bool {
        self.answered_at.is_none() && self.error.is_none()
    }
    /// Whether its answer had a button you could use: one in Bit's feed.
    pub fn offered(&self) -> bool {
        self.mode == "on" && !self.proposals.is_empty()
    }
}

/// What Bit would ask itself about next.
#[derive(Clone, Debug, PartialEq)]
pub struct Subject {
    pub subject: String,
    pub problem_id: Option<String>,
    pub repo: Option<String>,
    pub question: String,
    /// `watch` or `on`.
    pub mode: String,
}

/// The question about a problem, or none for a kind Bit doesn't ask about.
pub fn problem_question(p: &crate::problems::Problem) -> Option<String> {
    let what = match p.kind {
        ProblemKind::DefaultBranch => {
            format!("CI started failing on the default branch of {}.", p.repo)
        }
        ProblemKind::ReleaseBuild => format!(
            "The release build for {} in {} is failing.",
            p.branch, p.repo
        ),
        _ => return None,
    };
    Some(format!(
        "{what} What's the most useful thing to do about it now?"
    ))
}
/// What a question was about, in a few words: "about CI on a/b", "about
/// the release build for v1.2.0 in a/b", "about the morning brief".
pub fn about(q: &OwnQuestion) -> String {
    let repo = q.repo.as_deref().unwrap_or_default();
    if q.problem_id.is_none() {
        "about the morning brief".into()
    } else if let Some(tag) = q
        .question
        .strip_prefix("The release build for ")
        .and_then(|rest| rest.split_once(" in "))
        .map(|(tag, _)| tag)
    {
        format!("about the release build for {tag} in {repo}")
    } else {
        format!("about CI on {repo}")
    }
}
/// The question about the morning brief.
pub fn brief_question() -> String {
    "What's the one thing in this morning's brief most worth doing first?".into()
}

/// Whether Codex is answering and has used most of what it may, so Bit
/// waits, as write-ups do.
fn codex_busy(s: &Snapshot) -> bool {
    s.settings.buddy.answered_by() == crate::buddy::Answerer::Codex
        && s.health
            .iter()
            .any(|i| i.kind == crate::health::HealthKind::Usage && i.is_open())
}
/// The next subject Bit may ask itself about at `now`, by the rules: its
/// own questions on, Bit on, Codex not near a limit when it answers, fewer
/// than `PER_DAY` asked in the last day, nothing still being answered, and
/// a subject not asked about before, new within `FRESH`: an open default
/// branch or release build problem, oldest first, then today's brief.
/// Whether the answerer is ready, and free, is the engine's to say.
pub fn next(s: &Snapshot, now: DateTime<Utc>) -> Option<Subject> {
    let mode = s.settings.buddy.own_questions.as_str();
    if !matches!(mode, "watch" | "on") || !s.settings.buddy.enabled || codex_busy(s) {
        return None;
    }
    let day = now - Duration::days(1);
    let lately = s
        .own_questions
        .iter()
        .filter(|q| time(&q.asked_at).is_some_and(|t| t > day))
        .count();
    if lately >= PER_DAY || s.own_questions.iter().any(OwnQuestion::pending) {
        return None;
    }
    let asked = |subject: &str| s.own_questions.iter().any(|q| q.subject == subject);
    let fresh = |at: &str| time(at).is_some_and(|t| t <= now && now - t <= FRESH);
    let mut problems: Vec<&crate::problems::Problem> = s
        .problems
        .iter()
        .filter(|p| p.closed_at.is_none() && fresh(&p.opened_at))
        .collect();
    problems.sort_by(|a, b| a.opened_at.cmp(&b.opened_at));
    let problem = problems.into_iter().find_map(|p| {
        let subject = format!("problem:{}@{}", p.id, p.opened_at);
        let question = problem_question(p)?;
        (!asked(&subject)).then(|| Subject {
            subject,
            problem_id: Some(p.id.clone()),
            repo: Some(p.repo.clone()),
            question,
            mode: mode.into(),
        })
    });
    problem.or_else(|| {
        let b: &Brief = s.brief.latest.as_ref().filter(|b| fresh(&b.at))?;
        let subject = format!("brief:{}", b.day);
        (!asked(&subject)).then(|| Subject {
            subject,
            problem_id: None,
            repo: None,
            question: brief_question(),
            mode: mode.into(),
        })
    })
}

/// Records that Bit asked itself about `subject`, in note `note`.
pub fn asked(s: &mut Snapshot, subject: &Subject, note: u64, now: DateTime<Utc>) {
    s.own_questions.push(OwnQuestion {
        subject: subject.subject.clone(),
        problem_id: subject.problem_id.clone(),
        repo: subject.repo.clone(),
        question: subject.question.clone(),
        mode: subject.mode.clone(),
        asked_at: stamp(now),
        note,
        ..Default::default()
    });
}
/// The question still being answered in note `note`, if it is Bit's own.
pub fn of_note(s: &Snapshot, note: u64) -> Option<&OwnQuestion> {
    s.own_questions
        .iter()
        .rev()
        .find(|q| q.note == note && q.pending())
}
/// Keeps the answer to Bit's own question in note `note`, or why there is
/// none. When watching, or with no answer, its note leaves Bit's feed.
pub fn answered(s: &mut Snapshot, note: u64, answer: Result<(String, Vec<Proposal>), String>) {
    let now = stamp(Utc::now());
    let Some(q) = s
        .own_questions
        .iter_mut()
        .rev()
        .find(|q| q.note == note && q.pending())
    else {
        return;
    };
    let keep_note = match answer {
        Ok((text, proposals)) => {
            q.answered_at = Some(now);
            q.answer = text;
            q.proposals = proposals;
            q.mode == "on"
        }
        Err(error) => {
            q.error = Some(error);
            false
        }
    };
    if !keep_note {
        s.buddy_feed.notes.retain(|n| n.id != note);
    }
}
/// Notes that you used a button of the answer to Bit's own question in
/// note `note`. Says whether it was one.
pub fn used(s: &mut Snapshot, note: u64, now: DateTime<Utc>) -> bool {
    let Some(q) = s
        .own_questions
        .iter_mut()
        .rev()
        .find(|q| q.note == note && q.answered_at.is_some())
    else {
        return false;
    };
    if q.used_at.is_none() {
        q.used_at = Some(stamp(now));
    }
    true
}
/// Settles how each answer turned out, once Orbit can tell: for a
/// problem, when it has stayed closed for `problems::FLAP_QUIET` (so a
/// flap doesn't count), or a day and that hour after the answer; for the
/// brief, when you used a button, or a day after the answer. Says whether
/// any settled.
pub fn settle(s: &mut Snapshot, now: DateTime<Utc>) -> bool {
    let mut changed = false;
    for i in 0..s.own_questions.len() {
        let q = &s.own_questions[i];
        if q.settled_at.is_some() {
            continue;
        }
        let Some(answered) = q.answered_at.as_deref().and_then(time) else {
            continue;
        };
        let Some((turned, how)) = turned(s, q, answered, now) else {
            continue;
        };
        let q = &mut s.own_questions[i];
        q.turned = Some(turned);
        q.how = how;
        q.settled_at = Some(stamp(now));
        changed = true;
    }
    changed
}
fn turned(
    s: &Snapshot,
    q: &OwnQuestion,
    answered: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<(Turned, String)> {
    let used = q.used_at.as_deref().and_then(time);
    let day = answered + SETTLE_AFTER;
    let Some(id) = &q.problem_id else {
        if used.is_some() {
            return Some((Turned::Used, "You used its button.".into()));
        }
        if now < day {
            return None;
        }
        let how = if q.offered() {
            "Its buttons weren't used within a day."
        } else {
            "It had no button to use."
        };
        return Some((Turned::Unused, how.into()));
    };
    let Some(p) = s.problems.iter().find(|p| &p.id == id) else {
        return Some((
            Turned::Unknown,
            "Orbit no longer has the problem's record.".into(),
        ));
    };
    let quiet = crate::problems::FLAP_QUIET;
    let closed = p
        .closed_at
        .as_deref()
        .and_then(time)
        .filter(|c| *c <= day && now - *c >= quiet);
    let Some(closed) = closed else {
        if now < day + quiet {
            return None;
        }
        return Some(match used {
            Some(_) => (
                Turned::Used,
                "You used its button; the problem was still open a day later.".into(),
            ),
            None => (
                Turned::Unused,
                "The problem was still open a day after the answer.".into(),
            ),
        });
    };
    let when = if closed <= answered {
        "before the answer came".to_string()
    } else {
        format!(
            "{} after the answer",
            crate::problems::how_long(closed - answered)
        )
    };
    Some(match used.filter(|u| *u <= closed) {
        Some(_) => (
            Turned::UsedAndClosed,
            format!("You used its button, and the problem closed {when}."),
        ),
        None => (Turned::Closed, format!("The problem closed {when}.")),
    })
}
/// Bit's own questions Orbit was answering when it closed get no answer,
/// and their notes leave the feed. Run before `crate::buddy::recover`.
pub fn recover(s: &mut Snapshot) {
    let notes: Vec<u64> = s
        .own_questions
        .iter()
        .filter(|q| q.pending())
        .map(|q| q.note)
        .collect();
    for note in notes {
        answered(s, note, Err("Orbit closed before it was answered.".into()));
    }
}
/// Drops records older than `KEEP_DAYS`, and the oldest beyond `KEEP`,
/// never one still being answered. Returns whether any went.
pub fn prune(list: &mut Vec<OwnQuestion>, now: DateTime<Utc>) -> bool {
    let before = list.len();
    let old = now - Duration::days(KEEP_DAYS);
    list.retain(|q| q.pending() || time(&q.asked_at).is_none_or(|t| t > old));
    let mut excess = list.len().saturating_sub(KEEP);
    list.retain(|q| {
        let drop = excess > 0 && !q.pending();
        if drop {
            excess -= 1;
        }
        !drop
    });
    list.len() != before
}

#[cfg(test)]
#[path = "own_tests.rs"]
mod tests;
