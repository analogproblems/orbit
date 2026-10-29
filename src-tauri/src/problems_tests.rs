use super::*;
use crate::ci::{CommitCi, PullCi};
use crate::types::tests::pr;

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";

fn at(minutes: i64) -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH + Duration::days(20_000) + Duration::minutes(minutes)
}
fn check(name: &str) -> Check {
    Check {
        name: name.into(),
        url: String::new(),
    }
}
fn commit(sha: &str, state: CiState, failing: &[&str]) -> CommitCi {
    CommitCi {
        sha: sha.into(),
        state,
        failing: failing.iter().map(|n| check(n)).collect(),
        ..Default::default()
    }
}
fn branch(head: CommitCi) -> RepoCi {
    RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head,
        complete: true,
        ..Default::default()
    }
}
fn failing(sha: &str, checks: &[&str]) -> RepoCi {
    branch(commit(sha, CiState::Failing, checks))
}
fn passing(sha: &str) -> RepoCi {
    branch(commit(sha, CiState::Passing, &[]))
}
fn open(problems: &[Problem]) -> Vec<&Problem> {
    problems.iter().filter(|p| p.is_open()).collect()
}
fn timeline(p: &Problem) -> Vec<&str> {
    p.timeline.iter().map(|e| e.text.as_str()).collect()
}

#[test]
fn a_failing_default_branch_opens_one_problem_that_tells_you_once() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["CI / Rust"]), &[], at(0));
    assert_eq!(problems.len(), 1);
    let p = &problems[0];
    assert_eq!(p.key, "ci:a/b");
    assert_eq!(p.kind, ProblemKind::DefaultBranch);
    assert_eq!(p.alerts, 1);
    assert_eq!(timeline(p), ["Failing on main at 1111111: CI / Rust."]);
    let key = p.item_key();
    // Seen again, and on a newer commit with the same failures: quiet.
    observe(&mut problems, &failing(A, &["CI / Rust"]), &[], at(5));
    observe(&mut problems, &failing(B, &["CI / Rust"]), &[], at(10));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].item_key(), key);
    assert_eq!(problems[0].sha, B);
    assert_eq!(problems[0].since_sha, A);
    assert_eq!(
        timeline(&problems[0]),
        [
            "Failing on main at 1111111: CI / Rust.",
            "Still failing at 2222222."
        ]
    );
}

#[test]
fn running_or_unchecked_commits_change_nothing_and_a_pass_closes_it() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    observe(
        &mut problems,
        &branch(commit(B, CiState::Running, &[])),
        &[],
        at(1),
    );
    observe(
        &mut problems,
        &branch(commit(B, CiState::None, &[])),
        &[],
        at(2),
    );
    assert_eq!(open(&problems).len(), 1);
    assert_eq!(problems[0].timeline.len(), 1);
    observe(&mut problems, &passing(B), &[], at(3));
    assert!(open(&problems).is_empty());
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Passed at 2222222.")
    );
    // A passing branch with no problem opens nothing.
    observe(&mut problems, &passing(C), &[], at(4));
    assert_eq!(problems.len(), 1);
}

#[test]
fn more_failures_on_a_newer_commit_tell_you_again_but_not_on_the_same_one() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    // The same commit's other jobs finish failing too: an update, not news.
    observe(&mut problems, &failing(A, &["Test", "Lint"]), &[], at(2));
    assert_eq!(problems[0].alerts, 1);
    assert_eq!(
        timeline(&problems[0]).last(),
        Some(&"Now failing: Test and Lint.")
    );
    // A newer commit breaks a third check: worse.
    observe(
        &mut problems,
        &failing(B, &["Test", "Lint", "Build"]),
        &[],
        at(10),
    );
    assert_eq!(problems[0].alerts, 2);
}

#[test]
fn a_problem_still_open_reminds_you_a_day_later() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    observe(&mut problems, &failing(A, &["Test"]), &[], at(23 * 60));
    assert_eq!(problems[0].alerts, 1);
    observe(&mut problems, &failing(A, &["Test"]), &[], at(24 * 60));
    assert_eq!(problems[0].alerts, 2);
}

