use super::*;
use crate::{
    broke::Broke,
    ci::{Check, CiState, CommitCi, PullCi, RepoCi},
    ops::{Answer, AnswerKind},
    problems::{observe, stamp},
};
use chrono::{DateTime, Duration, TimeZone, Utc};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";
const RUST: &str = "CI / Rust";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap()
}
fn at(minutes: i64) -> DateTime<Utc> {
    now() + Duration::minutes(minutes)
}
fn check(name: &str) -> Check {
    Check {
        name: name.into(),
        url: String::new(),
    }
}
/// `main` at `sha`, read at `minutes`.
fn main_ci(sha: &str, state: CiState, failing: &[&str], minutes: i64) -> RepoCi {
    RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: CommitCi {
            sha: sha.into(),
            state,
            failing: failing.iter().map(|n| check(n)).collect(),
            ..Default::default()
        },
        complete: true,
        checked_at: stamp(at(minutes)),
        ..Default::default()
    }
}
/// `main` at `sha` read with each check by name, as Orbit reads it:
/// `checks` say where each one stands.
fn named(sha: &str, checks: &[(&str, CiState)], minutes: i64) -> RepoCi {
    let failing: Vec<&str> = checks
        .iter()
        .filter(|(_, state)| *state == CiState::Failing)
        .map(|(n, _)| *n)
        .collect();
    let state = if failing.is_empty() {
        CiState::Passing
    } else {
        CiState::Failing
    };
    let mut ci = main_ci(sha, state, &failing, minutes);
    ci.head.named = true;
    ci.head.checks = checks.iter().map(|(n, st)| (check(n), *st)).collect();
    ci
}
/// What Orbit does on each pass, in order: reads CI, follows each case,
/// ticks the playbooks, then brings its comments and issues up to date.
fn pass(s: &mut Snapshot, ci: Option<RepoCi>, minutes: i64) -> bool {
    if let Some(ci) = ci {
        observe(&mut s.problems, &ci, &[], at(minutes));
        s.ci = vec![ci];
    }
    crate::cases::follow(s, at(minutes));
    let changed = follow(s);
    crate::ops::refresh(&mut s.ops, &s.problems, at(minutes));
    changed
}
fn marks(p: &Problem) -> Vec<(StepKind, Who, Mark)> {
    p.playbook.iter().map(|s| (s.step, s.who, s.mark)).collect()
}
fn step(p: &Problem, step: StepKind) -> Step {
    p.playbook
        .iter()
        .find(|s| s.step == step)
        .cloned()
        .expect("the step")
}
/// GitHub shows Orbit's issue or comment, with each failing item on it.
fn shown(s: &mut Snapshot, minutes: i64) {
    let t = &mut s.ops[0];
    t.number = Some(5);
    t.comment_id = Some(50);
    t.url = "https://github.com/a/b/issues/5".into();
    for i in t.items.iter_mut().filter(|i| i.failing) {
        i.heard_from = stamp(at(minutes));
    }
}
fn answer(s: &mut Snapshot, kind: AnswerKind, commit: Option<&str>, minutes: i64) {
    let i = &mut s.ops[0].items[0];
    i.answer = Some(Answer {
        kind,
        author: "agent-bot".into(),
        at: stamp(at(minutes)),
        commit: commit.map(str::to_string),
        ..Default::default()
    });
}

