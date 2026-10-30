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
    problems[0].broke = Some(crate::broke::Broke {
        done: true,
        ..Default::default()
    });
    observe(&mut problems, &passing(B), &[], at(20));
    // Closed a moment ago, so its item stays announced.
    assert_eq!(quiet_keys(&problems, at(30)), std::slice::from_ref(&key));
    observe(&mut problems, &failing(C, &["Test"]), &[], at(30));
    assert_eq!(problems.len(), 1, "the same problem, back again");
    let p = &problems[0];
    assert!(p.is_open());
    assert_eq!(p.item_key(), key);
    assert_eq!(timeline(p).last(), Some(&"Failing again at 3333333."));
    // It passed in between, so it starts again from C.
    assert_eq!(p.since_sha, C);
    assert_eq!(p.broke, None);
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

const D: &str = "4444444444444444444444444444444444444444";
const E: &str = "5555555555555555555555555555555555555555";
const F: &str = "6666666666666666666666666666666666666666";
/// A pull request's commit whose passing checks began at minute `began`.
fn ran(sha: &str, state: CiState, failing: &[&str], began: i64) -> CommitCi {
    CommitCi {
        ran_at: stamp(at(began)),
        ..commit(sha, state, failing)
    }
}
/// `main` at `head`, made at minute `made`, with pull request 7 at `pull`.
fn based(head: &str, made: i64, pull: CommitCi) -> RepoCi {
    let mut ci = passing(head);
    ci.head.committed_at = stamp(at(made));
    with_pull(ci, 7, false, pull)
}

#[test]
fn a_pull_request_that_passed_before_its_base_moved_on_is_listed_until_ci_runs_again() {
    let mut problems = vec![];
    let p = followed(7, A);
    // main's latest is older than the checks: they ran with it.
    observe(
        &mut problems,
        &based(C, -5, ran(A, CiState::Passing, &[], 0)),
        &[&p],
        at(1),
    );
    assert!(problems.is_empty());
    // main moves on after they began.
    observe(
        &mut problems,
        &based(B, 10, ran(A, CiState::Passing, &[], 0)),
        &[&p],
        at(11),
    );
    assert_eq!(problems.len(), 1);
    let b = &problems[0];
    assert_eq!(b.kind, ProblemKind::BaseMoved);
    assert_eq!(b.key, "base:a/b#7");
    assert_eq!(b.pr_id.as_deref(), Some("a/b#7"));
    assert_eq!((b.branch.as_str(), b.base.as_str()), ("topic-7", "main"));
    assert_eq!((b.sha.as_str(), b.since_sha.as_str()), (A, A));
    assert_eq!(b.base_sha, B);
    assert!(!b.needs_you());
    assert_eq!(b.alerts, 0);
    assert_eq!(
        timeline(b),
        ["Passed at 1111111, but main moved on to 2222222 after its checks began."]
    );
    // main moves on again.
    observe(
        &mut problems,
        &based(D, 20, ran(A, CiState::Passing, &[], 0)),
        &[&p],
        at(21),
    );
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].base_sha, D);
    assert_eq!(
        timeline(&problems[0]).last(),
        Some(&"main moved on again, to 4444444.")
    );
    // The author pushes again, but its checks began before main's latest.
    let pushed = followed(7, E);
    observe(
        &mut problems,
        &based(D, 20, ran(E, CiState::Passing, &[], 15)),
        &[&pushed],
        at(22),
    );
    assert_eq!(
        (problems[0].sha.as_str(), problems[0].since_sha.as_str()),
        (E, A)
    );
    assert_eq!(
        timeline(&problems[0]).last(),
        Some(&"Passed again at 5555555, still without main's latest.")
    );
    // Then CI runs again: running waits, and passing with it closes it.
    let again = followed(7, F);
    observe(
        &mut problems,
        &based(D, 20, commit(F, CiState::Running, &[])),
        &[&again],
        at(23),
    );
    assert!(problems[0].is_open());
    observe(
        &mut problems,
        &based(D, 20, ran(F, CiState::Passing, &[], 24)),
        &[&again],
        at(30),
    );
    assert_eq!(problems.len(), 1);
    assert!(problems[0].passed());
    assert_eq!(problems[0].closed_sha, F);
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Passed at 6666666 with main's latest.")
    );
    // main moving on later is a new problem.
    observe(
        &mut problems,
        &based(C, 40, ran(F, CiState::Passing, &[], 24)),
        &[&again],
        at(41),
    );
    assert_eq!(problems.len(), 2);
    assert_eq!(open(&problems)[0].since_sha, F);
    assert_ne!(problems[0].id, problems[1].id);
}

