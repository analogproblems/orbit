use super::*;
use crate::{
    brief::Brief,
    buddy::{asked_itself, NoteKind},
    health::{HealthIssue, HealthKind},
    problems::Problem,
};

fn at(minutes_ago: i64) -> String {
    stamp(now() - Duration::minutes(minutes_ago))
}
fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-05T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn problem(id: &str, kind: ProblemKind, minutes_ago: i64) -> Problem {
    Problem {
        id: id.into(),
        key: format!("ci:acme/widgets/{id}"),
        kind,
        repo: "acme/widgets".into(),
        branch: if kind == ProblemKind::ReleaseBuild {
            "v1.2.0".into()
        } else {
            "main".into()
        },
        opened_at: at(minutes_ago),
        updated_at: at(minutes_ago),
        ..Default::default()
    }
}
fn snapshot(mode: &str) -> Snapshot {
    let mut s = Snapshot::default();
    s.settings.buddy.own_questions = mode.into();
    s
}
fn ask(s: &mut Snapshot) -> Option<Subject> {
    let subject = next(s, now())?;
    let note = asked_itself(s, &subject.question, subject.mode != "on");
    asked(s, &subject, note, now());
    Some(subject)
}

#[test]
fn off_by_default_and_asks_nothing_while_off() {
    assert_eq!(crate::buddy::Buddy::default().own_questions, "off");
    let mut s = snapshot("off");
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    assert_eq!(next(&s, now()), None);
}

#[test]
fn asks_about_a_new_default_branch_or_release_problem_once() {
    for mode in ["watch", "on"] {
        let mut s = snapshot(mode);
        s.problems.extend([
            // Not about a pull request, an old one or a closed one.
            problem("pr", ProblemKind::PullRequest, 1),
            problem("old", ProblemKind::DefaultBranch, 61),
            Problem {
                closed_at: Some(at(1)),
                ..problem("closed", ProblemKind::DefaultBranch, 2)
            },
            problem("build", ProblemKind::ReleaseBuild, 3),
            problem("main", ProblemKind::DefaultBranch, 10),
        ]);
        let first = next(&s, now()).unwrap();
        // The older of the two first.
        assert_eq!(first.problem_id.as_deref(), Some("main"));
        assert_eq!(first.mode, mode);
        assert_eq!(
            first.question,
            "CI started failing on the default branch of acme/widgets. What's the most useful thing to do about it now?"
        );
        ask(&mut s).unwrap();
        // Nothing more while it is answered.
        assert_eq!(next(&s, now()), None);
        let note = s.own_questions[0].note;
        answered(&mut s, note, Ok(("Rerun it.".into(), vec![])));
        let second = ask(&mut s).unwrap();
        assert_eq!(second.problem_id.as_deref(), Some("build"));
        assert!(second
            .question
            .starts_with("The release build for v1.2.0 in acme/widgets is failing."));
        let note = s.own_questions[1].note;
        answered(&mut s, note, Err("Codex couldn't finish.".into()));
        // Each asked about once, answered or not.
        assert_eq!(next(&s, now()), None);
    }
}

#[test]
fn a_problem_that_opens_again_is_a_new_subject() {
    let mut s = snapshot("on");
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    answered(&mut s, note, Ok(("Look.".into(), vec![])));
    s.problems[0].opened_at = at(1);
    let again = next(&s, now()).unwrap();
    assert_eq!(again.subject, format!("problem:p1@{}", at(1)));
}

#[test]
fn asks_about_todays_brief_once() {
    let mut s = snapshot("watch");
    s.brief.latest = Some(Brief {
        day: "2026-11-05".into(),
        at: at(70),
        ..Default::default()
    });
    // A brief from over an hour ago is too old.
    assert_eq!(next(&s, now()), None);
    s.brief.latest.as_mut().unwrap().at = at(2);
    let subject = ask(&mut s).unwrap();
    assert_eq!(subject.subject, "brief:2026-11-05");
    assert_eq!(subject.question, brief_question());
    let note = s.own_questions[0].note;
    answered(&mut s, note, Ok(("Merge #12.".into(), vec![])));
    assert_eq!(next(&s, now()), None);
}