/// A broken default branch, from the first read to staying fixed, each
/// step ticked as it happens.
#[test]
fn a_broken_default_branch_ticks_each_step_through_to_staying_fixed() {
    use Mark::*;
    use StepKind::*;
    use Who::*;
    let mut s = Snapshot::default();
    assert!(pass(
        &mut s,
        Some(main_ci(B, CiState::Failing, &[RUST], 0)),
        0
    ));
    let p = &s.problems[0];
    assert_eq!(
        marks(p),
        [
            (Found, Orbit, Todo),
            (Told, Orbit, Todo),
            (Taken, Agent, Todo),
            (Fixed, Agent, Todo),
            (Passes, Orbit, Todo),
            (Holds, Orbit, Todo),
        ]
    );
    let texts: Vec<&str> = p.playbook.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Orbit finds the commit that broke it.",
            "Orbit tells the agent in an issue.",
            "The agent takes it.",
            "A fix lands on main.",
            "Every check passes on main.",
            "It stays fixed for 3 days.",
        ]
    );
    // Nothing new: nothing changes.
    assert!(!pass(&mut s, None, 1));
    // Orbit finds what broke it.
    s.problems[0].broke = Some(Broke {
        done: true,
        green: A.into(),
        red: B.into(),
        commits: vec![B.into()],
        pulls: vec![7],
        pull: Some(7),
        ..Default::default()
    });
    assert!(pass(&mut s, None, 2));
    let found = step(&s.problems[0], Found);
    assert_eq!(found.mark, Done);
    assert_eq!(
        found.note,
        "It started failing with #7, merged as 2222222; 1111111 before it passed."
    );
    // GitHub shows Orbit's issue.
    shown(&mut s, 3);
    pass(&mut s, None, 4);
    let told = step(&s.problems[0], Told);
    assert_eq!((told.mark, told.at), (Done, Some(stamp(at(3)))));
    // The agent takes it, then says it fixed it.
    answer(&mut s, AnswerKind::Taking, None, 5);
    pass(&mut s, None, 6);
    let taken = step(&s.problems[0], Taken);
    assert_eq!(
        (taken.mark, taken.note.as_str()),
        (Done, "agent-bot said they're on it.")
    );
    answer(&mut s, AnswerKind::Fixed, Some(C), 7);
    pass(&mut s, None, 8);
    let fixed = step(&s.problems[0], Fixed);
    assert_eq!(
        (fixed.mark, fixed.note.as_str()),
        (Done, "The agent said it fixed it in 3333333.")
    );
    // A fix answer counts as taking it, from when it was said.
    assert_eq!(step(&s.problems[0], Taken).at, Some(stamp(at(7))));
    assert_eq!(step(&s.problems[0], Passes).mark, Todo);
    // It passes at the fix.
    pass(&mut s, Some(main_ci(C, CiState::Passing, &[], 10)), 10);
    let p = &s.problems[0];
    assert!(p.passed());
    let passes = step(p, Passes);
    assert_eq!(
        (passes.mark, passes.at.clone(), passes.note.as_str()),
        (Done, Some(stamp(at(10))), "Passed at 3333333.")
    );
    // What the issue showed stays, though its items no longer fail.
    assert!(s.ops[0].items.iter().all(|i| !i.failing));
    pass(&mut s, None, 11);
    let p = &s.problems[0];
    assert_eq!(
        marks(p)[..5].iter().map(|(_, _, m)| *m).collect::<Vec<_>>(),
        [Done; 5]
    );
    assert_eq!(step(p, Taken).at, Some(stamp(at(7))));
    // Orbit checks it stays fixed for 3 days.
    assert_eq!(step(p, Holds).mark, Todo);
    crate::outcomes::record(&mut s, at(11));
    pass(&mut s, None, 12);
    let holds = step(&s.problems[0], Holds);
    assert_eq!(
        (holds.mark, holds.due),
        (Todo, Some(stamp(at(10) + Duration::days(3))))
    );
    // Three days later, still passing.
    let later = 10 + 3 * 24 * 60 + 5;
    s.ci = vec![main_ci(C, CiState::Passing, &[], later)];
    assert_eq!(crate::outcomes::settle(&mut s, at(later)).len(), 1);
    pass(&mut s, None, later);
    let holds = step(&s.problems[0], Holds);
    assert_eq!((holds.mark, holds.due), (Done, None));
    assert_eq!(holds.at, Some(stamp(at(later))));
    assert!(s.problems[0].playbook.iter().all(|s| s.mark == Done));
    assert!(!pass(&mut s, None, later + 1));
}

/// A default branch that fails again within its days to stay fixed misses
/// that step, and one Orbit couldn't tell about doesn't count either way.
#[test]
fn staying_fixed_can_miss_or_be_unclear() {
    let mut s = Snapshot::default();
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 0)), 0);
    pass(&mut s, Some(main_ci(C, CiState::Passing, &[], 10)), 10);
    crate::outcomes::record(&mut s, at(11));
    let holds = |s: &Snapshot| step(&s.problems[0], StepKind::Holds);
    for (verdict, why, mark, note) in [
        (
            Verdict::Missed,
            "It failed again 2 days after it passed.",
            Mark::Missed,
            "It failed again 2 days after it passed.",
        ),
        (
            Verdict::Unclear,
            "Orbit doesn't keep that problem any more.",
            Mark::Skipped,
            "Orbit couldn't tell: Orbit doesn't keep that problem any more.",
        ),
        (Verdict::Held, "", Mark::Done, "It stayed fixed for 3 days."),
    ] {
        let o = &mut s.outcomes[0];
        o.verdict = verdict;
        o.why = why.into();
        o.settled_at = Some(stamp(at(60)));
        pass(&mut s, None, 61);
        let h = holds(&s);
        assert_eq!((h.mark, h.note.as_str()), (mark, note));
        assert_eq!(h.at, Some(stamp(at(60))));
    }
}

/// A check that passes when it runs again at the same commit needed no
/// fix, and a problem that comes back within the hour is to do again.
#[test]
fn passing_at_the_same_commit_needs_no_fix_and_coming_back_unticks_it() {
    use Mark::*;
    use StepKind::*;
    let mut s = Snapshot::default();
    pass(&mut s, Some(named(B, &[(RUST, CiState::Failing)], 0)), 0);
    pass(
        &mut s,
        Some(named(
            B,
            &[(RUST, CiState::Passing), ("Lint", CiState::Passing)],
            10,
        )),
        10,
    );
    let p = &s.problems[0];
    let fixed = step(p, Fixed);
    assert_eq!(
        (fixed.mark, fixed.at.clone(), fixed.note.as_str()),
        (
            Skipped,
            Some(stamp(at(10))),
            "It passed when it ran again at the same commit, so no fix was needed."
        )
    );
    assert_eq!(step(p, Passes).mark, Done);
    // It fails again within the hour: the same problem, back to do.
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 30)), 30);
    let p = &s.problems[0];
    assert!(p.is_open());
    assert_eq!(s.problems.len(), 1);
    for k in [Fixed, Passes, Holds] {
        assert_eq!(step(p, k).mark, Todo, "{k:?}");
    }
}

