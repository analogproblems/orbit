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

/// Bit asked about problem p1 at `now()`, and the answer came then, with a
/// button when on.
fn answered_about_p1(mode: &str) -> Snapshot {
    let mut s = snapshot(mode);
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    let proposals = vec![Proposal::new(crate::ask::Action::SyncNow)];
    answered(&mut s, note, Ok(("Sync now.".into(), proposals)));
    s.own_questions[0].answered_at = Some(stamp(now()));
    s
}
fn later(hours: i64) -> DateTime<Utc> {
    now() + Duration::hours(hours)
}

#[test]
fn a_problem_that_closes_after_you_use_a_button_turned_out_used_and_closed() {
    let mut s = answered_about_p1("on");
    let note = s.own_questions[0].note;
    assert!(used(&mut s, note, later(1)));
    // Not before it has stayed closed an hour, so a flap doesn't count.
    s.problems[0].closed_at = Some(stamp(later(3)));
    assert!(!settle(&mut s, later(3)));
    assert!(settle(&mut s, later(4)));
    let q = &s.own_questions[0];
    assert_eq!(q.turned, Some(Turned::UsedAndClosed));
    assert_eq!(
        q.how,
        "You used its button, and the problem closed 3 hours after the answer."
    );
    assert_eq!(q.settled_at, Some(stamp(later(4))));
    // Settled once.
    assert!(!settle(&mut s, later(30)));
}

#[test]
fn a_problem_that_closes_with_no_button_used_turned_out_closed() {
    for mode in ["watch", "on"] {
        let mut s = answered_about_p1(mode);
        s.problems[0].closed_at = Some(stamp(now() - Duration::minutes(1)));
        assert!(settle(&mut s, later(2)));
        let q = &s.own_questions[0];
        assert_eq!(q.turned, Some(Turned::Closed));
        assert_eq!(q.how, "The problem closed before the answer came.");
    }
}

#[test]
fn a_problem_still_open_a_day_and_an_hour_later_turned_out_unused_or_used() {
    let mut s = answered_about_p1("on");
    assert!(!settle(&mut s, later(24)));
    // It closed after the day: too late to count.
    s.problems[0].closed_at = Some(stamp(later(24) + Duration::minutes(10)));
    assert!(settle(&mut s, later(25)));
    assert_eq!(s.own_questions[0].turned, Some(Turned::Unused));
    assert_eq!(
        s.own_questions[0].how,
        "The problem was still open a day after the answer."
    );
    let mut s = answered_about_p1("on");
    let note = s.own_questions[0].note;
    used(&mut s, note, later(2));
    assert!(settle(&mut s, later(25)));
    assert_eq!(s.own_questions[0].turned, Some(Turned::Used));
}

#[test]
fn a_problem_whose_record_is_gone_cant_be_told() {
    let mut s = answered_about_p1("watch");
    s.problems.clear();
    assert!(settle(&mut s, later(1)));
    assert_eq!(s.own_questions[0].turned, Some(Turned::Unknown));
}

#[test]
fn the_briefs_answer_turned_out_used_at_once_or_unused_after_a_day() {
    let mut s = snapshot("on");
    s.brief.latest = Some(Brief {
        day: "2026-11-05".into(),
        at: at(2),
        ..Default::default()
    });
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    answered(&mut s, note, Ok(("Merge #12.".into(), vec![])));
    s.own_questions[0].answered_at = Some(stamp(now()));
    assert!(!settle(&mut s, later(23)));
    assert!(settle(&mut s, later(24)));
    assert_eq!(s.own_questions[0].turned, Some(Turned::Unused));
    assert_eq!(s.own_questions[0].how, "It had no button to use.");
    assert_eq!(about(&s.own_questions[0]), "about the morning brief");
    s.own_questions[0].settled_at = None;
    s.own_questions[0].used_at = Some(stamp(later(1)));
    assert!(settle(&mut s, later(1)));
    assert_eq!(s.own_questions[0].turned, Some(Turned::Used));
}

#[test]
fn only_an_answered_question_of_bits_own_counts_a_button() {
    let mut s = snapshot("on");
    s.problems
        .push(problem("p1", ProblemKind::DefaultBranch, 5));
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    // Still being answered.
    assert!(!used(&mut s, note, now()));
    assert!(!used(&mut s, note + 1, now()));
    answered(&mut s, note, Err("Codex couldn't finish.".into()));
    assert!(!settle(&mut s, later(48)));
    assert_eq!(s.own_questions[0].turned, None);
}

#[test]
fn says_what_each_question_was_about() {
    let mut s = snapshot("on");
    s.problems.extend([
        problem("main", ProblemKind::DefaultBranch, 10),
        problem("build", ProblemKind::ReleaseBuild, 3),
    ]);
    ask(&mut s).unwrap();
    let note = s.own_questions[0].note;
    answered(&mut s, note, Ok(("Look.".into(), vec![])));
    ask(&mut s).unwrap();
    assert_eq!(about(&s.own_questions[0]), "about CI on acme/widgets");
    assert_eq!(
        about(&s.own_questions[1]),
        "about the release build for v1.2.0 in acme/widgets"
    );
}

#[test]
fn a_button_used_after_the_problem_closed_counts_as_used_but_not_before() {
    // Used after it closed, before it settled.
    let mut s = answered_about_p1("on");
    let note = s.own_questions[0].note;
    s.problems[0].closed_at = Some(stamp(later(2)));
    used(&mut s, note, later(3) - Duration::minutes(30));
    assert!(settle(&mut s, later(3)));
    let q = &s.own_questions[0];
    assert_eq!(q.turned, Some(Turned::Closed));
    assert_eq!(
        q.how,
        "The problem closed 2 hours after the answer, before you used its button."
    );
    assert!(used_in_time(q));
    // Used after it closed and settled, still within the day.
    let mut s = answered_about_p1("on");
    let note = s.own_questions[0].note;
    s.problems[0].closed_at = Some(stamp(later(2)));
    assert!(settle(&mut s, later(3)));
    assert!(!used_in_time(&s.own_questions[0]));
    used(&mut s, note, later(5));
    let q = &s.own_questions[0];
    assert_eq!(q.turned, Some(Turned::Closed));
    assert!(used_in_time(q));
}

#[test]
fn a_button_used_after_the_day_doesnt_count() {
    for (minutes, counts) in [(24 * 60, true), (24 * 60 + 30, false)] {
        let mut s = answered_about_p1("on");
        let note = s.own_questions[0].note;
        used(&mut s, note, now() + Duration::minutes(minutes));
        assert_eq!(s.own_questions[0].used_at.is_some(), counts);
        // Settled late, it is still judged by the day.
        assert!(settle(&mut s, later(30)));
        let q = &s.own_questions[0];
        assert_eq!(used_in_time(q), counts);
        let want = if counts { Turned::Used } else { Turned::Unused };
        assert_eq!(q.turned, Some(want), "{minutes}");
    }
    // A use recorded after the day, as an older Orbit could, doesn't count
    // either.
    let mut s = answered_about_p1("on");
    s.own_questions[0].used_at = Some(stamp(later(25)));
    assert!(settle(&mut s, later(30)));
    assert!(!used_in_time(&s.own_questions[0]));
    assert_eq!(s.own_questions[0].turned, Some(Turned::Unused));
}