#[test]
fn a_base_that_moved_on_is_over_when_the_checks_fail_or_the_pull_request_goes() {
    let p = followed(7, A);
    let behind = |problems: &mut Vec<Problem>| {
        observe(
            problems,
            &based(B, 10, ran(A, CiState::Passing, &[], 0)),
            &[&p],
            at(11),
        );
        assert_eq!(open(problems).len(), 1);
    };
    // Its checks fail at a new commit: the failure is the problem now.
    let mut problems = vec![];
    behind(&mut problems);
    let pushed = followed(7, E);
    let failing = ran(E, CiState::Failing, &["Test"], 20);
    observe(&mut problems, &based(B, 10, failing), &[&pushed], at(21));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Its checks fail at 5555555 now.")
    );
    assert!(!problems[0].passed());
    assert_eq!(open(&problems)[0].kind, ProblemKind::PullRequest);
    // The pull request closes, once the list is whole.
    let mut problems = vec![];
    behind(&mut problems);
    observe(&mut problems, &passing(B), &[], at(12));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("The pull request closed.")
    );
    // It goes into another branch.
    let mut problems = vec![];
    behind(&mut problems);
    let mut elsewhere = p.clone();
    elsewhere.base_ref = "release".into();
    observe(
        &mut problems,
        &based(B, 10, ran(A, CiState::Passing, &[], 0)),
        &[&elsewhere],
        at(12),
    );
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Its base is no longer main.")
    );
    // The default branch is renamed, and the pull request's base with it:
    // the problem about main is over, and one about trunk opens at once.
    let mut problems = vec![];
    behind(&mut problems);
    let mut renamed = p.clone();
    renamed.base_ref = "trunk".into();
    let mut trunk = based(B, 10, ran(A, CiState::Passing, &[], 0));
    trunk.branch = "trunk".into();
    observe(&mut problems, &trunk, &[&renamed], at(12));
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Its base is no longer main.")
    );
    assert_eq!(open(&problems).len(), 1);
    assert_eq!(open(&problems)[0].base, "trunk");
    assert_eq!(
        timeline(open(&problems)[0]),
        ["Passed at 1111111, but trunk moved on to 2222222 after its checks began."]
    );
    // Before Orbit's list of pull requests catches up, only the old one ends.
    let mut problems = vec![];
    behind(&mut problems);
    observe(&mut problems, &trunk, &[&p], at(12));
    assert!(open(&problems).is_empty());
    // Nothing is listed when Orbit can't tell: into another branch, a
    // check that didn't say when it began, or main's latest without a time.
    let mut problems = vec![];
    observe(
        &mut problems,
        &based(B, 10, ran(A, CiState::Passing, &[], 0)),
        &[&elsewhere],
        at(11),
    );
    let untimed = commit(A, CiState::Passing, &[]);
    observe(&mut problems, &based(B, 10, untimed), &[&p], at(11));
    let mut no_time = based(B, 10, ran(A, CiState::Passing, &[], 0));
    no_time.head.committed_at.clear();
    observe(&mut problems, &no_time, &[&p], at(11));
    assert!(problems.is_empty());
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
    assert!(!prune(&mut problems, &Default::default(), at(14 * 24 * 60)));
    assert!(prune(
        &mut problems,
        &Default::default(),
        at(14 * 24 * 60 + 1)
    ));
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
    assert!(prune(&mut problems, &Default::default(), at(3)));
    assert_eq!(problems.iter().filter(|p| !p.is_open()).count(), MAX_CLOSED);
    assert_eq!(problems.iter().filter(|p| p.is_open()).count(), 1);
    let kept: Vec<String> = problems.iter().map(|p| p.id.clone()).collect();
    assert!(
        !prune(&mut problems, &Default::default(), at(4)),
        "the same ones are kept"
    );
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

