use super::*;
use crate::ci::Check;
use chrono::TimeZone;
use serde_json::json;

const JOB: &str = "https://github.com/a/b/actions/runs/1/job/2";

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
}
/// A line of a job's log as GitHub gives it, `s` seconds after `t0`.
fn at(s: f64, text: &str) -> String {
    let t = t0() + Duration::milliseconds((s * 1000.0) as i64);
    format!("{}0000Z {text}", t.format("%Y-%m-%dT%H:%M:%S%.3f"))
}
/// A step as GitHub's answer about a job gives it, from `from` to `to`
/// seconds after `t0`.
fn step(name: &str, conclusion: &str, from: i64, to: i64) -> Value {
    json!({
        "name": name,
        "conclusion": conclusion,
        "started_at": stamp(t0() + Duration::seconds(from)),
        "completed_at": stamp(t0() + Duration::seconds(to)),
    })
}
fn understood(steps: &[Value], log: &[String]) -> Failure {
    understand(read(JOB), &json!({ "steps": steps }), &log.join("\n"))
}

/// A job's log: checking out, then a test step with its command in a
/// group, colours, a warning and two errors, then a later step's error.
fn log() -> Vec<String> {
    vec![
        format!("\u{feff}{}", at(0.1, "##[group]Run actions/checkout@v4")),
        at(0.11, "with:"),
        at(0.12, "##[endgroup]"),
        at(1.0, "Syncing repository: a/b"),
        at(2.0, "##[group]Run cargo test"),
        at(2.01, "cargo test --locked"),
        at(2.02, "shell: /usr/bin/bash -e {0}"),
        at(2.03, "##[endgroup]"),
        at(3.0, "   \x1b[1m\x1b[32mCompiling\x1b[0m orbit v0.1.0\r"),
        at(4.0, ""),
        at(5.0, "test it_works ...\t\x1b[31mFAILED\x1b[0m"),
        at(6.0, "##[warning]Something odd"),
        at(7.0, "##[error]Process completed with exit code 101."),
        at(7.1, "##[error]Second error"),
        at(8.0, "Post job cleanup."),
        at(9.0, "##[error]A later step's error"),
    ]
}
fn steps() -> Vec<Value> {
    vec![
        step("Set up job", "success", 0, 0),
        step("Run actions/checkout@v4", "success", 0, 1),
        step("Run \x1b[1mcargo test\x1b[0m", "failure", 2, 7),
        step("Post Run actions/checkout@v4", "success", 8, 8),
        step("Complete job", "success", 9, 9),
    ]
}

/// The end of the step that failed: after its command, up to and with its
/// last error, without times, colours, blank lines or GitHub's markers,
/// and nothing a later step logged.
#[test]
fn an_excerpt_is_the_end_of_the_failing_step() {
    let failure = understood(&steps(), &log());
    assert_eq!(failure.step, "Run cargo test");
    assert_eq!(
        failure.lines,
        [
            "   Compiling orbit v0.1.0",
            "test it_works ... FAILED",
            "Warning: Something odd",
            "Error: Process completed with exit code 101.",
            "Error: Second error",
        ]
    );
    assert!(!failure.held);
}

/// An earlier step that logged an error but succeeded, or that continued
/// on error, isn't the one shown, nor is the end of a step that ran in
/// the same second before the failing one.
#[test]
fn only_the_step_that_failed_the_job_is_shown() {
    let log = vec![
        at(1.0, "##[group]Run npm run lint"),
        at(1.01, "##[endgroup]"),
        at(1.5, "##[error]src/a.ts: an annotation"),
        at(2.0, "##[group]Run npm run audit"),
        at(2.01, "##[endgroup]"),
        at(2.5, "audit found 1 problem"),
        at(3.0, "##[error]Process completed with exit code 1."),
        at(4.0, "##[group]Run npm run build"),
        at(4.01, "##[endgroup]"),
        at(5.4, "built"),
        at(5.5, "##[group]Run npm test"),
        at(5.51, "npm test"),
        at(5.52, "##[endgroup]"),
        at(6.0, "FAIL src/a.test.ts"),
        at(7.0, "##[error]Process completed with exit code 1."),
        at(8.0, "##[group]Run actions/upload-artifact@v4"),
        at(8.01, "##[endgroup]"),
        at(8.5, "##[error]No files were found"),
    ];
    let steps = [
        step("Set up job", "success", 0, 0),
        step("Lint", "success", 1, 1),
        step("Audit", "failure", 2, 3),
        step("Build", "success", 4, 5),
        step("Test", "failure", 5, 7),
        step("Deploy", "skipped", 7, 7),
        step("Upload results", "failure", 8, 8),
        step("Post Run actions/checkout@v4", "success", 9, 9),
        step("Complete job", "success", 9, 9),
    ];
    let failure = understood(&steps, &log);
    assert_eq!(failure.step, "Test");
    assert_eq!(
        failure.lines,
        [
            "FAIL src/a.test.ts",
            "Error: Process completed with exit code 1."
        ]
    );
}

