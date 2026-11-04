//! What done looks like for each kind of problem, and how far along it is
//! (see `crate::problems`).
//!
//! **Playbooks.** Each kind of problem about failing checks has a built-in
//! playbook (`steps`): the steps to done, in order, and who does each one,
//! Orbit, the agent or you. For a default branch: Orbit finds the commit
//! that broke it, Orbit tells the agent, the agent takes it, a fix lands,
//! every check passes, and it stays fixed for `HOLDS_DAYS` days.
//!
//! **Ticks.** Code ticks each step from what Orbit already reads, on each
//! pass (`follow`): the problem itself, Orbit's comment or issue about it
//! and the agent's answers there (`crate::ops`), and the outcome check of
//! a default branch staying fixed (`crate::outcomes`). An agent's word that
//! it fixed it counts until Orbit sees checks fail at that commit, or the
//! commit isn't on the branch (`Answer::check`). A step is to do, done,
//! skipped (no fix landed, as it stopped failing at the commit it failed
//! at, which is bad luck only when its case saw each job that failed pass
//! again there, or Orbit couldn't tell whether it stayed fixed), or missed
//! (it failed again
//! within the days it should have stayed fixed, or Orbit stopped following
//! it before it passed). It runs before Orbit
//! refreshes its comments and issues, so the items failing there are this
//! problem's. Once the problem closes, its comment may move on to another
//! problem, so the steps it ticked on the pass it closed on, which still
//! reads the comment as it was, stay ticked (`Problem.playbook_closed`);
//! while the problem is open, or open again within the hour, each step
//! says where it stands now.
//!
//! Guidance, never authority: a playbook says what's left, on Operations,
//! in Orbit's comment or issue for the agent (`done_text`) and to Bit. It
//! changes no rule, and nothing here posts anything or reads GitHub.
use crate::{
    broker::clean,
    cases::{Case, End},
    ci::short,
    ops::{Answer, AnswerKind, FixCheck, Item, ItemKind, Thread},
    outcomes::{Kind, Outcome, Verdict, HOLDS_DAYS},
    problems::{Problem, ProblemKind},
    types::*,
};
use serde::{Deserialize, Serialize};

/// One step of a playbook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Orbit finds the commit that broke a default branch.
    #[default]
    Found,
    /// Orbit tells the agent, in its comment or issue.
    Told,
    /// The agent takes it.
    Taken,
    /// A fix lands.
    Fixed,
    /// Every check passes.
    Passes,
    /// A default branch stays fixed for `HOLDS_DAYS` days.
    Holds,
}

/// Who does a step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    #[default]
    Orbit,
    Agent,
    You,
}

/// Where a step stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    #[default]
    Todo,
    Done,
    /// Skipped: no fix landed, as it stopped failing at the commit it
    /// failed at, or Orbit couldn't tell whether it stayed fixed.
    Skipped,
    /// It didn't happen: it failed again, or Orbit stopped following it.
    Missed,
}

/// A step of a problem's playbook, and where it stands (`Problem.playbook`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Step {
    pub step: StepKind,
    pub who: Who,
    /// What the step is, in a sentence, as the app shows it.
    pub text: String,
    pub mark: Mark,
    /// When it was done, found not needed or missed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// How, in a sentence, such as "The agent said it fixed it in 1a2b3c4."
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// For staying fixed: when Orbit is sure, while it waits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
}

/// The steps of a kind of problem's playbook, in order, and who does each;
/// none for a kind without one yet.
pub fn steps(kind: ProblemKind) -> &'static [(StepKind, Who)] {
    use StepKind::*;
    use Who::*;
    match kind {
        ProblemKind::DefaultBranch => &[
            (Found, Orbit),
            (Told, Orbit),
            (Taken, Agent),
            (Fixed, Agent),
            (Passes, Orbit),
            (Holds, Orbit),
        ],
        ProblemKind::PullRequest | ProblemKind::BaseMoved | ProblemKind::ReleaseBuild => &[
            (Told, Orbit),
            (Taken, Agent),
            (Fixed, Agent),
            (Passes, Orbit),
        ],
        _ => &[],
    }
}

/// Code formatting of names for the agent, or none for the app.
type Show<'a> = &'a dyn Fn(&str) -> String;