fn setup(sha: &str, read: i64, workflows: &[(&str, crate::workflows::OnPulls)]) -> Setup {
    Setup {
        repo: "a/b".into(),
        branch: "main".into(),
        sha: sha.into(),
        read_at: stamp(at(read)),
        workflows: workflows
            .iter()
            .map(|(path, on)| crate::workflows::Workflow {
                path: path.to_string(),
                on_pulls: *on,
            })
            .collect(),
        proposal: "name: CI\n".into(),
        from: vec!["Cargo.toml".into()],
        ..Default::default()
    }
}
fn quiet(sha: &str) -> RepoCi {
    branch(commit(sha, CiState::None, &[]))
}

#[test]
fn a_repository_with_no_ci_on_pull_requests_is_listed_once_orbit_is_sure() {
    use crate::workflows::OnPulls;
    let mut problems = vec![];
    let mut none = setup(A, 0, &[]);
    // Not until a while after the files were read, so another service's
    // checks can show up.
    see_setup(&mut problems, Some(&mut none), &quiet(A), &[], at(9));
    assert!(problems.is_empty());
    // Files read at an older commit, or not read yet, say nothing.
    see_setup(&mut problems, Some(&mut none), &quiet(B), &[], at(20));
    see_setup(&mut problems, None, &quiet(A), &[], at(20));
    let mut failed = Setup {
        error: Some("down".into()),
        ..none.clone()
    };
    see_setup(&mut problems, Some(&mut failed), &quiet(A), &[], at(20));
    assert!(problems.is_empty());
    see_setup(&mut problems, Some(&mut none), &quiet(A), &[], at(10));
    assert_eq!(problems.len(), 1);
    let p = &problems[0];
    assert_eq!(p.key, "setup:a/b");
    assert_eq!(p.kind, ProblemKind::NoCi);
    assert_eq!((p.branch.as_str(), p.sha.as_str()), ("main", A));
    assert!(!p.needs_you());
    assert_eq!(p.alerts, 0);
    assert_eq!(p.found, Some(none.found()));
    assert_eq!(timeline(p), ["No workflows on main at 1111111."]);
    // The same again changes nothing; a newer commit with other files is
    // noted.
    see_setup(&mut problems, Some(&mut none), &quiet(A), &[], at(11));
    let mut deploy = setup(B, 12, &[(".github/workflows/deploy.yml", OnPulls::No)]);
    see_setup(&mut problems, Some(&mut deploy), &quiet(B), &[], at(12));
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].sha, B);
    assert_eq!(
        problems[0].found.as_ref().unwrap().workflows,
        [".github/workflows/deploy.yml"]
    );
    assert_eq!(
        timeline(&problems[0]),
        [
            "No workflows on main at 1111111.",
            "What Orbit found changed at 2222222."
        ]
    );
    // Checks on the default branch don't count when its workflows only run
    // there.
    let pushed = branch(commit(B, CiState::Passing, &[]));
    see_setup(&mut problems, Some(&mut deploy), &pushed, &[], at(13));
    assert!(problems[0].is_open());

    // A workflow that runs on pull requests closes it as done.
    let mut ci = setup(C, 14, &[(".github/workflows/ci.yml", OnPulls::Yes)]);
    see_setup(&mut problems, Some(&mut ci), &quiet(C), &[], at(14));
    let p = &problems[0];
    assert!(p.passed());
    assert_eq!(p.closed_sha, C);
    assert_eq!(
        p.closed_reason.as_deref(),
        Some("CI runs on pull requests at 3333333.")
    );
    assert_eq!(
        &timeline(p)[2..],
        [
            ".github/workflows/ci.yml runs on pull requests.",
            "CI runs on pull requests at 3333333."
        ]
    );
    // Once closed, nothing opens while CI runs.
    see_setup(&mut problems, Some(&mut ci), &quiet(C), &[], at(40));
    assert_eq!(problems.len(), 1);
}

