//! What done looks like for each kind of problem, and how far along it is
//! (see `crate::problems`).
//!
//! **Playbooks.** Each kind of problem has a built-in playbook (`steps`):
//! the steps to done, in order, and who does each one, Orbit, the agent or
//! you. For a default branch: Orbit finds the commit that broke it, Orbit
//! tells the agent, the agent takes it, a fix lands, every check passes,
//! and it stays fixed for `HOLDS_DAYS` days. Any other kind Orbit tells an
//! agent about (`crate::ops::tells_agent`) goes from telling it to done;
//! the rest, which only a person can fix or which an issue would show
//! others, go from your fix to done (`done`). Alerts, risky settings and
//! version files are each their own thing, so a fix is done once the agent
//! named one for each still open.
//!
//! **Ticks.** Code ticks each step from what Orbit already reads, on each
//! pass (`follow`): the problem itself, Orbit's comment or issue about it
//! and the agent's answers there (`crate::ops`), and the outcome check of
//! a default branch staying fixed (`crate::outcomes`). An agent's word that
//! it fixed it counts until Orbit sees checks fail at that commit, or the
//! commit isn't on the branch (`Answer::check`). A step is to do, done,
//! skipped (no fix landed, as it stopped failing at the commit it failed
//! at, which is bad luck only when its case saw each job that failed pass
//! again there, or a base that moved on needed no merge; or Orbit couldn't
//! tell whether it stayed fixed, or what became of each alert), or missed
//! (it failed again within the days it should have stayed fixed, or Orbit
//! stopped following it before it passed). It runs before Orbit refreshes
//! its comments and issues, so the items failing there are this problem's.
//! Once the problem closes, its comment may move on to another problem, so
//! the steps it ticked on the pass it closed on, which still reads the
//! comment as it was, stay ticked (`Problem.playbook_closed`); while the
//! problem is open, or open again within the hour, each step says where it
//! stands now.
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
    security::Source,
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
    /// Skipped: not needed, as it stopped failing at the commit it failed
    /// at, or Orbit couldn't tell whether it stayed fixed or what became of
    /// each alert.
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

/// The steps of a problem's playbook, in order, and who does each. Where
/// Orbit tells an agent (`ops::tells_agent`), Orbit tells it, the agent
/// takes it, a fix lands and Orbit sees it done; a default branch starts
/// with what broke it and ends staying fixed. Where only a person can fix
/// it, or an issue would show others where a weakness is, it's you who
/// fixes it, and Orbit sees it done.
pub fn steps(p: &Problem) -> Vec<(StepKind, Who)> {
    use StepKind::*;
    use Who::*;
    let agent = [
        (Told, Orbit),
        (Taken, Agent),
        (Fixed, Agent),
        (Passes, Orbit),
    ];
    match p.kind {
        ProblemKind::DefaultBranch => [(Found, Orbit)]
            .into_iter()
            .chain(agent)
            .chain([(Holds, Orbit)])
            .collect(),
        _ if crate::ops::tells_agent(p) => agent.to_vec(),
        _ => vec![(Fixed, You), (Passes, Orbit)],
    }
}

