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
/// What Orbit does on each pass, in order: reads CI, ticks the playbooks,
/// then brings its comments and issues up to date.
fn pass(s: &mut Snapshot, ci: Option<RepoCi>, minutes: i64) -> bool {
    if let Some(ci) = ci {
        observe(&mut s.problems, &ci, &[], at(minutes));
        s.ci = vec![ci];
    }
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
    pass(&mut s, Some(main_ci(B, CiState::Failing, &[RUST], 0)), 0);
    pass(&mut s, Some(main_ci(B, CiState::Passing, &[], 10)), 10);
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
        playbook(p, None, &[]).into_iter().map(|s| s.text).collect()
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
            "Every run of the release build passes.",
        ]
    );
    // A release build passes at its tag's commit: no fix was needed.
    let passed = Problem {
        closed_at: Some(stamp(at(5))),
        closed_sha: A.into(),
        closed_reason: Some("Passed at 1111111.".into()),
        ..build
    };
    let steps = playbook(&passed, None, &[]);
    assert_eq!(steps[2].mark, Mark::Skipped);
    assert_eq!(steps[3].mark, Mark::Done);
    // Kinds without a playbook yet get none.
    let mut s = Snapshot::default();
    s.problems.push(Problem {
        kind: ProblemKind::Waiting,
        ..Default::default()
    });
    assert!(!follow(&mut s));
    assert!(s.problems[0].playbook.is_empty());
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

/// A problem saved before playbooks still loads, and one saved with its
/// playbook keeps it.
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