#[test]
fn checks_or_files_orbit_cant_rule_out_mean_ci_runs() {
    use crate::workflows::OnPulls;
    let open_one = |problems: &mut Vec<Problem>| {
        see_setup(
            problems,
            Some(&mut setup(A, 0, &[])),
            &quiet(A),
            &[],
            at(10),
        );
        assert_eq!(open(problems).len(), 1);
    };
    // Checks on the default branch, with no workflows at all: another
    // service runs them. Never opened, and an open one closes, not done.
    let mut problems = vec![];
    let checked = branch(commit(A, CiState::Running, &[]));
    see_setup(
        &mut problems,
        Some(&mut setup(A, 0, &[])),
        &checked,
        &[],
        at(10),
    );
    assert!(problems.is_empty());
    open_one(&mut problems);
    see_setup(
        &mut problems,
        Some(&mut setup(A, 0, &[])),
        &checked,
        &[],
        at(11),
    );
    assert!(!problems[0].is_open() && !problems[0].passed());
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Checks ran on the default branch at 1111111, so something runs them.")
    );

    // Checks on a followed pull request's latest commit, whatever the
    // workflows say; not on an older commit, a draft's or one not followed.
    let mut deploy = setup(A, 0, &[(".github/workflows/deploy.yml", OnPulls::No)]);
    let pr = followed(7, B);
    let older = with_pull(quiet(A), 7, false, commit(C, CiState::Passing, &[]));
    let mut problems = vec![];
    see_setup(&mut problems, Some(&mut deploy), &older, &[&pr], at(10));
    see_setup(&mut problems, Some(&mut deploy), &older, &[], at(10));
    assert_eq!(open(&problems).len(), 1);
    let unchecked = with_pull(quiet(A), 7, false, commit(B, CiState::None, &[]));
    see_setup(&mut problems, Some(&mut deploy), &unchecked, &[&pr], at(11));
    assert_eq!(open(&problems).len(), 1);
    let checked = with_pull(quiet(A), 7, false, commit(B, CiState::Failing, &["x"]));
    see_setup(&mut problems, Some(&mut deploy), &checked, &[&pr], at(12));
    assert!(open(&problems).is_empty());
    assert_eq!(
        problems[0].closed_reason.as_deref(),
        Some("Checks ran on #7, so something runs them.")
    );
    // Once seen, checks count from then on: the pull request closing, or
    // a newer commit without them, opens nothing again.
    see_setup(&mut problems, Some(&mut deploy), &quiet(A), &[], at(20));
    assert_eq!((problems.len(), open(&problems).len()), (1, 0));
    let mut setups = vec![deploy];
    let newer = setup(B, 20, &[(".github/workflows/deploy.yml", OnPulls::No)]);
    crate::workflows::record(&mut setups, "a/b", "main", B, Ok(newer), at(20));
    see_setup(&mut problems, setups.first_mut(), &quiet(B), &[], at(40));
    assert_eq!((problems.len(), open(&problems).len()), (1, 0));
    // Never opened where a pull request has checks.
    let mut problems = vec![];
    let mut fresh = setup(A, 0, &[(".github/workflows/deploy.yml", OnPulls::No)]);
    see_setup(&mut problems, Some(&mut fresh), &checked, &[&pr], at(20));
    assert!(problems.is_empty());
    assert_eq!(
        fresh.ran.as_deref(),
        Some("Checks ran on #7, so something runs them.")
    );

    // A workflow Orbit can't read, or another service's files.
    let mut problems = vec![];
    open_one(&mut problems);
    let mut odd = setup(A, 0, &[(".github/workflows/odd.yml", OnPulls::Unknown)]);
    see_setup(&mut problems, Some(&mut odd), &quiet(A), &[], at(11));
    assert!(!problems[0].passed());
    assert_eq!(
        &timeline(&problems[0])[1..],
        [
            "Orbit can't tell whether .github/workflows/odd.yml runs on pull requests.",
            "Orbit can't tell any more that nothing runs on pull requests, at 1111111."
        ]
    );
    let mut problems = vec![];
    open_one(&mut problems);
    let mut circle = Setup {
        others: vec![".circleci".into()],
        ..setup(A, 0, &[])
    };
    see_setup(&mut problems, Some(&mut circle), &quiet(A), &[], at(11));
    assert!(problems[0].passed());
    assert_eq!(
        timeline(&problems[0])[1],
        ".circleci sets up another CI service."
    );
    // Disconnecting closes it like any other.
    let mut problems = vec![];
    open_one(&mut problems);
    close_unwatched(&mut problems, &[], false, at(12));
    assert!(open(&problems).is_empty());
}