fn branch(p: &Problem, show: Show) -> String {
    if p.branch.is_empty() {
        "the default branch".into()
    } else {
        show(&clean(&p.branch, 100))
    }
}

/// What a step is, in a sentence, for the app.
fn text(p: &Problem, step: StepKind) -> String {
    use StepKind::*;
    let plain = &str::to_string;
    let base = || plain(&clean(&p.base, 100));
    match (p.kind, step) {
        (_, Found) => "Orbit finds the commit that broke it.".into(),
        (ProblemKind::PullRequest | ProblemKind::BaseMoved, Told) => {
            "Orbit tells the agent in its comment on the pull request.".into()
        }
        (_, Told) => "Orbit tells the agent in an issue.".into(),
        (_, Taken) => "The agent takes it.".into(),
        (ProblemKind::DefaultBranch, Fixed) => format!("A fix lands on {}.", branch(p, plain)),
        (ProblemKind::ReleaseBuild, Fixed) => {
            "A fix lands on the default branch, for the next version.".into()
        }
        (ProblemKind::BaseMoved, Fixed) => format!("The agent merges {} in.", base()),
        (_, Fixed) => "A fix lands on the pull request.".into(),
        (ProblemKind::DefaultBranch, Passes) => {
            format!("Every check passes on {}.", branch(p, plain))
        }
        (ProblemKind::ReleaseBuild, Passes) => "Every run of the release build passes.".into(),
        (ProblemKind::BaseMoved, Passes) => {
            format!("Every check passes again with {}'s latest.", base())
        }
        (_, Passes) => "Every check passes.".into(),
        (_, Holds) => format!("It stays fixed for {HOLDS_DAYS} days."),
    }
}

/// What done looks like, for the agent, as a numbered list and a blank
/// line, while the problem is open; or nothing. It names no step Orbit does
/// before the agent reads it, and it doesn't change as steps are ticked, so
/// Orbit's comment or issue is edited only as CI changes.
pub fn done_text(p: &Problem, show: Show) -> String {
    use StepKind::*;
    if !p.is_open() {
        return String::new();
    }
    let base = || show(&clean(&p.base, 100));
    let lines: Vec<String> = steps(p.kind)
        .iter()
        .filter_map(|(step, _)| {
            Some(match (p.kind, step) {
                (_, Found | Told) => return None,
                (_, Taken) => "An agent takes each item.".into(),
                (ProblemKind::DefaultBranch, Fixed) => {
                    format!("A fix lands on {}.", branch(p, show))
                }
                (ProblemKind::ReleaseBuild, Fixed) => {
                    "A fix lands on the default branch, so the next version's tag builds with it."
                        .into()
                }
                (ProblemKind::BaseMoved, Fixed) => {
                    format!("{} is merged in, and CI runs again.", base())
                }
                (_, Fixed) => "A fix lands on this branch.".into(),
                (ProblemKind::DefaultBranch, Passes) => {
                    format!("Every check passes on {}.", branch(p, show))
                }
                (ProblemKind::ReleaseBuild, Passes) => format!(
                    "Every run of the release build for {} passes, such as after a rerun, or a newer version is tagged.",
                    show(&clean(&p.branch, 100))
                ),
                (ProblemKind::BaseMoved, Passes) => format!(
                    "Every check passes, having begun after {}'s latest commit.",
                    base()
                ),
                (_, Passes) => "Every check passes.".into(),
                (_, Holds) => format!("It stays fixed for {HOLDS_DAYS} days."),
            })
        })
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = (1..)
        .zip(&lines)
        .map(|(n, line)| format!("{n}. {line}"))
        .collect();
    format!("What done looks like:\n\n{}\n\n", lines.join("\n"))
}

/// Ticks each problem's playbook from what Orbit reads now. Returns
/// whether any changed.
pub fn follow(s: &mut Snapshot) -> bool {
    let mut changed = false;
    for i in 0..s.problems.len() {
        let p = &s.problems[i];
        if steps(p.kind).is_empty() {
            continue;
        }
        let thread = s.ops.iter().find(|t| t.problem_id == p.id);
        let case = s.cases.iter().find(|c| c.id == p.id);
        let next = playbook(p, thread, case, &s.outcomes);
        let closed = !p.is_open();
        let p = &mut s.problems[i];
        if p.playbook != next || p.playbook_closed != closed {
            p.playbook = next;
            p.playbook_closed = closed;
            changed = true;
        }
    }
    changed
}

