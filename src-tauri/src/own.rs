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
}
impl OwnQuestion {
    pub fn pending(&self) -> bool {
        self.answered_at.is_none() && self.error.is_none()
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