/// The step is the first marked failed or timed out after which none of
/// the workflow's own steps succeeded, or else the last marked failed.
#[test]
fn the_failed_step_is_the_one_that_failed_the_job() {
    let name = |steps: Value| {
        failed_step(&json!({ "steps": steps })).map(|s| s["name"].as_str().unwrap().to_string())
    };
    let s = |name: &str, conclusion: &str| json!({ "name": name, "conclusion": conclusion });
    assert_eq!(
        name(json!([
            s("Set up job", "success"),
            s("Test", "failure"),
            s("More", "failure"),
            s("Post Run actions/checkout@v4", "success"),
            s("Complete job", "success")
        ])),
        Some("Test".into())
    );
    // Continued on error, then another step passed.
    assert_eq!(
        name(json!([
            s("Lint", "failure"),
            s("Build", "success"),
            s("Test", "failure"),
            s("Post Build", "success")
        ])),
        Some("Test".into())
    );
    // A clean-up step that always runs passed after the failure.
    assert_eq!(
        name(json!([
            s("Lint", "failure"),
            s("Test", "failure"),
            s("Clean up", "success")
        ])),
        Some("Test".into())
    );
    assert_eq!(name(json!([s("Wait", "timed_out")])), Some("Wait".into()));
    assert_eq!(name(json!([s("Test", "success")])), None);
    assert_eq!(name(json!([])), None);
    assert_eq!(failed_step(&json!([])), None);
}

/// A private key longer than an excerpt holds the log back, though the
/// excerpt would have kept only lines after the key's first line.
#[test]
fn a_secret_anywhere_in_the_step_holds_it_back() {
    let begin = format!("-----BEGIN {} PRIVATE KEY-----", "RSA");
    let mut log = vec![
        at(2.0, "##[group]Run ./deploy.sh"),
        at(2.01, "##[endgroup]"),
        at(2.1, &begin),
    ];
    log.extend((0..30).map(|n| at(2.2, &format!("MIIEkeybody{n:02}{}", "Q".repeat(50)))));
    log.push(at(2.3, &format!("-----END {} PRIVATE KEY-----", "RSA")));
    log.push(at(3.0, "##[error]Process completed with exit code 1."));
    let steps = [step("Deploy", "failure", 2, 3)];
    let failure = understood(&steps, &log);
    assert!(failure.held);
    assert!(failure.lines.is_empty() && failure.step.is_empty());
    assert!(!format!("{failure:?}").contains("keybody"));
    // In an earlier step that passed, it isn't the failed step's.
    log.insert(0, at(1.0, "##[group]Run ./ok.sh"));
    log.push(at(4.0, "##[group]Run ./check.sh"));
    log.push(at(4.01, "##[endgroup]"));
    log.push(at(5.0, "##[error]boom"));
    let earlier = [step("Ok", "success", 1, 3), step("Check", "failure", 4, 5)];
    let failure = understood(&earlier, &log);
    assert!(!failure.held);
    assert_eq!(failure.lines, ["Error: boom"]);
    // Without the steps' times the whole log is checked.
    let failure = understand(read(JOB), &json!({ "steps": [] }), &log.join("\n"));
    assert!(failure.held);
}