#[test]
fn asks_at_most_a_few_a_day() {
    let mut s = snapshot("on");
    for i in 0..PER_DAY + 1 {
        s.problems.push(problem(
            &format!("p{i}"),
            ProblemKind::DefaultBranch,
            50 - i as i64,
        ));
    }
    for _ in 0..PER_DAY {
        ask(&mut s).unwrap();
        let note = s.own_questions.last().unwrap().note;
        answered(&mut s, note, Ok(("Look.".into(), vec![])));
    }
    assert_eq!(next(&s, now()), None);
    // A day later, the count starts over, for the one left.
    for q in &mut s.own_questions {
        q.asked_at = stamp(now() - Duration::hours(25));
    }
    let last = format!("p{PER_DAY}");
    assert_eq!(
        next(&s, now()).unwrap().problem_id.as_deref(),
        Some(last.as_str())
    );
}

#[test]
fn waits_while_bit_is_off_or_codex_is_near_a_limit() {
    let mut s = snapshot("on");
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    s.settings.buddy.enabled = false;
    assert_eq!(next(&s, now()), None);
    s.settings.buddy.enabled = true;
    s.health.push(HealthIssue {
        id: "usage".into(),
        kind: HealthKind::Usage,
        opened_at: at(30),
        ..Default::default()
    });
    assert_eq!(next(&s, now()), None);
    // Another answerer doesn't use Codex's allowance.
    s.settings.buddy.answerer = "claude".into();
    assert!(next(&s, now()).is_some());
    s.settings.buddy.answerer = "codex".into();
    s.health[0].closed_at = Some(at(1));
    assert!(next(&s, now()).is_some());
}

#[test]
fn watching_keeps_the_answer_out_of_bits_feed() {
    let mut s = snapshot("watch");
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    let shown = s.buddy_feed.notes.iter().find(|n| n.id == note).unwrap();
    assert!(shown.own && shown.quiet && shown.pending);
    assert_eq!(shown.kind, NoteKind::Answer);
    assert_eq!(
        of_note(&s, note).unwrap().subject,
        s.own_questions[0].subject
    );
    answered(&mut s, note, Ok(("Rerun the tests.".into(), vec![])));
    assert!(!s.buddy_feed.notes.iter().any(|n| n.id == note));
    let q = &s.own_questions[0];
    assert_eq!(q.answer, "Rerun the tests.");
    assert!(q.answered_at.is_some() && !q.pending());
    assert_eq!(of_note(&s, note), None);
}

#[test]
fn on_keeps_the_answer_in_bits_feed_but_never_an_error() {
    let mut s = snapshot("on");
    s.problems.extend([
        problem("p1", ProblemKind::DefaultBranch, 5),
        problem("p2", ProblemKind::DefaultBranch, 4),
    ]);
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    assert!(!s.buddy_feed.notes[0].quiet);
    answered(&mut s, note, Ok(("Rerun the tests.".into(), vec![])));
    assert!(s.buddy_feed.notes.iter().any(|n| n.id == note));
    ask(&mut s).unwrap();
    let note = s.own_questions[1].note;
    answered(&mut s, note, Err("Codex couldn't finish.".into()));
    assert!(!s.buddy_feed.notes.iter().any(|n| n.id == note));
    assert_eq!(
        s.own_questions[1].error.as_deref(),
        Some("Codex couldn't finish.")
    );
}

#[test]
fn a_question_open_when_orbit_closed_has_no_answer() {
    let mut s = snapshot("on");
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    ask(&mut s).unwrap();
    recover(&mut s);
    assert!(s.buddy_feed.notes.is_empty());
    assert_eq!(
        s.own_questions[0].error.as_deref(),
        Some("Orbit closed before it was answered.")
    );
}

#[test]
fn keeps_a_month_and_never_drops_one_being_answered() {
    let mut list: Vec<OwnQuestion> = (0..KEEP + 5)
        .map(|i| OwnQuestion {
            subject: format!("brief:{i}"),
            asked_at: at(10),
            answered_at: Some(at(9)),
            ..Default::default()
        })
        .collect();
    list.push(OwnQuestion {
        subject: "old".into(),
        asked_at: stamp(now() - Duration::days(KEEP_DAYS + 1)),
        answered_at: Some(at(9)),
        ..Default::default()
    });
    list.insert(
        0,
        OwnQuestion {
            subject: "pending".into(),
            asked_at: stamp(now() - Duration::days(KEEP_DAYS + 1)),
            ..Default::default()
        },
    );
    assert!(prune(&mut list, now()));
    assert_eq!(list.len(), KEEP);
    assert_eq!(list[0].subject, "pending");
    assert!(!list.iter().any(|q| q.subject == "old"));
    assert!(!prune(&mut list, now()));
}