/// Stopping failing at the same commit is bad luck only when Orbit saw
/// what failed pass there: a failed job cancelled or skipped beside one
/// that passed isn't.
#[test]
fn stopping_failing_at_the_same_commit_without_passing_isnt_bad_luck() {
    let mut s = Snapshot::default();
    pass(&mut s, Some(named(B, &[(RUST, CiState::Failing)], 0)), 0);
    pass(
        &mut s,
        Some(named(
            B,
            &[(RUST, CiState::None), ("Lint", CiState::Passing)],
            10,
        )),
        10,
    );
    let p = &s.problems[0];
    assert!(p.passed());
    assert_eq!(s.cases[0].end, crate::cases::End::Unclear);
    let fixed = step(p, StepKind::Fixed);
    assert_eq!(
        (fixed.mark, fixed.note.as_str()),
        (
            Mark::Skipped,
            "No fix landed: it stopped failing at the same commit, but Orbit didn't see what failed pass there."
        )
    );
}

/// The pass a problem closes on reads its comment or issue as it was,
/// with what Orbit found when it checked the agent's fix since the last
/// pass; only later passes keep what that pass ticked.
#[test]
fn a_fix_found_failing_just_before_it_closed_does_not_count() {
    use crate::ops::FixCheck;
    let mut s = Snapshot::default();
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 0)), 0);
    shown(&mut s, 1);
    answer(&mut s, AnswerKind::Fixed, Some(C), 2);
    s.ops[0].items[0].answer.as_mut().unwrap().check = Some(FixCheck::Waiting);
    pass(&mut s, None, 3);
    assert_eq!(step(&s.problems[0], StepKind::Fixed).mark, Mark::Done);
    // Orbit checks the fix after that pass: it fails there. Then Orbit
    // stops following the problem.
    s.ops[0].items[0].answer.as_mut().unwrap().check = Some(FixCheck::Fails);
    s.problems[0].closed_at = Some(stamp(at(4)));
    s.problems[0].closed_reason = Some("Watch CI was turned off.".into());
    pass(&mut s, None, 4);
    let p = &s.problems[0];
    assert!(p.playbook_closed);
    let fixed = step(p, StepKind::Fixed);
    let c = &C[..7];
    let note = format!("The agent said it fixed it in {c}, but checks still fail there.");
    assert_eq!(
        (fixed.mark, fixed.note.as_str()),
        (Mark::Todo, note.as_str())
    );
    // Its items are no longer failing, so later passes keep what it ticked.
    assert_eq!(step(p, StepKind::Told).mark, Mark::Done);
    pass(&mut s, None, 5);
    let p = &s.problems[0];
    assert_eq!(step(p, StepKind::Told).mark, Mark::Done);
    assert_eq!(step(p, StepKind::Fixed).note, note);
}

/// While the problem is open, each step says where it stands now: an
/// answer that changes changes it. An agent that declines or asks for a
/// person leaves it to do.
#[test]
fn the_agents_latest_answer_counts_while_it_is_open() {
    let mut s = Snapshot::default();
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 0)), 0);
    shown(&mut s, 1);
    let taken = |s: &Snapshot| {
        let t = step(&s.problems[0], StepKind::Taken);
        (t.mark, t.note)
    };
    answer(&mut s, AnswerKind::Taking, None, 2);
    pass(&mut s, None, 3);
    assert_eq!(taken(&s).0, Mark::Done);
    answer(&mut s, AnswerKind::Declined, None, 4);
    pass(&mut s, None, 5);
    assert_eq!(taken(&s), (Mark::Todo, "The agent declined it.".into()));
    answer(&mut s, AnswerKind::Question, None, 6);
    pass(&mut s, None, 7);
    assert_eq!(
        taken(&s),
        (Mark::Todo, "The agent asked for a person.".into())
    );
    // A fix with no commit, or not a commit.
    answer(&mut s, AnswerKind::Fixed, Some("not-a-commit"), 8);
    pass(&mut s, None, 9);
    assert_eq!(
        step(&s.problems[0], StepKind::Fixed).note,
        "The agent said it fixed it."
    );
}