/// The items on a problem's comment or issue that are about it now: those
/// failing, of its kind. Orbit marks them as no longer failing once it sees
/// them pass, after this has read them.
fn items<'a>(p: &Problem, t: Option<&'a Thread>) -> Vec<&'a Item> {
    let kind = match p.kind {
        ProblemKind::BaseMoved => ItemKind::BaseMoved,
        _ => ItemKind::Check,
    };
    t.map(|t| {
        t.items
            .iter()
            .filter(|i| i.failing && i.kind == kind)
            .collect()
    })
    .unwrap_or_default()
}

fn mark(mark: Mark, at: Option<&str>, note: impl Into<String>) -> (Mark, Option<String>, String) {
    (mark, at.map(str::to_string), note.into())
}

/// A problem's playbook as Orbit reads it now (see the module's docs).
fn playbook(
    p: &Problem,
    t: Option<&Thread>,
    case: Option<&Case>,
    outcomes: &[Outcome],
) -> Vec<Step> {
    use StepKind::*;
    let items = items(p, t);
    let answers = || items.iter().filter_map(|i| i.answer.as_ref());
    let earliest = |kinds: &[AnswerKind]| {
        answers()
            .filter(|a| kinds.contains(&a.kind))
            .min_by(|a, b| a.at.cmp(&b.at))
    };
    let closed = p.closed_at.as_deref();
    let todo = || mark(Mark::Todo, None, "");
    steps(p.kind)
        .iter()
        .map(|&(step, who)| {
            let (m, at, note) = match step {
                Found => match p.broke.as_ref().filter(|b| b.done) {
                    None => todo(),
                    Some(b) => match crate::broke::text(b) {
                        Some(text) => mark(Mark::Done, None, text),
                        None => mark(
                            Mark::Missed,
                            None,
                            match &b.error {
                                Some(e) => format!("Orbit couldn't tell: {}", clean(e, 200)),
                                None => "Orbit couldn't tell which commit broke it.".into(),
                            },
                        ),
                    },
                },
                Told => match items
                    .iter()
                    .filter(|i| !i.heard_from.is_empty())
                    .map(|i| i.heard_from.as_str())
                    .min()
                {
                    Some(at) => mark(Mark::Done, Some(at), ""),
                    None => todo(),
                },
                Taken => match earliest(&[AnswerKind::Taking, AnswerKind::Fixed]) {
                    Some(a) => mark(
                        Mark::Done,
                        Some(&a.at),
                        format!("{} said they're on it.", clean(&a.author, 40)),
                    ),
                    None if answers().any(|a| a.kind == AnswerKind::Question) => {
                        mark(Mark::Todo, None, "The agent asked for a person.")
                    }
                    None if answers().any(|a| a.kind == AnswerKind::Declined) => {
                        mark(Mark::Todo, None, "The agent declined it.")
                    }
                    None => todo(),
                },
                // An agent's fix counts until Orbit sees its checks fail
                // at that commit, or the commit isn't there.
                Fixed => match answers()
                    .filter(|a| a.kind == AnswerKind::Fixed && !refuted(a))
                    .min_by(|a, b| a.at.cmp(&b.at))
                {
                    Some(a) => mark(
                        Mark::Done,
                        Some(&a.at),
                        match fix_commit(a) {
                            Some(c) => format!("The agent said it fixed it in {c}."),
                            None => "The agent said it fixed it.".into(),
                        },
                    ),
                    // Bad luck only when its case saw each job that
                    // failed pass again in its own run (`cases::passed_again`).
                    None if p.passed() && p.closed_sha == p.sha => mark(
                        Mark::Skipped,
                        closed,
                        if case.is_some_and(|c| c.end == End::Again) {
                            "It passed when it ran again at the same commit, so no fix was needed."
                        } else {
                            "No fix landed: it stopped failing at the same commit, but Orbit didn't see what failed pass there."
                        },
                    ),
                    None if p.passed() => mark(
                        Mark::Done,
                        closed,
                        format!("It passed at {}.", short(&p.closed_sha)),
                    ),
                    None => match answers()
                        .filter(|a| a.kind == AnswerKind::Fixed)
                        .max_by(|a, b| a.at.cmp(&b.at))
                    {
                        Some(a) => mark(Mark::Todo, None, not_fixed(p, a)),
                        None => todo(),
                    },
                },
                Passes if p.passed() => mark(
                    Mark::Done,
                    closed,
                    clean(p.closed_reason.as_deref().unwrap_or_default(), 200),
                ),
                Passes if !p.is_open() => mark(
                    Mark::Missed,
                    closed,
                    format!(
                        "Orbit stopped following it. {}",
                        clean(p.closed_reason.as_deref().unwrap_or_default(), 200)
                    )
                    .trim_end()
                    .to_string(),
                ),
                Passes => todo(),
                Holds => return holds(p, who, outcomes),
            };
            let mut s = Step {
                step,
                who,
                text: text(p, step),
                mark: m,
                at,
                note,
                due: None,
            };
            // What the comment or issue showed stays once the problem
            // closed: it may be about another problem by now. The pass it
            // closes on still reads it as it was, with the latest answers
            // and what Orbit found checking a fix, so only later passes
            // keep what that one ticked.
            let shown = matches!(step, Told | Taken | Fixed);
            if let Some(old) = p.playbook.iter().find(|o| o.step == step) {
                if shown && !p.is_open() && p.playbook_closed && old.mark == Mark::Done {
                    s = Step {
                        text: s.text,
                        ..old.clone()
                    };
                }
            }
            s
        })
        .collect()
}

