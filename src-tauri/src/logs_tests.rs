use super::*;
use crate::ci::Check;
use crate::problems::ProblemKind;
use serde_json::json;

const JOB: &str = "https://github.com/a/b/actions/runs/1/job/2";

/// A job's log as GitHub gives it: each line with its time, the step's
/// command in a group, colours, a warning and two errors.
fn log() -> String {
    [
        "\u{feff}2026-10-29T12:00:00.1000000Z ##[group]Run actions/checkout@v4",
        "2026-10-29T12:00:00.1100000Z with:",
        "2026-10-29T12:00:00.1200000Z ##[endgroup]",
        "2026-10-29T12:00:01.0000000Z Syncing repository: a/b",
        "2026-10-29T12:00:02.0000000Z ##[group]Run cargo test",
        "2026-10-29T12:00:02.0100000Z cargo test --locked",
        "2026-10-29T12:00:02.0200000Z shell: /usr/bin/bash -e {0}",
        "2026-10-29T12:00:02.0300000Z ##[endgroup]",
        "2026-10-29T12:00:03.0000000Z    \x1b[1m\x1b[32mCompiling\x1b[0m orbit v0.1.0\r",
        "2026-10-29T12:00:04.0000000Z ",
        "2026-10-29T12:00:05.0000000Z test it_works ...\t\x1b[31mFAILED\x1b[0m",
        "2026-10-29T12:00:06.0000000Z ##[warning]Something odd",
        "2026-10-29T12:00:07.0000000Z ##[error]Process completed with exit code 101.",
        "2026-10-29T12:00:07.1000000Z ##[error]Second error",
        "2026-10-29T12:00:08.0000000Z Post job cleanup.",
        "2026-10-29T12:00:09.0000000Z ##[error]A later step's error",
    ]
    .join("\n")
}

/// The end of the failing step: after its command, up to and with its
/// errors, without times, colours, blank lines or GitHub's markers.
#[test]
fn an_excerpt_is_the_end_of_the_failing_step() {
    assert_eq!(
        excerpt(&log()),
        [
            "   Compiling orbit v0.1.0",
            "test it_works ... FAILED",
            "Warning: Something odd",
            "Error: Process completed with exit code 101.",
            "Error: Second error",
        ]
    );
}

/// An excerpt keeps the last `MAX_LINES` lines, each cut to `MAX_LINE`
/// characters; a log with no error gives the end of its last step, and one
/// with no steps marked gives its end.
#[test]
fn an_excerpt_is_short() {
    let mut lines = vec!["##[group]Run make".to_string(), "##[endgroup]".into()];
    lines.extend((1..=30).map(|n| format!("line {n}")));
    lines.push("x".repeat(300));
    lines.push("##[error]boom".into());
    let kept = excerpt(&lines.join("\n"));
    assert_eq!(kept.len(), MAX_LINES);
    assert_eq!(kept[0], "line 13");
    assert_eq!(kept[MAX_LINES - 2].chars().count(), MAX_LINE);
    assert!(kept[MAX_LINES - 2].ends_with("x…"));
    assert_eq!(kept[MAX_LINES - 1], "Error: boom");

    let quiet =
        "##[group]Run one\n##[endgroup]\nfirst\n##[group]Run two\nsecond\n##[endgroup]\nlast";
    assert_eq!(excerpt(quiet), ["last"]);
    assert_eq!(excerpt("a\n\nb\n##[error]c"), ["a", "b", "Error: c"]);
    assert_eq!(excerpt(""), Vec::<String>::new());
}

/// Only a GitHub Actions job's page in the problem's repository names a
/// job.
#[test]
fn only_actions_jobs_in_the_repository_are_read() {
    assert_eq!(job_of(JOB, "a/b"), Some(2));
    assert_eq!(
        job_of(
            "https://github.com/A/B/actions/runs/11/job/22?pr=12#step:4:7",
            "a/b"
        ),
        Some(22)
    );
    for url in [
        "https://github.com/c/d/actions/runs/1/job/2",
        "https://github.com/a/b/actions/runs/1",
        "https://github.com/a/b/actions/runs/1/job/",
        "https://github.com/a/b/actions/runs/x/job/2",
        "https://github.com/a/b/actions/runs/1/job/+2",
        "https://github.com/a/b/actions/runs//job/2",
        "https://github.com/a/b/actions/runs/1/job/2/extra",
        "https://github.com/a/b/runs/2",
        "http://github.com/a/b/actions/runs/1/job/2",
        "https://ci.example.com/a/b/actions/runs/1/job/2",
        "",
    ] {
        assert_eq!(job_of(url, "a/b"), None, "{url}");
    }
}