/// An agent's fix counts until Orbit sees it fail, or finds the commit
/// isn't there.
#[test]
fn a_fix_orbit_saw_fail_does_not_count() {
    use crate::ops::FixCheck;
    let mut s = Snapshot::default();
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 0)), 0);
    shown(&mut s, 1);
    answer(&mut s, AnswerKind::Fixed, Some(C), 2);
    let fixed = |s: &mut Snapshot, check: Option<FixCheck>, minutes: i64| {
        s.ops[0].items[0].answer.as_mut().unwrap().check = check;
        pass(s, None, minutes);
        let f = step(&s.problems[0], StepKind::Fixed);
        (f.mark, f.note)
    };
    let c = &C[..7];
    for (check, mark, note) in [
        (
            None,
            Mark::Done,
            format!("The agent said it fixed it in {c}."),
        ),
        (
            Some(FixCheck::Waiting),
            Mark::Done,
            format!("The agent said it fixed it in {c}."),
        ),
        (
            Some(FixCheck::Fails),
            Mark::Todo,
            format!("The agent said it fixed it in {c}, but checks still fail there."),
        ),
        (
            Some(FixCheck::NotOnBranch),
            Mark::Todo,
            format!("The agent said it fixed it in {c}, but that commit isn't on main."),
        ),
        (
            Some(FixCheck::NotFound),
            Mark::Todo,
            format!("The agent said it fixed it in {c}, but GitHub doesn't know that commit."),
        ),
        (
            Some(FixCheck::Passes),
            Mark::Done,
            format!("The agent said it fixed it in {c}."),
        ),
    ] {
        assert_eq!(fixed(&mut s, check, 3), (mark, note), "{check:?}");
    }
    // Taking it still counts when the fix didn't hold up.
    fixed(&mut s, Some(FixCheck::Fails), 4);
    assert_eq!(step(&s.problems[0], StepKind::Taken).mark, Mark::Done);
}

fn pull(sha: &str, state: CiState, failing: &[&str]) -> RepoCi {
    let mut ci = main_ci(A, CiState::Passing, &[], 0);
    ci.pulls = vec![PullCi {
        number: 12,
        draft: false,
        ci: CommitCi {
            sha: sha.into(),
            state,
            failing: failing.iter().map(|n| check(n)).collect(),
            ..Default::default()
        },
    }];
    ci
}
fn followed() -> crate::types::PullRequest {
    let mut p = crate::types::tests::pr();
    p.number = 12;
    p.id = "a/b#12".into();
    p.repo = "a/b".into();
    p.head_sha = B.into();
    p.head_ref = "fix-it".into();
    p
}

/// A pull request's playbook, and one Orbit stops following before it
/// passes, which misses passing and says why.
#[test]
fn a_pull_request_that_closes_before_it_passes_says_orbit_stopped() {
    use Mark::*;
    use StepKind::*;
    let pr = followed();
    let mut s = Snapshot::default();
    let read = |s: &mut Snapshot, ci: RepoCi, prs: &[&crate::types::PullRequest], m: i64| {
        observe(&mut s.problems, &ci, prs, at(m));
        s.ci = vec![ci];
        follow(s);
        crate::ops::refresh(&mut s.ops, &s.problems, at(m));
    };
    read(&mut s, pull(B, CiState::Failing, &[RUST]), &[&pr], 0);
    let p = &s.problems[0];
    assert_eq!(p.kind, ProblemKind::PullRequest);
    let texts: Vec<&str> = p.playbook.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Orbit tells the agent in its comment on the pull request.",
            "The agent takes it.",
            "A fix lands on the pull request.",
            "Every check passes.",
        ]
    );
    shown(&mut s, 1);
    answer(&mut s, AnswerKind::Taking, None, 2);
    read(&mut s, pull(B, CiState::Failing, &[RUST]), &[&pr], 3);
    // The pull request closes.
    let mut gone = pull(B, CiState::Failing, &[RUST]);
    gone.pulls.clear();
    read(&mut s, gone.clone(), &[], 4);
    let p = &s.problems[0];
    assert!(!p.is_open() && !p.passed());
    assert_eq!(
        marks(p).iter().map(|(_, _, m)| *m).collect::<Vec<_>>(),
        [Done, Done, Todo, Missed]
    );
    assert_eq!(
        step(p, Passes).note,
        "Orbit stopped following it. The pull request closed."
    );
    // What the comment showed stays, though its items moved on.
    read(&mut s, gone, &[], 5);
    assert_eq!(step(&s.problems[0], Taken).mark, Done);
}

