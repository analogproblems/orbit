use super::*;
use crate::{
    ci::{Check, CiState, CommitCi, RepoCi},
    logs::Failure,
    ops::{Answer, Item},
    outcomes::Outcome,
    problems::observe,
    reruns::Rerun,
};
use chrono::TimeZone;

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";
const RUST: &str = "CI / Rust";

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap()
}
fn at(minutes: i64) -> DateTime<Utc> {
    now() + Duration::minutes(minutes)
}
fn days_ago(days: i64) -> String {
    stamp(now() - Duration::days(days))
}
fn check(name: &str) -> Check {
    Check {
        name: name.into(),
        url: String::new(),
    }
}
fn main_ci(sha: &str, state: CiState, failing: &[&str]) -> RepoCi {
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
        ..Default::default()
    }
}
/// What Orbit does on each pass: reads CI, then follows the problems.
fn see(s: &mut Snapshot, ci: RepoCi, minutes: i64) -> (bool, Vec<Lesson>) {
    observe(&mut s.problems, &ci, &[], at(minutes));
    follow(s, at(minutes))
}
/// An open problem about `main` failing `RUST` at `sha`.
fn problem(id: &str, sha: &str) -> Problem {
    Problem {
        id: id.into(),
        key: "ci:a/b".into(),
        kind: ProblemKind::DefaultBranch,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: sha.into(),
        since_sha: sha.into(),
        failing: vec![check(RUST)],
        opened_at: stamp(at(0)),
        ..Default::default()
    }
}
fn close(p: &mut Problem, minutes: i64, passed_at: &str, reason: &str) {
    p.closed_at = Some(stamp(at(minutes)));
    p.closed_sha = passed_at.into();
    p.closed_reason = Some(reason.into());
}
/// The problem's CI thread, with its one item failing at `sha`.
fn thread(problem: &str, sha: &str) -> Thread {
    Thread {
        key: "ci:a/b".into(),
        repo: "a/b".into(),
        problem_id: problem.into(),
        items: vec![Item {
            number: 1,
            check: RUST.into(),
            failing: true,
            since: sha.into(),
            at: sha.into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}
fn last_time(s: &Snapshot, problem: usize) -> String {
    s.problems[problem]
        .last_time
        .as_ref()
        .map(|l| l.text.clone())
        .unwrap_or_default()
}
/// A case on `main` that closed `days` ago, ending as `end`.
fn case(id: &str, end: End, days: i64) -> Case {
    let closed = now() - Duration::days(days);
    Case {
        id: id.into(),
        key: "ci:a/b".into(),
        kind: ProblemKind::DefaultBranch,
        repo: "a/b".into(),
        branch: "main".into(),
        checks: vec![RUST.into()],
        last: vec![RUST.into()],
        sha: A.into(),
        opened_at: stamp(closed - Duration::hours(1)),
        closed_at: Some(stamp(closed)),
        end,
        passed: if end == End::Again { A } else { B }.into(),
        ..Default::default()
    }
}
fn missed(id: &str, kind: Kind, days: i64) -> Outcome {
    Outcome {
        id: id.into(),
        kind,
        repo: "a/b".into(),
        at: days_ago(days + 1),
        verdict: Verdict::Missed,
        settled_at: Some(days_ago(days)),
        ..Default::default()
    }
}
fn ids(lessons: &[Lesson]) -> Vec<&str> {
    lessons.iter().map(|l| l.id.as_str()).collect()
}
fn code(text: &str) -> String {
    format!("`{text}`")
}

#[test]
fn a_case_follows_its_problem_and_a_new_one_says_what_happened_last_time() {
    let mut s = Snapshot::default();
    assert!(see(&mut s, main_ci(A, CiState::Failing, &[RUST]), 0).0);
    assert_eq!(s.cases.len(), 1);
    let c = &s.cases[0];
    assert_eq!(c.id, s.problems[0].id);
    assert_eq!(
        (c.key.as_str(), c.kind, c.repo.as_str(), c.branch.as_str()),
        ("ci:a/b", ProblemKind::DefaultBranch, "a/b", "main")
    );
    assert_eq!(
        (c.end, c.sha.as_str(), c.checks.clone()),
        (End::Open, A, vec![RUST.to_string()])
    );
    // Nothing new: nothing changes.
    assert!(!see(&mut s, main_ci(A, CiState::Failing, &[RUST]), 5).0);
    // Another check fails at a newer commit: both are named, the latest
    // as the last.
    see(&mut s, main_ci(B, CiState::Failing, &["Lint"]), 6);
    let c = &s.cases[0];
    assert_eq!(
        (c.sha.as_str(), c.last.clone()),
        (B, vec!["Lint".to_string()])
    );
    assert_eq!(c.checks, [RUST, "Lint"]);
    // It passes at a newer commit.
    see(&mut s, main_ci(C, CiState::Passing, &[]), 10);
    let c = &s.cases[0];
    assert_eq!(c.end, End::Passed);
    assert_eq!(c.passed, C);
    assert_eq!(c.closed_at.as_deref(), Some(stamp(at(10)).as_str()));
    // It fails again three hours later, as a problem of its own, which says
    // what happened last time.
    see(&mut s, main_ci(A, CiState::Failing, &[RUST]), 190);
    assert_eq!(s.problems.len(), 2);
    assert_eq!(s.cases.len(), 2);
    assert_eq!(
        last_time(&s, 1),
        "Last time, 3 hours earlier: Lint failed on main. It passed again at 3333333, 10 minutes later."
    );
    assert!(s.problems[1].looked_back);
    // Its timeline already says it was fixed before, so it isn't said twice.
    let timeline: Vec<&str> = s.problems[1]
        .timeline
        .iter()
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(
        timeline,
        [
            "Failing on main at 1111111: CI / Rust.",
            "It was fixed before: it passed at 3333333 3 hours earlier."
        ]
    );
    // The agent hears it with names as code.
    assert_eq!(
        seen_before(&s.problems[1], &code),
        ["Last time, 3 hours earlier: `Lint` failed on `main`. It passed again at `3333333`, 10 minutes later."]
    );
    // It looks once.
    assert!(!see(&mut s, main_ci(A, CiState::Failing, &[RUST]), 195).0);
}

#[test]
fn passing_at_the_commit_it_failed_at_is_bad_luck() {
    let mut s = Snapshot::default();
    s.problems.push(problem("p1", A));
    let mut t = thread("p1", A);
    t.items[0].why = Some(Failure {
        step: "  Run cargo test ".into(),
        ..Default::default()
    });
    s.ops.push(t.clone());
    assert!(record(&mut s));
    assert_eq!(s.cases[0].step, "Run cargo test");
    assert!(!s.cases[0].reran);
    // Orbit reruns its job, and it passes at the same commit.
    t.items[0].reruns.push(Rerun {
        done: true,
        ..Default::default()
    });
    s.ops[0] = t;
    close(&mut s.problems[0], 30, A, "Passed at 1111111.");
    assert!(record(&mut s));
    let c = s.cases[0].clone();
    assert_eq!((c.end, c.reran, c.passed.as_str()), (End::Again, true, A));
    let l = LastTime {
        case: c.clone(),
        at: stamp(at(90)),
        text: String::new(),
    };
    assert_eq!(
        last_time_text(&l, &str::to_string),
        "Last time, an hour earlier: CI / Rust failed on main. It passed when Orbit reran it at the same commit, 30 minutes later."
    );
    // When it ran again by itself, it says so.
    let l = LastTime {
        case: Case { reran: false, ..c },
        ..l
    };
    assert!(last_time_text(&l, &str::to_string)
        .ends_with("It passed when it ran again at the same commit, 30 minutes later."));
    // A closed case stays as it ended.
    assert!(!record(&mut s));
}

#[test]
fn an_agents_fix_is_named_and_stopping_says_why() {
    let fixed = |passed_at: &str| {
        let mut s = Snapshot::default();
        s.problems.push(problem("p1", A));
        let mut t = thread("p1", A);
        t.items[0].answer = Some(Answer {
            kind: AnswerKind::Fixed,
            commit: Some(C.into()),
            at: stamp(at(5)),
            ..Default::default()
        });
        s.ops.push(t);
        record(&mut s);
        assert!(s.cases[0].fixed);
        assert_eq!(s.cases[0].fix, C);
        close(&mut s.problems[0], 120, passed_at, "Passed.");
        record(&mut s);
        let c = s.cases[0].clone();
        assert_eq!(c.end, End::Fixed);
        last_time_text(
            &LastTime {
                case: c,
                at: stamp(at(120)),
                text: String::new(),
            },
            &str::to_string,
        )
    };
    assert_eq!(
        fixed(C),
        "Last time, a minute earlier: CI / Rust failed on main. It passed again at 3333333 after the agent's fix, 2 hours later."
    );
    assert!(fixed(B)
        .ends_with("It passed again at 2222222 after the agent's fix in 3333333, 2 hours later."));

    // Closed without passing.
    let mut s = Snapshot::default();
    s.problems.push(Problem {
        kind: ProblemKind::PullRequest,
        key: "ci:a/b#5".into(),
        pr_id: Some("a/b#5".into()),
        branch: "fix-it".into(),
        ..problem("p1", A)
    });
    record(&mut s);
    assert_eq!(s.cases[0].number, Some(5));
    close(&mut s.problems[0], 45, "", "The pull request closed.");
    record(&mut s);
    let c = s.cases[0].clone();
    assert_eq!(
        (c.end, c.reason.as_str()),
        (End::Closed, "The pull request closed.")
    );
    let text = last_time_text(
        &LastTime {
            case: c,
            at: stamp(at(45)),
            text: String::new(),
        },
        &str::to_string,
    );
    assert_eq!(
        text,
        "Last time, a minute earlier: CI / Rust failed on pull request #5. Orbit stopped following it, 45 minutes later: The pull request closed."
    );
}

#[test]
fn a_problem_that_comes_back_within_the_hour_is_the_same_case() {
    let mut s = Snapshot::default();
    see(&mut s, main_ci(A, CiState::Failing, &[RUST]), 0);
    see(&mut s, main_ci(B, CiState::Passing, &[]), 10);
    assert_eq!(s.cases[0].end, End::Passed);
    see(&mut s, main_ci(C, CiState::Failing, &[RUST]), 20);
    assert_eq!(s.problems.len(), 1);
    assert_eq!(s.cases.len(), 1);
    let c = &s.cases[0];
    assert_eq!(
        (
            c.end,
            c.closed_at.clone(),
            c.passed.as_str(),
            c.sha.as_str()
        ),
        (End::Open, None, "", C)
    );
    // A problem already closed when Orbit began keeping cases gets none.
    let mut s = Snapshot::default();
    let mut p = problem("p1", A);
    close(&mut p, 5, B, "Passed.");
    s.problems.push(p);
    assert!(!record(&mut s));
    assert!(s.cases.is_empty());
}

#[test]
fn last_time_is_found_by_key_and_else_by_a_check_of_the_same_kind() {
    let mut s = Snapshot::default();
    s.cases.push(case("old", End::Passed, 9));
    s.cases.push(Case {
        key: "build:a/b:v1.0.0".into(),
        kind: ProblemKind::ReleaseBuild,
        branch: "v1.0.0".into(),
        ..case("build", End::Closed, 3)
    });
    // A pull request failing the same check, with no case of its own.
    s.problems.push(Problem {
        kind: ProblemKind::PullRequest,
        key: "ci:a/b#5".into(),
        pr_id: Some("a/b#5".into()),
        branch: "fix-it".into(),
        ..problem("p1", A)
    });
    // A release build failing that check too, and one in another
    // repository.
    s.problems.push(Problem {
        kind: ProblemKind::ReleaseBuild,
        key: "build:a/b:v1.1.0".into(),
        branch: "v1.1.0".into(),
        ..problem("p2", A)
    });
    s.problems.push(Problem {
        repo: "c/d".into(),
        key: "ci:c/d".into(),
        ..problem("p3", A)
    });
    assert!(look_back(&mut s));
    assert_eq!(
        last_time(&s, 0),
        "Last time, 9 days earlier: CI / Rust failed on main. It passed again at 2222222, an hour later."
    );
    assert_eq!(
        s.problems[0].timeline.last().map(|e| e.text.clone()),
        Some(last_time(&s, 0))
    );
    assert_eq!(
        last_time(&s, 1),
        "Last time, 3 days earlier: CI / Rust failed in the release build for v1.0.0. Orbit stopped following it, an hour later."
    );
    assert!(s.problems[2].last_time.is_none());
    assert!(s.problems.iter().all(|p| p.looked_back));
    assert!(!look_back(&mut s));
    // One with the same key comes first, even when an older one shares a
    // check; and a case that closed after the problem opened isn't last
    // time.
    let mut s = Snapshot::default();
    s.cases.push(Case {
        key: "ci:a/b#7".into(),
        ..case("near", End::Again, 1)
    });
    s.cases.push(case("same", End::Passed, 5));
    s.cases.push(case("later", End::Passed, -1));
    s.problems.push(Problem {
        opened_at: stamp(now()),
        ..problem("p1", A)
    });
    look_back(&mut s);
    assert_eq!(
        s.problems[0].last_time.as_ref().map(|l| l.case.id.as_str()),
        Some("same")
    );
}

#[test]
fn a_check_that_passes_when_run_again_three_times_is_a_lesson() {
    let mut s = Snapshot::default();
    s.cases.push(case("c1", End::Again, 20));
    s.cases.push(case("c2", End::Again, 3));
    assert!(learn(&mut s, now()).is_empty());
    s.cases.push(case("c3", End::Again, 10));
    let learned = learn(&mut s, now());
    assert_eq!(ids(&learned), ["luck:a/b:ci / rust"]);
    let l = &s.lessons[0];
    assert_eq!(
        (l.kind, l.check.as_str(), l.count),
        (LessonKind::Luck, RUST, 3)
    );
    assert_eq!(l.from, ["c2", "c3", "c1"]);
    assert_eq!(l.seen_at, days_ago(3));
    assert_eq!(l.learned_at, stamp(now()));
    assert_eq!(
        l.what,
        "CI / Rust in a/b failed and then passed when it ran again at the same commit 3 times in 30 days."
    );
    assert!(l
        .next
        .starts_with("When it fails again, running it again will likely pass"));
    // Learned once; a fourth time updates it, keeping when it was learned.
    assert!(learn(&mut s, now()).is_empty());
    s.cases.push(case("c4", End::Again, 1));
    assert!(learn(&mut s, at(60)).is_empty());
    assert_eq!(
        (s.lessons[0].count, s.lessons[0].learned_at.clone()),
        (4, stamp(now()))
    );
    // The next failure of that check hears it, with the check as code.
    s.problems.push(problem("p1", A));
    s.problems.push(Problem {
        failing: vec![check("Lint")],
        ..problem("p2", A)
    });
    assert!(apply(&mut s));
    assert_eq!(ids(&s.problems[0].lessons), ["luck:a/b:ci / rust"]);
    assert!(s.problems[1].lessons.is_empty());
    assert!(!apply(&mut s));
    let lines = seen_before(&s.problems[0], &code);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("`CI / Rust` in a/b failed and then passed"));
    assert!(lines[0].ends_with("so it stops failing by chance."));
    // A closed problem says nothing more.
    close(&mut s.problems[0], 5, A, "Passed.");
    assert!(seen_before(&s.problems[0], &code).is_empty());
}

#[test]
fn a_default_branch_that_keeps_breaking_is_a_lesson_but_a_pull_request_isnt() {
    let mut s = Snapshot::default();
    for (id, days) in [("c1", 12), ("c2", 2), ("c3", 7)] {
        s.cases.push(Case {
            passed: format!("{id:0<40}").replace('c', "a"),
            ..case(id, End::Passed, days)
        });
        s.cases.push(Case {
            key: "ci:a/b#5".into(),
            kind: ProblemKind::PullRequest,
            number: Some(5),
            ..case(&format!("pr-{id}"), End::Fixed, days)
        });
    }
    // Too old to count.
    s.cases.push(case("c0", End::Passed, 31));
    let learned = learn(&mut s, now());
    assert_eq!(ids(&learned), ["back:ci:a/b"]);
    let l = &s.lessons[0];
    assert_eq!(l.count, 3);
    assert_eq!(l.from, ["c2", "c3", "c1"]);
    assert_eq!(l.fixes[0], format!("{:0<40}", "a2"));
    assert_eq!(
        l.what,
        "CI on main in a/b broke 3 times in 30 days and was fixed each time, most recently at a200000."
    );
    assert_eq!(
        l.next,
        "Look for what keeps breaking it, not just the check that fails now."
    );
    // It is about the default branch's problems only.
    assert!(about(l, &problem("p1", A)));
    let pull = Problem {
        kind: ProblemKind::PullRequest,
        key: "ci:a/b#5".into(),
        ..problem("p2", A)
    };
    assert!(!about(l, &pull));
}

#[test]
fn fixes_settings_and_builds_that_miss_twice_are_lessons() {
    let mut s = Snapshot::default();
    s.outcomes.push(missed("o1", Kind::Fix, 4));
    s.outcomes.push(Outcome {
        verdict: Verdict::Held,
        ..missed("o2", Kind::Fix, 3)
    });
    s.outcomes.push(missed("o3", Kind::StillOn, 9));
    s.outcomes[2].setting = Some(Setting::DependabotAlerts);
    s.outcomes.push(missed("o4", Kind::Build, 2));
    assert!(learn(&mut s, now()).is_empty());
    s.outcomes.push(missed("o5", Kind::Fix, 1));
    s.outcomes.push(Outcome {
        setting: Some(Setting::DependabotAlerts),
        ..missed("o6", Kind::StillOn, 6)
    });
    s.outcomes.push(missed("o7", Kind::Build, 5));
    s.outcomes.push(missed("o8", Kind::Rerun, 1));
    s.outcomes.push(missed("o9", Kind::Rerun, 2));
    let learned = learn(&mut s, now());
    assert_eq!(
        ids(&learned),
        ["build:a/b", "fix:a/b", "setting:a/b:dependabot_alerts"]
    );
    let text = |id: &str| {
        let l = s.lessons.iter().find(|l| l.id == id).unwrap();
        (l.from.clone(), l.what.clone(), l.next.clone())
    };
    assert_eq!(
        text("fix:a/b"),
        (
            vec!["o5".to_string(), "o1".to_string()],
            "Fixes an agent named in a/b didn't pass their checks 2 times in 30 days.".into(),
            "Before saying a check is fixed, see it pass at the fix's commit.".into()
        )
    );
    assert_eq!(
        text("setting:a/b:dependabot_alerts").1,
        "Dependabot alerts in a/b was off again a week after Orbit turned it on, 2 times in 30 days."
    );
    assert_eq!(
        text("build:a/b").1,
        "Release builds in a/b failed after Orbit pushed their tag 2 times in 30 days."
    );
    // Fixes are about every failing check in the repository, builds only
    // about release builds, and a setting about no problem of checks.
    let lesson = |id: &str| s.lessons.iter().find(|l| l.id == id).unwrap();
    let build = Problem {
        kind: ProblemKind::ReleaseBuild,
        key: "build:a/b:v1.0.0".into(),
        ..problem("p2", A)
    };
    assert!(about(lesson("fix:a/b"), &problem("p1", A)));
    assert!(!about(lesson("build:a/b"), &problem("p1", A)));
    assert!(about(lesson("build:a/b"), &build));
    assert!(!about(
        lesson("setting:a/b:dependabot_alerts"),
        &problem("p1", A)
    ));
    let elsewhere = Problem {
        repo: "c/d".into(),
        ..problem("p3", A)
    };
    assert!(!about(lesson("fix:a/b"), &elsewhere));
}

#[test]
fn a_forgotten_lesson_is_learned_again_only_from_what_happens_after() {
    let mut s = Snapshot::default();
    for (id, days) in [("c1", 6), ("c2", 5), ("c3", 4)] {
        s.cases.push(case(id, End::Again, days));
    }
    s.problems.push(problem("p1", A));
    let (changed, learned) = follow(&mut s, now());
    assert!(changed);
    assert_eq!(learned.len(), 1);
    assert_eq!(s.problems[0].lessons.len(), 1);
    assert!(!forget(&mut s, "luck:a/b:lint", now()));
    assert!(forget(&mut s, "luck:a/b:ci / rust", now()));
    assert!(s.lessons.is_empty());
    assert!(s.problems[0].lessons.is_empty());
    assert_eq!(
        s.forgotten_lessons,
        [Forgot {
            id: "luck:a/b:ci / rust".into(),
            at: stamp(now()),
        }]
    );
    // What it was learned from doesn't teach it again.
    assert!(learn(&mut s, at(60)).is_empty());
    // Three more times after you forgot it do.
    for (id, minutes) in [("c4", 10), ("c5", 20)] {
        s.cases.push(Case {
            closed_at: Some(stamp(at(minutes))),
            ..case(id, End::Again, 0)
        });
    }
    assert!(learn(&mut s, at(60)).is_empty());
    s.cases.push(Case {
        closed_at: Some(stamp(at(30))),
        ..case("c6", End::Again, 0)
    });
    let learned = learn(&mut s, at(60));
    assert_eq!(ids(&learned), ["luck:a/b:ci / rust"]);
    assert_eq!(learned[0].from, ["c6", "c5", "c4"]);
}

#[test]
fn old_cases_lessons_and_what_you_forgot_are_let_go() {
    let mut s = Snapshot::default();
    s.cases.push(case("old", End::Passed, KEEP_DAYS + 1));
    s.cases.push(case("kept", End::Passed, KEEP_DAYS - 1));
    s.cases.push(Case {
        closed_at: None,
        end: End::Open,
        ..case("draft", End::Open, 0)
    });
    s.cases.push(Case {
        closed_at: None,
        end: End::Open,
        ..case("open", End::Open, 0)
    });
    s.problems.push(problem("open", A));
    s.lessons.push(Lesson {
        id: "luck:a/b:old".into(),
        seen_at: days_ago(KEEP_DAYS + 1),
        ..Default::default()
    });
    s.lessons.push(Lesson {
        id: "luck:a/b:new".into(),
        seen_at: days_ago(1),
        ..Default::default()
    });
    s.forgotten_lessons.push(Forgot {
        id: "luck:a/b:old".into(),
        at: days_ago(KEEP_DAYS + 1),
    });
    s.forgotten_lessons.push(Forgot {
        id: "luck:a/b:new".into(),
        at: days_ago(2),
    });
    assert!(prune(&mut s, now()));
    let cases: Vec<&str> = s.cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(cases, ["open", "kept"]);
    assert_eq!(ids(&s.lessons), ["luck:a/b:new"]);
    assert_eq!(s.forgotten_lessons.len(), 1);
    assert!(!prune(&mut s, now()));
    // At most so many, the newest.
    for i in 0..MAX_CASES + 5 {
        s.cases
            .push(case(&format!("c{i}"), End::Passed, (i % 80) as i64));
    }
    prune(&mut s, now());
    assert_eq!(s.cases.len(), MAX_CASES);
    assert_eq!(s.cases[0].id, "open");
    assert!(s.cases.iter().all(|c| c.id != "kept"));
}

#[test]
fn state_from_before_cases_still_loads() {
    let s: Snapshot = serde_json::from_str("{}").unwrap();
    assert!(s.cases.is_empty() && s.lessons.is_empty() && s.forgotten_lessons.is_empty());
    let p: Problem = serde_json::from_str(r#"{"id":"p1"}"#).unwrap();
    assert!(p.last_time.is_none() && !p.looked_back && p.lessons.is_empty());
    // A case and a lesson read back as written.
    let c = case("c1", End::Again, 1);
    let back: Case = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["end"], "again");
    assert_eq!(json["openedAt"], c.opened_at);
}