/// The commit an agent said fixes it, short.
fn fix_commit(a: &Answer) -> Option<&str> {
    a.commit.as_deref().filter(|c| valid_sha(c)).map(short)
}

/// Whether Orbit checked an agent's fix and it didn't hold up.
fn refuted(a: &Answer) -> bool {
    matches!(
        a.check,
        Some(FixCheck::Fails | FixCheck::NotOnBranch | FixCheck::NotFound)
    )
}

/// Why an agent's fix doesn't count yet.
fn not_fixed(p: &Problem, a: &Answer) -> String {
    let Some(c) = fix_commit(a) else {
        return "The agent said it fixed it.".into();
    };
    match a.check {
        Some(FixCheck::Fails) => {
            format!("The agent said it fixed it in {c}, but checks still fail there.")
        }
        Some(FixCheck::NotOnBranch) => format!(
            "The agent said it fixed it in {c}, but that commit isn't on {}.",
            branch(p, &str::to_string)
        ),
        Some(FixCheck::NotFound) => {
            format!("The agent said it fixed it in {c}, but GitHub doesn't know that commit.")
        }
        _ => format!("The agent said it fixed it in {c}."),
    }
}

/// Whether a default branch stayed fixed, by its outcome check (see
/// `crate::outcomes`): to do until it passed, then waiting until the check
/// is due, done when it held, missed when it failed again, and not needed
/// when Orbit couldn't tell.
fn holds(p: &Problem, who: Who, outcomes: &[Outcome]) -> Step {
    let mut s = Step {
        step: StepKind::Holds,
        who,
        text: text(p, StepKind::Holds),
        ..Default::default()
    };
    if !p.passed() {
        return s;
    }
    let Some(o) = outcomes
        .iter()
        .find(|o| o.kind == Kind::Holds && o.problem == p.id && o.sha == p.closed_sha)
    else {
        return s;
    };
    let why = clean(&o.why, 200);
    let (m, note) = match o.verdict {
        Verdict::Waiting => {
            s.due = o.due.clone();
            return s;
        }
        Verdict::Held if why.is_empty() => (
            Mark::Done,
            format!("It stayed fixed for {HOLDS_DAYS} days."),
        ),
        Verdict::Held => (Mark::Done, why),
        Verdict::Missed => (Mark::Missed, why),
        Verdict::Unclear => (Mark::Skipped, format!("Orbit couldn't tell: {why}")),
    };
    s.mark = m;
    s.note = note;
    s.at = o.settled_at.clone();
    s
}

/// What's left of a problem's playbook, and what was done, one line each,
/// for Bit.
pub fn summary(p: &Problem) -> Vec<serde_json::Value> {
    p.playbook
        .iter()
        .map(|s| {
            serde_json::json!({
                "step": s.text,
                "who": s.who,
                "state": s.mark,
                "at": s.at,
                "note": (!s.note.is_empty()).then(|| clean(&s.note, 300)),
                "due": s.due,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "playbooks_tests.rs"]
mod tests;