/// Without the failed step's times, the end of the log's last step with an
/// error is shown. An excerpt keeps the last `MAX_LINES` lines, each cut to
/// `MAX_LINE` characters.
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
    let later = "##[group]Run one\n##[endgroup]\nfirst\n##[error]e\n##[group]Run two\nsecond";
    assert_eq!(excerpt(later), ["first", "Error: e"]);
    assert_eq!(excerpt("a\n\nb\n##[error]c"), ["a", "b", "Error: c"]);
    assert_eq!(excerpt(""), Vec::<String>::new());
    // A step GitHub logged nothing for in its times shows nothing.
    let failure = understood(&[step("Test", "failure", 30, 31)], &log());
    assert_eq!(failure.step, "Test");
    assert!(failure.lines.is_empty());
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
fn urls(wanted: Vec<(String, String, String)>) -> Vec<String> {
    wanted.into_iter().map(|(_, _, u)| u).collect()
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
    let wanted = wanted(&problems, &[read(&job(3))], t0());
    assert_eq!(wanted[0], ("a/b".into(), "Rust".into(), job(2)));
    assert_eq!(urls(wanted), [job(2), job(1), job(4)]);
}

/// A log GitHub didn't give is tried again after a wait that doubles, up
/// to an hour, and meanwhile the next jobs are read, then the waiting ones
/// once due, after anything new.
#[test]
fn a_log_github_wont_give_waits_without_holding_up_others() {
    let problems = vec![problem(
        "p1",
        ProblemKind::DefaultBranch,
        &[
            ("A", &job(1)),
            ("B", &job(2)),
            ("C", &job(3)),
            ("D", &job(4)),
        ],
    )];
    let mut read = vec![];
    let first = wanted(&problems, &read, t0());
    assert_eq!(urls(first.clone()), [job(1), job(2), job(3)]);
    for (_, check, url) in &first {
        retry_later(
            &mut read,
            check,
            url,
            "gh: gh: Bad Gateway (HTTP 502)\n",
            t0(),
        );
    }
    assert_eq!(
        read[0].error.as_deref(),
        Some("Orbit couldn't read its log yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    assert_eq!(read[0].check, "A");
    assert_eq!(read[0].tries, 1);
    assert_eq!(read[0].retry_at, Some(stamp(t0() + Duration::minutes(1))));
    let soon = t0() + Duration::seconds(30);
    assert_eq!(urls(wanted(&problems, &read, soon)), [job(4)]);
    record(&mut read, read_ok(&job(4)));
    assert!(wanted(&problems, &read, soon).is_empty());
    let due = t0() + Duration::minutes(1);
    assert_eq!(
        urls(wanted(&problems, &read, due)),
        [job(1), job(2), job(3)]
    );
    let mut more = problems.clone();
    more[0].failing.push(check("E", &job(6)));
    assert_eq!(urls(wanted(&more, &read, due)), [job(6), job(1), job(2)]);

    let mut later = due;
    for wait in [2, 4, 8, 16, 32, 60, 60] {
        retry_later(&mut read, "A", &job(1), "gh: Bad Gateway (HTTP 502)", later);
        later += Duration::minutes(wait);
        assert_eq!(read[0].retry_at, Some(stamp(later)), "{wait}");
    }
    assert_eq!(read[0].tries, 8);
    record(&mut read, read_ok(&job(1)));
    assert_eq!(read.len(), 4);
    assert_eq!((read[0].tries, read[0].retry_at.clone()), (0, None));
}
fn read_ok(url: &str) -> Failure {
    Failure {
        step: "Run it".into(),
        ..read(url)
    }
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
/// asked for again; a rate limit, and anything else, is tried again later.
#[test]
fn lasting_errors_are_kept_and_others_retried() {
    let gone = lasting("gh: Not Found (HTTP 404)".into(), read(JOB)).unwrap();
    assert_eq!(
        gone.error.as_deref(),
        Some("GitHub no longer has this job's log.")
    );
    let expired = lasting("gh: Gone (HTTP 410)".into(), read(JOB)).unwrap();
    assert_eq!(expired.error, gone.error);
    let refused = lasting(
        "gh: Resource not accessible by integration (HTTP 403)".into(),
        read(JOB),
    )
    .unwrap();
    assert_eq!(
        refused.error.as_deref(),
        Some("Orbit's GitHub sign-in can't read this job's log.")
    );
    for error in [
        "gh: Bad Gateway (HTTP 502)",
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)",
        "gh: You have exceeded a secondary rate limit. Please wait a few minutes before you try again. (HTTP 403)",
        "gh: Too Many Requests (HTTP 429)",
    ] {
        assert!(lasting(error.into(), read(JOB)).is_err(), "{error}");
    }
    assert!(rate_limited(
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)"
    ));
    assert!(rate_limited("gh: Too Many Requests (HTTP 429)"));
    assert!(!rate_limited("gh: Bad Gateway (HTTP 502)"));
}