/// Whether a problem closed with nothing left to do: its commit passed or
/// was fixed (`Problem::passed`), GitHub said what became of each of its
/// security alerts (`Problem::resolved`), or Orbit read every queued run
/// and none waits for a runner.
pub fn done(p: &Problem) -> bool {
    p.passed()
        || p.resolved()
        || (p.kind == ProblemKind::Waiting && !p.is_open() && p.waiting.is_empty())
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
fn text(p: &Problem, step: StepKind, who: Who) -> String {
    use ProblemKind as K;
    use StepKind::*;
    let plain = &str::to_string;
    let base = || plain(&clean(&p.base, 100));
    let on = || branch(p, plain);
    let secrets = p.source == Some(Source::SecretScanning);
    match (p.kind, step) {
        (_, Found) => "Orbit finds the commit that broke it.".into(),
        (K::PullRequest | K::BaseMoved, Told) => {
            "Orbit tells the agent in its comment on the pull request.".into()
        }
        (K::NoCi, Told) => "Orbit proposes a workflow to the agent in an issue.".into(),
        (_, Told) => "Orbit tells the agent in an issue.".into(),
        (_, Taken) => "The agent takes it.".into(),
        (K::DefaultBranch, Fixed) => format!("A fix lands on {}.", on()),
        (K::WorkflowRisks, Fixed) if who == Who::Agent => {
            format!("A fix lands on {} for each risky setting.", on())
        }
        (K::Versions, Fixed) => format!("A fix lands on {} for each file that disagrees.", on()),
        (K::ReleaseBuild, Fixed) => {
            "A fix lands on the default branch, for the next version.".into()
        }
        (K::BaseMoved, Fixed) => format!("The agent merges {} in.", base()),
        (K::NoCi, Fixed) => format!("A workflow that runs on pull requests lands on {}.", on()),
        (K::Security, Fixed) if who == Who::Agent => "A fix lands for each alert.".into(),
        (K::Security, Fixed) if secrets => {
            "You revoke each secret, then close its alert on GitHub.".into()
        }
        (K::Security, Fixed) => "You fix each alert, or dismiss it on GitHub.".into(),
        (K::WorkflowRisks, Fixed) => format!("You fix each risky setting on {}.", on()),
        (K::Waiting, Fixed) => "You bring a runner online that can take the waiting runs.".into(),
        (K::Tag, Fixed) => format!(
            "You move {} to a commit whose version files say its version.",
            plain(&clean(&p.tag, 100))
        ),
        (_, Fixed) => "A fix lands on the pull request.".into(),
        (K::DefaultBranch, Passes) => format!("Every check passes on {}.", on()),
        (K::ReleaseBuild, Passes) => "Every run of the release build passes.".into(),
        (K::BaseMoved, Passes) => format!("Every check passes again with {}'s latest.", base()),
        (K::NoCi, Passes) => format!(
            "Orbit sees a workflow on {} that runs on pull requests.",
            on()
        ),
        (K::Security, Passes) if secrets => "GitHub marks each alert resolved.".into(),
        (K::Security, Passes) => "GitHub marks each alert fixed or dismissed.".into(),
        (K::WorkflowRisks, Passes) => format!("Orbit finds no risky settings on {}.", on()),
        (K::Versions, Passes) => format!("The version files agree on {}.", on()),
        (K::Waiting, Passes) => "No runs wait for a runner.".into(),
        (K::Tag, Passes) => format!(
            "{} points at a commit whose version files say its version.",
            plain(&clean(&p.tag, 100))
        ),
        (_, Passes) => "Every check passes.".into(),
        (_, Holds) => format!("It stays fixed for {HOLDS_DAYS} days."),
    }
}

/// What done looks like, for the agent, as a numbered list and a blank
/// line, while the problem is open and Orbit tells an agent about it; or
/// nothing. It names no step Orbit does before the agent reads it, and it
/// doesn't change as steps are ticked, so Orbit's comment or issue is
/// edited only as what it is about changes.
pub fn done_text(p: &Problem, show: Show) -> String {
    use ProblemKind as K;
    use StepKind::*;
    if !p.is_open() || !crate::ops::tells_agent(p) {
        return String::new();
    }
    let base = || show(&clean(&p.base, 100));
    let on = || branch(p, show);
    let lines: Vec<String> = steps(p)
        .iter()
        .filter_map(|(step, _)| {
            Some(match (p.kind, step) {
                (_, Found | Told) => return None,
                (_, Taken) => "An agent takes each item.".into(),
                (K::DefaultBranch, Fixed) => format!("A fix lands on {}.", on()),
                (K::WorkflowRisks, Fixed) => {
                    format!("A fix lands on {} for each risky setting.", on())
                }
                (K::Versions, Fixed) => {
                    format!("A fix lands on {} for each file that disagrees.", on())
                }
                (K::ReleaseBuild, Fixed) => {
                    "A fix lands on the default branch, so the next version's tag builds with it."
                        .into()
                }
                (K::BaseMoved, Fixed) => format!("{} is merged in, and CI runs again.", base()),
                (K::NoCi, Fixed) => {
                    format!("A workflow that runs on pull requests lands on {}.", on())
                }
                (K::Security, Fixed) => "A fix lands for each alert.".into(),
                (_, Fixed) => "A fix lands on this branch.".into(),
                (K::DefaultBranch, Passes) => format!("Every check passes on {}.", on()),
                (K::ReleaseBuild, Passes) => format!(
                    "Every run of the release build for {} passes, such as after a rerun, or a newer version is tagged.",
                    show(&clean(&p.branch, 100))
                ),
                (K::BaseMoved, Passes) => format!(
                    "Every check passes, having begun after {}'s latest commit.",
                    base()
                ),
                (K::NoCi, Passes) => format!(
                    "Orbit reads a workflow on {} that runs on pull requests.",
                    on()
                ),
                (K::Security, Passes) => "GitHub marks each alert fixed or dismissed.".into(),
                (K::WorkflowRisks, Passes) => {
                    format!("Orbit finds no risky settings on {}.", on())
                }
                (K::Versions, Passes) => format!("The version files agree on {}.", on()),
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

/// Whether a problem's items are each their own thing, fixed on its own:
/// security alerts, risky workflow settings and version files. Its issue
/// is its own, so every item of it counts, and a fix is done once the
/// agent named one for each item still open.
fn each(p: &Problem) -> bool {
    matches!(
        p.kind,
        ProblemKind::Security | ProblemKind::WorkflowRisks | ProblemKind::Versions
    )
}

/// The items on a problem's comment or issue that are about it now: those
/// of its kind, and of a branch's checks, only those failing. Orbit marks
/// them as no longer failing once it sees them pass, after this has read
/// them.
fn items<'a>(p: &Problem, t: Option<&'a Thread>) -> Vec<&'a Item> {
    let kind = match p.kind {
        ProblemKind::BaseMoved => ItemKind::BaseMoved,
        ProblemKind::NoCi => ItemKind::NoCi,
        ProblemKind::Security => ItemKind::Alert,
        ProblemKind::WorkflowRisks => ItemKind::Risk,
        ProblemKind::Versions => ItemKind::Version,
        _ => ItemKind::Check,
    };
    t.map(|t| {
        t.items
            .iter()
            .filter(|i| i.kind == kind && (i.failing || each(p)))
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
    steps(p)
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
                // at that commit, or the commit isn't there. A branch's
                // checks are fixed with the first fix; items each their own
                // thing, once each one still open has one.
                Fixed => {
                    let fix = fix_of;
                    let fixes: Vec<&Answer> = items.iter().filter_map(|i| fix(i)).collect();
                    let left: Vec<&&Item> = items.iter().filter(|i| i.failing).collect();
                    let fixed_left = left.iter().filter(|i| fix(i).is_some()).count();
                    let found = if each(p) {
                        fixes
                            .iter()
                            .max_by(|a, b| a.at.cmp(&b.at))
                            .filter(|_| fixed_left == left.len())
                    } else {
                        fixes.iter().min_by(|a, b| a.at.cmp(&b.at))
                    };
                    match found {
                        Some(a) => mark(
                            Mark::Done,
                            Some(&a.at),
                            match (fix_commit(a), each(p) && fixes.len() > 1) {
                                (Some(c), false) => format!("The agent said it fixed it in {c}."),
                                (None, false) => "The agent said it fixed it.".into(),
                                (Some(c), true) => {
                                    format!("The agent said it fixed each one, the last in {c}.")
                                }
                                (None, true) => "The agent said it fixed each one.".into(),
                            },
                        ),
                        None if done(p) => fixed_anyway(p, case),
                        None => match answers()
                            .filter(|a| a.kind == AnswerKind::Fixed && refuted(a))
                            .max_by(|a, b| a.at.cmp(&b.at))
                        {
                            Some(a) => mark(Mark::Todo, None, not_fixed(p, a)),
                            None if fixed_left > 0 => mark(
                                Mark::Todo,
                                None,
                                format!(
                                    "The agent said it fixed {fixed_left} of the {} left.",
                                    left.len()
                                ),
                            ),
                            None => todo(),
                        },
                    }
                }
                Passes if done(p) => mark(
                    Mark::Done,
                    closed,
                    clean(p.closed_reason.as_deref().unwrap_or_default(), 200),
                ),
                // No alert is open, but GitHub didn't say what became of
                // each one.
                Passes if p.cleared() => mark(
                    Mark::Skipped,
                    closed,
                    format!(
                        "Orbit couldn't tell: {}",
                        clean(p.closed_reason.as_deref().unwrap_or_default(), 200)
                    ),
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
                text: text(p, step, who),
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

/// The fix step of a problem done with no fix an agent named, as it
/// closed. A pull request that passed at the commit it failed at needed
/// no fix only when its case saw each job that failed pass again in its
/// own run (`cases::passed_again`), and one whose base moved on needed no
/// merge, as its checks ran again with the base's latest.
fn fixed_anyway(p: &Problem, case: Option<&Case>) -> (Mark, Option<String>, String) {
    let closed = p.closed_at.as_deref();
    let at = short(&p.closed_sha);
    match p.kind {
        ProblemKind::BaseMoved if p.closed_sha == p.sha => mark(
            Mark::Skipped,
            closed,
            format!(
                "Its checks ran again with {}'s latest at the same commit, so no merge was needed.",
                clean(&p.base, 100)
            ),
        ),
        _ if crate::cases::tracked(p.kind) && p.closed_sha == p.sha => mark(
            Mark::Skipped,
            closed,
            if case.is_some_and(|c| c.end == End::Again) {
                "It passed when it ran again at the same commit, so no fix was needed."
            } else {
                "No fix landed: it stopped failing at the same commit, but Orbit didn't see what failed pass there."
            },
        ),
        ProblemKind::DefaultBranch
        | ProblemKind::PullRequest
        | ProblemKind::BaseMoved
        | ProblemKind::ReleaseBuild => mark(Mark::Done, closed, format!("It passed at {at}.")),
        ProblemKind::Tag => mark(Mark::Done, closed, format!("It points at {at} now.")),
        _ if p.passed() => mark(Mark::Done, closed, format!("It landed at {at}.")),
        _ => mark(Mark::Done, closed, ""),
    }
}

/// The fix an agent named for an item, unless Orbit checked it and it
/// didn't hold up.
fn fix_of(i: &Item) -> Option<&Answer> {
    i.answer
        .as_ref()
        .filter(|a| a.kind == AnswerKind::Fixed && !refuted(a))
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
        text: text(p, StepKind::Holds, who),
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