/// The agent hears what done looks like while the problem is open, in
/// the same words each time, and nothing once it closed. It names no step
/// Orbit does before the agent reads it.
#[test]
fn the_agent_hears_what_done_looks_like() {
    let code = |s: &str| format!("`{s}`");
    let problem = |kind: ProblemKind| Problem {
        kind,
        repo: "a/b".into(),
        branch: if kind == ProblemKind::ReleaseBuild {
            "v1.2.0".into()
        } else {
            "main".into()
        },
        base: "main".into(),
        opened_at: stamp(at(0)),
        ..Default::default()
    };
    assert_eq!(
        done_text(&problem(ProblemKind::DefaultBranch), &code),
        "What done looks like:\n\n1. An agent takes each item.\n2. A fix lands on `main`.\n3. Every check passes on `main`.\n4. It stays fixed for 3 days.\n\n"
    );
    assert_eq!(
        done_text(&problem(ProblemKind::PullRequest), &code),
        "What done looks like:\n\n1. An agent takes each item.\n2. A fix lands on this branch.\n3. Every check passes.\n\n"
    );
    assert_eq!(
        done_text(&problem(ProblemKind::BaseMoved), &code),
        "What done looks like:\n\n1. An agent takes each item.\n2. `main` is merged in, and CI runs again.\n3. Every check passes, having begun after `main`'s latest commit.\n\n"
    );
    assert_eq!(
        done_text(&problem(ProblemKind::ReleaseBuild), &code),
        "What done looks like:\n\n1. An agent takes each item.\n2. A fix lands on the default branch, so the next version's tag builds with it.\n3. Every run of the release build for `v1.2.0` passes, such as after a rerun, or a newer version is tagged.\n\n"
    );
    // Not as checkboxes, which anyone could tick, editing Orbit's text.
    assert!(!done_text(&problem(ProblemKind::DefaultBranch), &code).contains("[ ]"));
    let closed = Problem {
        closed_at: Some(stamp(at(5))),
        ..problem(ProblemKind::DefaultBranch)
    };
    assert_eq!(done_text(&closed, &code), "");
    assert_eq!(done_text(&problem(ProblemKind::Waiting), &code), "");
    // Its issue says it, between what broke it and the items.
    let mut s = Snapshot::default();
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 0)), 0);
    let body = crate::ops::issue_body(&s.ops[0], &s.problems[0], false);
    let done = body.find("What done looks like:").expect("said");
    assert!(done < body.find("**I1**").expect("the item"));
    // It doesn't change as steps are ticked.
    let said = done_text(&s.problems[0], &code);
    assert!(body.contains(&said));
    shown(&mut s, 1);
    answer(&mut s, AnswerKind::Taking, None, 2);
    pass(&mut s, None, 3);
    assert_eq!(step(&s.problems[0], StepKind::Taken).mark, Mark::Done);
    assert_eq!(done_text(&s.problems[0], &code), said);
}

/// The steps of a base that moved on and of a release build, and Bit's
/// summary of what's left.
#[test]
fn other_kinds_have_their_own_steps_and_bit_hears_whats_left() {
    let texts = |p: &Problem| -> Vec<String> {
        playbook(p, None, None, &[])
            .into_iter()
            .map(|s| s.text)
            .collect()
    };
    let moved = Problem {
        kind: ProblemKind::BaseMoved,
        base: "main".into(),
        ..Default::default()
    };
    assert_eq!(
        texts(&moved),
        [
            "Orbit tells the agent in its comment on the pull request.",
            "The agent takes it.",
            "The agent merges main in.",
            "Every check passes again with main's latest.",
        ]
    );
    let build = Problem {
        kind: ProblemKind::ReleaseBuild,
        branch: "v1.2.0".into(),
        sha: A.into(),
        ..Default::default()
    };
    assert_eq!(
        texts(&build),
        [
            "Orbit tells the agent in an issue.",
            "The agent takes it.",
            "A fix lands on the default branch, for the next version.",
            "Every run of the release build passes, or a newer version is tagged.",
        ]
    );
    // A newer version took its place: its own build shows whether the
    // fix is in.
    let newer = Problem {
        closed_at: Some(stamp(at(5))),
        closed_reason: Some("v1.3.0 is the newest version tag now.".into()),
        superseded_by: "v1.3.0".into(),
        ..build.clone()
    };
    assert_eq!(
        ends(&newer),
        [
            (
                StepKind::Fixed,
                Mark::Skipped,
                "Orbit couldn't tell: v1.3.0 was tagged since, and its own build shows whether the fix is in.".into()
            ),
            (
                StepKind::Passes,
                Mark::Done,
                "v1.3.0 is the newest version tag now.".into()
            ),
        ]
    );
    // A release build passes at its tag's commit: no fix was needed.
    let passed = Problem {
        closed_at: Some(stamp(at(5))),
        closed_sha: A.into(),
        closed_reason: Some("Passed at 1111111.".into()),
        ..build
    };
    let steps = playbook(&passed, None, None, &[]);
    assert_eq!(steps[2].mark, Mark::Skipped);
    assert_eq!(steps[3].mark, Mark::Done);
    // Bit hears each step, who does it and where it stands.
    let p = Problem {
        playbook: steps,
        ..passed
    };
    let summary = summary(&p);
    assert_eq!(summary.len(), 4);
    assert_eq!(summary[0]["who"], "orbit");
    assert_eq!(summary[0]["state"], "todo");
    assert_eq!(summary[2]["who"], "agent");
    assert_eq!(summary[2]["state"], "skipped");
    assert_eq!(summary[3]["state"], "done");
    assert_eq!(summary[3]["note"], "Passed at 1111111.");
}

fn texts(p: &Problem) -> Vec<(StepKind, Who, String)> {
    steps(p)
        .into_iter()
        .map(|(step, who)| (step, who, text(p, step, who)))
        .collect()
}
fn closed(p: &Problem, sha: &str, reason: &str) -> Problem {
    Problem {
        closed_at: Some(stamp(at(5))),
        closed_sha: sha.into(),
        closed_reason: Some(reason.into()),
        ..p.clone()
    }
}
fn ends(p: &Problem) -> Vec<(StepKind, Mark, String)> {
    playbook(p, None, None, &[])
        .into_iter()
        .filter(|s| matches!(s.step, StepKind::Fixed | StepKind::Passes))
        .map(|s| (s.step, s.mark, s.note))
        .collect()
}