/// The step is the first GitHub marks failed or timed out.
#[test]
fn the_failed_step_is_the_first_marked_failed() {
    let job = json!({ "steps": [
        { "name": "Set up job", "conclusion": "success" },
        { "name": "Run \x1b[1mcargo test\x1b[0m", "conclusion": "failure" },
        { "name": "Run npm test", "conclusion": "failure" },
        { "name": "Post Run actions/checkout@v4", "conclusion": "skipped" }
    ] });
    assert_eq!(failed_step(&job), "Run cargo test");
    let slow = json!({ "steps": [{ "name": "Wait", "conclusion": "timed_out" }] });
    assert_eq!(failed_step(&slow), "Wait");
    assert_eq!(failed_step(&json!({ "steps": [] })), "");
    assert_eq!(failed_step(&json!([])), "");
}

fn check(name: &str, url: &str) -> Check {
    Check {
        name: name.into(),
        url: url.into(),
    }
}
fn problem(id: &str, kind: ProblemKind, checks: &[(&str, &str)]) -> Problem {
    Problem {
        id: id.into(),
        kind,
        repo: "a/b".into(),
        failing: checks.iter().map(|(n, u)| check(n, u)).collect(),
        ..Default::default()
    }
}
fn read(url: &str) -> Failure {
    Failure {
        url: url.into(),
        ..Default::default()
    }
}
fn job(n: u32) -> String {
    format!("https://github.com/a/b/actions/runs/1/job/{n}")
}

/// A pass reads the jobs of open problems Orbit hasn't read yet, default
/// branches first, each job once, and only a few.
#[test]
fn a_pass_reads_a_few_new_jobs_default_branches_first() {
    let mut closed = problem("p0", ProblemKind::DefaultBranch, &[("Old", &job(9))]);
    closed.closed_at = Some(now());
    let problems = vec![
        closed,
        problem(
            "p1",
            ProblemKind::PullRequest,
            &[("Lint", &job(1)), ("Status", ""), ("Again", &job(1))],
        ),
        problem(
            "p2",
            ProblemKind::DefaultBranch,
            &[("Rust", &job(2)), ("Known", &job(3))],
        ),
        problem(
            "p3",
            ProblemKind::PullRequest,
            &[("Docs", &job(4)), ("More", &job(5))],
        ),
    ];
    let wanted = wanted(&problems, &[read(&job(3))]);
    let urls: Vec<&str> = wanted.iter().map(|(_, _, u)| u.as_str()).collect();
    assert_eq!(urls, [job(2), job(1), job(4)]);
    assert_eq!(wanted[0], ("a/b".into(), "Rust".into(), job(2)));
    assert_eq!(wanted.len(), PER_PASS);
}

/// Orbit keeps what it read only while that job fails on an open problem.
#[test]
fn what_was_read_goes_once_its_job_stops_failing() {
    let mut closed = problem("p0", ProblemKind::DefaultBranch, &[("Old", &job(9))]);
    closed.closed_at = Some(now());
    let problems = vec![
        closed,
        problem("p1", ProblemKind::PullRequest, &[("Lint", &job(1))]),
    ];
    let mut kept = vec![read(&job(1)), read(&job(9)), read(&job(7))];
    assert!(prune(&mut kept, &problems));
    assert_eq!(kept, [read(&job(1))]);
    assert!(!prune(&mut kept, &problems));
}

/// A job's log that GitHub no longer has, or won't show, says so and isn't
/// asked for again; anything else is tried again later.
#[test]
fn lasting_errors_are_kept_and_others_retried() {
    let gone = lasting("gh: Not Found (HTTP 404)".into(), read(JOB)).unwrap();
    assert_eq!(
        gone.error.as_deref(),
        Some("GitHub no longer has this job's log.")
    );
    let expired = lasting("gh: Gone (HTTP 410)".into(), read(JOB)).unwrap();
    assert_eq!(expired.error, gone.error);
    let refused = lasting("gh: Forbidden (HTTP 403)".into(), read(JOB)).unwrap();
    assert_eq!(
        refused.error.as_deref(),
        Some("Orbit's GitHub sign-in can't read this job's log.")
    );
    assert!(lasting("gh: Bad Gateway (HTTP 502)".into(), read(JOB)).is_err());
}