#[test]
fn a_flaky_branch_comes_back_quietly_and_tells_you_an_hour_after_its_alert() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    let key = problems[0].item_key();
    observe(&mut problems, &passing(B), &[], at(20));
    // Closed a moment ago, so its item stays announced.
    assert_eq!(quiet_keys(&problems, at(30)), std::slice::from_ref(&key));
    observe(&mut problems, &failing(C, &["Test"]), &[], at(30));
    assert_eq!(problems.len(), 1, "the same problem, back again");
    let p = &problems[0];
    assert!(p.is_open());
    assert_eq!(p.item_key(), key);
    assert_eq!(timeline(p).last(), Some(&"Failing again at 3333333."));
    assert!(p.alert_due.is_some());
    // Still red an hour after the first alert: it tells you then.
    observe(&mut problems, &failing(C, &["Test"]), &[], at(59));
    assert_eq!(problems[0].alerts, 1);
    observe(&mut problems, &failing(C, &["Test"]), &[], at(60));
    assert_eq!(problems[0].alerts, 2);
    assert!(problems[0].alert_due.is_none());
}

#[test]
fn a_flaky_branch_that_comes_back_worse_tells_you_at_once_and_only_once() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    observe(&mut problems, &passing(B), &[], at(10));
    observe(&mut problems, &failing(C, &["Test", "Lint"]), &[], at(20));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].alerts, 2, "worse tells you now");
    assert!(problems[0].alert_due.is_none());
    // The next check finds it the same: nothing new to tell.
    observe(&mut problems, &failing(C, &["Test", "Lint"]), &[], at(25));
    observe(&mut problems, &failing(C, &["Test", "Lint"]), &[], at(61));
    assert_eq!(problems[0].alerts, 2);
}

#[test]
fn a_branch_that_breaks_again_later_is_a_new_problem() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    observe(&mut problems, &passing(B), &[], at(10));
    observe(&mut problems, &failing(C, &["Test"]), &[], at(10 + 61));
    assert_eq!(problems.len(), 2);
    assert_eq!(open(&problems)[0].alerts, 1);
    assert!(quiet_keys(&problems, at(10 + 61)).is_empty());
}

fn followed(number: u64, sha: &str) -> PullRequest {
    let mut p = pr();
    p.number = number;
    p.id = pr_id("a/b", number);
    p.head_sha = sha.into();
    p.head_ref = format!("topic-{number}");
    p
}
fn with_pull(mut ci: RepoCi, number: u64, draft: bool, head: CommitCi) -> RepoCi {
    ci.pulls.push(PullCi {
        number,
        draft,
        ci: head,
    });
    ci
}

#[test]
fn a_pull_requests_failing_checks_are_listed_but_never_need_you() {
    let mut problems = vec![];
    let p = followed(7, A);
    let ci = with_pull(passing(C), 7, false, commit(A, CiState::Failing, &["Test"]));
    observe(&mut problems, &ci, &[&p], at(0));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].kind, ProblemKind::PullRequest);
    assert_eq!(problems[0].key, "ci:a/b#7");
    assert_eq!(problems[0].alerts, 0);
    assert!(!problems[0].needs_you());
    assert_eq!(
        timeline(&problems[0]),
        ["Failing on topic-7 at 1111111: Test."]
    );
    // The pull request moved on before GitHub's answer caught up: wait.
    let newer = followed(7, B);
    observe(&mut problems, &ci, &[&newer], at(1));
    assert_eq!(problems[0].sha, A);
}