/// Where Orbit tells an agent, it tells it, the agent takes it, a fix
/// lands and Orbit sees it done, and the agent hears what done looks like.
#[test]
fn kinds_orbit_tells_an_agent_about_go_from_telling_to_done() {
    use crate::security::Source;
    use Mark::*;
    use StepKind::*;
    use Who::*;
    let code = |s: &str| format!("`{s}`");
    let on_main = |kind| Problem {
        kind,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: A.into(),
        private: true,
        ..Default::default()
    };
    let told = (
        Told,
        Orbit,
        "Orbit tells the agent in an issue.".to_string(),
    );
    let taken = (Taken, Agent, "The agent takes it.".to_string());
    let no_ci = on_main(ProblemKind::NoCi);
    assert_eq!(
        texts(&no_ci),
        [
            (
                Told,
                Orbit,
                "Orbit proposes a workflow to the agent in an issue.".into()
            ),
            taken.clone(),
            (
                Fixed,
                Agent,
                "A workflow that runs on pull requests lands on main.".into()
            ),
            (
                Passes,
                Orbit,
                "Orbit sees a workflow on main that runs on pull requests.".into()
            ),
        ]
    );
    let alerts = Problem {
        source: Some(Source::Dependabot),
        ..on_main(ProblemKind::Security)
    };
    assert_eq!(
        texts(&alerts),
        [
            told.clone(),
            taken.clone(),
            (Fixed, Agent, "A fix lands for each alert.".into()),
            (
                Passes,
                Orbit,
                "GitHub marks each alert fixed or dismissed.".into()
            ),
        ]
    );
    let risks = on_main(ProblemKind::WorkflowRisks);
    assert_eq!(
        texts(&risks)[2..],
        [
            (
                Fixed,
                Agent,
                "A fix lands on main for each risky setting.".into()
            ),
            (
                Passes,
                Orbit,
                "Orbit finds no risky settings on main.".into()
            ),
        ]
    );
    let versions = Problem {
        private: false,
        ..on_main(ProblemKind::Versions)
    };
    assert_eq!(
        texts(&versions),
        [
            told,
            taken,
            (
                Fixed,
                Agent,
                "A fix lands on main for each file that disagrees.".into()
            ),
            (Passes, Orbit, "The version files agree on main.".into()),
        ]
    );
    // What done looks like, for the agent.
    for (p, fixed, done) in [
        (
            &no_ci,
            "A workflow that runs on pull requests lands on `main`.",
            "Orbit reads a workflow on `main` that runs on pull requests.",
        ),
        (
            &alerts,
            "A fix lands for each alert.",
            "GitHub marks each alert fixed or dismissed.",
        ),
        (
            &risks,
            "A fix lands on `main` for each risky setting.",
            "Orbit finds no risky settings on `main`.",
        ),
        (
            &versions,
            "A fix lands on `main` for each file that disagrees.",
            "The version files agree on `main`.",
        ),
    ] {
        assert_eq!(
            done_text(p, &code),
            format!(
                "What done looks like:\n\n1. An agent takes each item.\n2. {fixed}\n3. {done}\n\n"
            ),
            "{:?}",
            p.kind
        );
    }
    // Done once Orbit sees it, at the commit it landed at.
    for p in [&no_ci, &risks, &versions] {
        assert_eq!(
            ends(&closed(p, B, "Done at 2222222.")),
            [
                (Fixed, Done, "It landed at 2222222.".into()),
                (Passes, Done, "Done at 2222222.".into()),
            ]
        );
    }
    // Every alert fixed or dismissed, GitHub says; or none open, but
    // GitHub didn't say what became of some.
    let gone = closed(&alerts, "", "GitHub says each one is fixed.");
    assert_eq!(
        ends(&gone),
        [
            (Fixed, Done, String::new()),
            (Passes, Done, "GitHub says each one is fixed.".into()),
        ]
    );
    let vanished = Problem {
        vanished: vec![3],
        ..closed(&alerts, "", "GitHub no longer has alert 3.")
    };
    assert_eq!(
        ends(&vanished),
        [
            (Fixed, Todo, String::new()),
            (
                Passes,
                Skipped,
                "Orbit couldn't tell: GitHub no longer has alert 3.".into()
            ),
        ]
    );
    // Stopped following it.
    assert_eq!(
        ends(&closed(&versions, "", "Watch CI was turned off."))[1],
        (
            Passes,
            Missed,
            "Orbit stopped following it. Watch CI was turned off.".into()
        )
    );
}

/// Alerts, risky settings and version files are each their own thing: a
/// fix is done once the agent named one for each still open, and what it
/// said about one already gone still counts. A branch's checks are fixed
/// with the first fix.
#[test]
fn alerts_settings_and_files_need_a_fix_for_each_one() {
    use crate::ops::{ItemKind, Thread};
    use Mark::*;
    use StepKind::*;
    let fixed = |minutes, commit: &str| Answer {
        kind: AnswerKind::Fixed,
        author: "agent-bot".into(),
        at: stamp(at(minutes)),
        commit: Some(commit.into()),
        ..Default::default()
    };
    let item = |number, failing, answer: Option<Answer>| Item {
        number,
        kind: ItemKind::Risk,
        risk: number,
        check: format!("risk {number}"),
        failing,
        heard_from: stamp(at(30)),
        answer,
        ..Default::default()
    };
    let p = Problem {
        id: "risks".into(),
        kind: ProblemKind::WorkflowRisks,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: A.into(),
        private: true,
        ..Default::default()
    };
    let thread = |items| Thread {
        key: "issue:risks".into(),
        repo: "a/b".into(),
        problem_id: "risks".into(),
        items,
        ..Default::default()
    };
    let at_step = |t: &Thread, step| {
        let s = playbook(&p, Some(t), None, &[])
            .into_iter()
            .find(|s| s.step == step)
            .expect("the step");
        (s.mark, s.note)
    };
    // One of two fixed: not yet.
    let t = thread(vec![item(1, true, Some(fixed(20, B))), item(2, true, None)]);
    assert_eq!(
        at_step(&t, Fixed),
        (Todo, "The agent said it fixed 1 of the 2 left.".into())
    );
    assert_eq!(at_step(&t, Taken).0, Done);
    // The one it fixed is gone, and the one left has a fix too: done, at
    // the last one. Taking it still counts, though that item is gone.
    let t = thread(vec![
        item(1, false, Some(fixed(10, B))),
        item(2, true, Some(fixed(20, C))),
    ]);
    assert_eq!(
        at_step(&t, Fixed),
        (
            Done,
            "The agent said it fixed each one, the last in 3333333.".into()
        )
    );
    assert_eq!(at_step(&t, Told).0, Done);
    assert_eq!(at_step(&t, Taken).0, Done);
    // A gone item says nothing about one still open.
    let t = thread(vec![
        item(1, false, Some(fixed(20, B))),
        item(2, true, None),
    ]);
    assert_eq!(at_step(&t, Fixed), (Todo, String::new()));
}

/// Where only a person can fix it, or an issue would show others where a
/// weakness is, it's you who fixes it, and Orbit sees it done. The agent
/// hears nothing.
#[test]
fn kinds_only_you_can_fix_go_from_your_fix_to_done() {
    use crate::{runners::Waiting, security::Source};
    use Mark::*;
    use StepKind::*;
    use Who::*;
    let code = |s: &str| format!("`{s}`");
    let p = |kind| Problem {
        kind,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: A.into(),
        ..Default::default()
    };
    let waiting = Problem {
        waiting: vec![Waiting::default()],
        ..p(ProblemKind::Waiting)
    };
    let tag = Problem {
        tag: "v0.3.0".into(),
        ..p(ProblemKind::Tag)
    };
    let secrets = Problem {
        source: Some(Source::SecretScanning),
        private: true,
        ..p(ProblemKind::Security)
    };
    let public = Problem {
        source: Some(Source::CodeScanning),
        ..p(ProblemKind::Security)
    };
    let risks = p(ProblemKind::WorkflowRisks);
    for (p, fixed, done) in [
        (
            &waiting,
            "You bring a runner online that can take the waiting runs.",
            "No runs wait for a runner.",
        ),
        (
            &tag,
            "If nothing was released from v0.3.0 yet, you move it to a commit whose version files say its version; if a release is out, you leave it and release the next version.",
            "v0.3.0 points at a commit whose version files say its version, or a newer version comes out on the right commit.",
        ),
        (
            &secrets,
            "You revoke each secret, then close its alert on GitHub.",
            "GitHub marks each alert resolved.",
        ),
        (
            &public,
            "You fix each alert, or dismiss it on GitHub.",
            "GitHub marks each alert fixed or dismissed.",
        ),
        (
            &risks,
            "You fix each risky setting on main.",
            "Orbit finds no risky settings on main.",
        ),
    ] {
        assert_eq!(
            texts(p),
            [(Fixed, You, fixed.into()), (Passes, Orbit, done.into())],
            "{:?}",
            p.kind
        );
        assert_eq!(done_text(p, &code), "", "{:?}", p.kind);
    }
    // No runs wait, as Orbit read every queued run; or Orbit stopped.
    let clear = Problem {
        waiting: vec![],
        ..closed(&waiting, "", "No runs wait for a runner now.")
    };
    assert_eq!(
        ends(&clear),
        [
            (Fixed, Done, String::new()),
            (Passes, Done, "No runs wait for a runner now.".into()),
        ]
    );
    assert_eq!(
        ends(&closed(&waiting, "", "Watch CI was turned off.")),
        [
            (Fixed, Todo, String::new()),
            (
                Passes,
                Missed,
                "Orbit stopped following it. Watch CI was turned off.".into()
            ),
        ]
    );
    // A tag nothing was released from moved to the right commit; one a
    // release came out from stayed, and the next version came out on the
    // right commit. Or it's gone.
    let reason = "v0.3.0 now points at 2222222, whose version files say its version.";
    assert_eq!(
        ends(&closed(&tag, B, reason)),
        [
            (Fixed, Done, "It points at 2222222 now.".into()),
            (Passes, Done, reason.into()),
        ]
    );
    let reason = "v0.4.0 came out on the right commit since, so Orbit stops raising v0.3.0.";
    let released = Problem {
        superseded_by: "v0.4.0".into(),
        ..closed(&tag, "", reason)
    };
    assert_eq!(
        ends(&released),
        [
            (Fixed, Done, "v0.4.0 came out on the right commit.".into()),
            (Passes, Done, reason.into()),
        ]
    );
    assert_eq!(
        ends(&closed(&tag, "", "The tag v0.3.0 is gone."))[1],
        (
            Passes,
            Missed,
            "Orbit stopped following it. The tag v0.3.0 is gone.".into()
        )
    );
}