#[test]
fn a_pull_request_problem_closes_with_its_pull_request_only_when_the_list_is_whole() {
    let mut problems = vec![];
    let p = followed(7, A);
    let ci = with_pull(passing(C), 7, false, commit(A, CiState::Failing, &["Test"]));
    observe(&mut problems, &ci, &[&p], at(0));
    let mut cut_short = passing(C);
    cut_short.complete = false;
    observe(&mut problems, &cut_short, &[], at(1));
    assert_eq!(open(&problems).len(), 1);
    observe(&mut problems, &passing(C), &[], at(2));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("The pull request closed.")
    );
}

#[test]
fn a_pull_request_back_to_draft_or_no_longer_followed_closes_its_problem() {
    let failing_pull =
        |draft| with_pull(passing(C), 7, draft, commit(A, CiState::Failing, &["Test"]));
    let p = followed(7, A);
    let mut problems = vec![];
    observe(&mut problems, &failing_pull(false), &[&p], at(0));
    observe(&mut problems, &failing_pull(true), &[], at(1));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("The pull request is a draft again.")
    );
    let mut problems = vec![];
    observe(&mut problems, &failing_pull(false), &[&p], at(0));
    observe(&mut problems, &failing_pull(false), &[], at(1));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Orbit no longer follows this pull request.")
    );
}

#[test]
fn disconnecting_or_turning_watching_off_closes_what_is_open() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    close_unwatched(&mut problems, &["A/B".into()], false, at(1));
    assert!(problems[0].is_open(), "names ignore case");
    close_unwatched(&mut problems, &[], false, at(2));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("The repository was disconnected.")
    );
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    close_unwatched(&mut problems, &["a/b".into()], true, at(1));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Orbit stopped watching CI.")
    );
}

#[test]
fn closed_problems_are_kept_for_two_weeks_and_open_ones_always() {
    let mut problems = vec![];
    observe(&mut problems, &failing(A, &["Test"]), &[], at(0));
    observe(&mut problems, &passing(B), &[], at(1));
    let mut later = failing(C, &["Test"]);
    later.repo = "a/c".into();
    observe(&mut problems, &later, &[], at(2));
    assert!(!prune(&mut problems, at(14 * 24 * 60)));
    assert!(prune(&mut problems, at(14 * 24 * 60 + 1)));
    assert_eq!(problems.len(), 1);
    assert!(problems[0].is_open());
}

#[test]
fn at_most_a_hundred_closed_problems_are_kept_even_when_they_closed_together() {
    let mut problems = vec![];
    for i in 0..150 {
        let mut ci = failing(A, &["Test"]);
        ci.repo = format!("a/r{i}");
        observe(&mut problems, &ci, &[], at(0));
    }
    // Watch CI goes off: every one closes at the same moment.
    close_unwatched(&mut problems, &[], true, at(1));
    let mut open = failing(B, &["Test"]);
    open.repo = "a/still-red".into();
    observe(&mut problems, &open, &[], at(2));
    assert!(prune(&mut problems, at(3)));
    assert_eq!(problems.iter().filter(|p| !p.is_open()).count(), MAX_CLOSED);
    assert_eq!(problems.iter().filter(|p| p.is_open()).count(), 1);
    let kept: Vec<String> = problems.iter().map(|p| p.id.clone()).collect();
    assert!(!prune(&mut problems, at(4)), "the same ones are kept");
    assert_eq!(
        problems.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        kept
    );
}

#[test]
fn the_timeline_keeps_its_newest_entries() {
    let mut problems = vec![];
    for i in 0..30 {
        let sha = format!("{i:040x}");
        observe(&mut problems, &failing(&sha, &["Test"]), &[], at(i));
    }
    let p = &problems[0];
    assert_eq!(p.timeline.len(), MAX_TIMELINE);
    assert_eq!(p.timeline.last().unwrap().text, "Still failing at 0000000.");
}

#[test]
fn check_names_read_as_a_sentence() {
    let checks: Vec<Check> = ["A", "B", "C", "D"].iter().map(|n| check(n)).collect();
    assert_eq!(names(&checks[..1]), "A");
    assert_eq!(names(&checks[..2]), "A and B");
    assert_eq!(names(&checks), "A, B and 2 more");
}