/// A pull request whose checks ran again with its base's latest at the
/// same commit needed no merge.
#[test]
fn checks_that_ran_again_with_the_base_need_no_merge() {
    let moved = Problem {
        kind: ProblemKind::BaseMoved,
        base: "main".into(),
        sha: A.into(),
        ..Default::default()
    };
    let fixed = |p: &Problem| {
        let s = playbook(p, None, None, &[]).swap_remove(2);
        (s.mark, s.note)
    };
    assert_eq!(
        fixed(&closed(&moved, A, "Passed at 1111111 with main's latest.")),
        (
            Mark::Skipped,
            "Its checks ran again with main's latest at the same commit, so no merge was needed."
                .into()
        )
    );
    assert_eq!(
        fixed(&closed(&moved, B, "Passed at 2222222 with main's latest.")),
        (Mark::Done, "It passed at 2222222.".into())
    );
}

/// A problem saved before playbooks still loads, and one saved with its
/// playbook keeps it.
/// What an older Orbit saved, from before it said which newer version
/// took a problem's place or cleared the runs once none waited, reads as
/// done once it's upgraded; what didn't end that way stays as it was.
#[test]
fn problems_an_older_orbit_closed_read_as_done() {
    let saved = r#"[
        {"id":"w1","key":"wait:a/b","kind":"waiting","repo":"a/b",
         "waiting":[{"id":1,"name":"CI"}],"closedAt":"2026-11-03T10:00:00.000Z",
         "closedReason":"No runs wait for a runner now."},
        {"id":"w2","key":"wait:a/c","kind":"waiting","repo":"a/c",
         "waiting":[{"id":2,"name":"CI"}],"closedAt":"2026-11-03T10:00:00.000Z",
         "closedReason":"The repository was disconnected."},
        {"id":"b1","key":"build:a/b:v1.0.0","kind":"release_build","repo":"a/b",
         "branch":"v1.0.0","sha":"1111111111111111111111111111111111111111",
         "closedAt":"2026-11-03T10:00:00.000Z",
         "closedReason":"v1.1.0 is the newest version tag now."},
        {"id":"t1","key":"tag:a/b:v0.3.0","kind":"tag","repo":"a/b","tag":"v0.3.0",
         "closedAt":"2026-11-03T10:00:00.000Z",
         "closedReason":"v0.4.0 came out on the right commit since, so Orbit stops raising v0.3.0."},
        {"id":"t2","key":"tag:a/b:v0.5.0","kind":"tag","repo":"a/b","tag":"v0.5.0",
         "closedAt":"2026-11-03T10:00:00.000Z",
         "closedReason":"The tag v0.5.0 is gone."}
    ]"#;
    let mut s = Snapshot {
        problems: serde_json::from_str(saved).unwrap(),
        ..Default::default()
    };
    let passes = |s: &Snapshot| -> Vec<Mark> {
        s.problems
            .iter()
            .map(|p| step(p, StepKind::Passes).mark)
            .collect()
    };
    follow(&mut s);
    assert_eq!(passes(&s), [Mark::Missed; 5]);
    // Loading it upgrades it.
    crate::engine::recover(&mut s).unwrap();
    follow(&mut s);
    assert_eq!(
        passes(&s),
        [
            Mark::Done,
            Mark::Missed,
            Mark::Done,
            Mark::Done,
            Mark::Missed
        ]
    );
    assert!(s.problems[0].waiting.is_empty());
    assert_eq!(s.problems[1].waiting.len(), 1);
    assert_eq!(s.problems[2].superseded_by, "v1.1.0");
    assert_eq!(s.problems[3].superseded_by, "v0.4.0");
    assert!(s.problems[4].superseded_by.is_empty());
    // Once is enough.
    assert!(!crate::problems::upgrade(&mut s.problems));
}

#[test]
fn state_from_before_playbooks_still_loads() {
    let p: Problem = serde_json::from_str(r#"{"id":"p1","key":"ci:a/b"}"#).unwrap();
    assert!(p.playbook.is_empty());
    let p = Problem {
        playbook: vec![Step {
            step: StepKind::Holds,
            mark: Mark::Missed,
            note: "It failed again.".into(),
            ..Default::default()
        }],
        ..p
    };
    let back: Problem = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back, p);
}
