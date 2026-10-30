use super::*;
use crate::ci::{Check, CommitCi, RepoCi};
use crate::logs::Failure;
use crate::ops::{explain, issue_body, record, refresh, Posted};
use crate::problems::ProblemKind;
use crate::types::{PullRequest, Repository};
use chrono::TimeZone;
use serde_json::json;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
}
fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
fn job(id: u32) -> String {
    format!("https://github.com/a/b/actions/runs/1/job/{id}")
}
const LOST: &str = "The runner went away.";

/// Orbit with `a/b` connected, its default branch failing `CI / Rust` at
/// commit 1 by job 2, whose log Orbit read: like bad luck when `flaky`.
fn state(flaky: Option<&str>) -> Snapshot {
    let mut s = Snapshot::default();
    s.settings.repositories.push(Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    });
    s.ci = vec![RepoCi {
        repo: "a/b".into(),
        head: CommitCi {
            sha: sha('1'),
            ..Default::default()
        },
        ..Default::default()
    }];
    s.problems = vec![Problem {
        id: "p1".into(),
        key: "ci:a/b".into(),
        kind: ProblemKind::DefaultBranch,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: sha('1'),
        since_sha: sha('1'),
        opened_at: stamp(t0()),
        ..Default::default()
    }];
    fail(&mut s, 2, flaky);
    s
}
/// The check fails again by job `id`, whose log Orbit read.
fn fail(s: &mut Snapshot, id: u32, flaky: Option<&str>) {
    s.problems[0].failing = vec![Check {
        name: "CI / Rust".into(),
        url: job(id),
    }];
    refresh(&mut s.ops, &s.problems, t0());
    s.failures = vec![Failure {
        url: job(id),
        check: "CI / Rust".into(),
        flaky: flaky.map(String::from),
        ..Default::default()
    }];
    explain(&mut s.ops, &s.failures);
}
fn item(s: &Snapshot) -> &Item {
    &s.ops[0].items[0]
}
/// Reruns what Orbit plans, as GitHub would take it, and returns Bit's
/// note.
fn rerun(s: &mut Snapshot, planned: &Planned, result: Result<(), &str>) -> Option<Event> {
    let id = record(&mut s.journal, planned, t0());
    let result = result
        .map(|()| Posted {
            id: 2,
            number: None,
            url: planned.rerun.as_ref().unwrap().url.clone(),
            updated_at: String::new(),
            body: String::new(),
        })
        .map_err(String::from);
    settle(&mut s.ops, &mut s.journal, &id, planned, result, t0())
}

/// A failed job that looks like bad luck is rerun once on its own; one that
/// doesn't, isn't.
#[test]
fn a_job_that_looks_like_bad_luck_is_rerun_once() {
    assert!(all(&state(None), t0()).is_empty());
    let mut s = state(Some(LOST));
    let planned = all(&s, t0());
    assert_eq!(planned.len(), 1);
    let p = &planned[0];
    assert_eq!(p.action, Action::RerunJob);
    assert_eq!(p.thread, "issue:p1");
    assert_eq!(
        p.rerun,
        Some(Job {
            item: 1,
            url: job(2),
            sha: sha('1'),
            reason: LOST.into(),
        })
    );
    let note = rerun(&mut s, p, Ok(()));
    assert_eq!(
        note,
        Some(Event::Reran {
            thread: "issue:p1".into(),
            number: 1
        })
    );
    let r = &item(&s).reruns[0];
    assert!(r.done && !r.already && r.error.is_none());
    assert_eq!((r.url.as_str(), r.reason.as_str()), (job(2).as_str(), LOST));
    let w = &s.journal[0];
    assert_eq!(
        (w.state, w.url.as_str()),
        (WriteState::Done, job(2).as_str())
    );
    assert!(all(&s, t0()).is_empty(), "once");
    // Not this job again, even when a second rerun needs no tap: only a new
    // try that fails counts as failing again.
    s.settings.buddy.rules.rerun_again = "do_it".into();
    assert!(all(&s, t0()).is_empty());
    let (_, _, text) = ops::event_text(&s.ops, &s.problems, &note.unwrap()).unwrap();
    assert_eq!(
        text,
        "Orbit reran I1 (CI / Rust) on main in a/b, as it looks like bad luck: The runner went away."
    );
}

/// Bit's rule decides, and nothing is rerun while Bit is off, Orbit no
/// longer watches the repository, or a newer commit is out.
#[test]
fn the_rule_and_the_commit_decide_whether_orbit_reruns() {
    let mut s = state(Some(LOST));
    s.settings.buddy.rules.rerun_flaky = "leave_it".into();
    assert!(all(&s, t0()).is_empty());
    s.settings.buddy.rules.rerun_flaky = "ask_first".into();
    assert!(all(&s, t0()).is_empty());
    s.ops[0].items[0].rerun_ok = job(2);
    assert_eq!(all(&s, t0()).len(), 1, "you said yes");

    for off in [
        |s: &mut Snapshot| s.settings.buddy.enabled = false,
        |s: &mut Snapshot| s.settings.watch_ci = false,
        |s: &mut Snapshot| s.settings.repositories.clear(),
        |s: &mut Snapshot| s.ci[0].head.sha = sha('2'),
        |s: &mut Snapshot| s.ops[0].gone = true,
    ] {
        let mut s = state(Some(LOST));
        off(&mut s);
        assert!(all(&s, t0()).is_empty());
    }
    // Only GitHub Actions jobs in the repository.
    let mut s = state(Some(LOST));
    s.ops[0].items[0].url = "https://ci.example.com/build/2".into();
    s.ops[0].items[0].why.as_mut().unwrap().url = s.ops[0].items[0].url.clone();
    assert!(all(&s, t0()).is_empty());
}

/// A pull request's job is rerun only at its latest commit, while Orbit
/// follows it.
#[test]
fn a_pull_requests_job_is_rerun_at_its_head() {
    let mut s = state(Some(LOST));
    s.problems[0] = Problem {
        id: "p2".into(),
        key: "ci:a/b#12".into(),
        kind: ProblemKind::PullRequest,
        pr_id: Some("a/b#12".into()),
        branch: "fix-it".into(),
        ..s.problems[0].clone()
    };
    s.ops.clear();
    fail(&mut s, 2, Some(LOST));
    let pr: PullRequest = serde_json::from_value(json!({
        "id": "a/b#12", "repo": "a/b", "number": 12, "title": "Fix it",
        "author": "agent", "body": "", "url": "", "headSha": sha('1'),
        "baseSha": sha('0'), "headRef": "fix-it", "baseRef": "main",
        "updatedAt": "", "draft": false, "additions": 1, "deletions": 0,
        "changedFiles": 1, "reviewRequested": false, "labels": [],
        "headRepo": "a/b", "authorAssociation": "MEMBER", "baseRepo": "a/b"
    }))
    .unwrap();
    s.prs = vec![pr.clone()];
    assert_eq!(all(&s, t0())[0].thread, "pr:a/b#12");
    s.prs[0].head_sha = sha('2');
    assert!(all(&s, t0()).is_empty(), "a newer commit is out");
    s.prs[0] = PullRequest {
        draft: true,
        ..pr.clone()
    };
    assert!(all(&s, t0()).is_empty(), "no longer followed");
    s.prs.clear();
    assert!(all(&s, t0()).is_empty());
}

/// After a rerun fails again, Bit asks before a second, and there is never
/// a third; each failure again is noticed once.
#[test]
fn a_second_rerun_waits_for_your_tap_and_there_is_no_third() {
    let mut s = state(Some(LOST));
    let first = all(&s, t0()).remove(0);
    rerun(&mut s, &first, Ok(()));
    // Before the new try shows, nothing changes.
    assert!(noticed(&mut s).is_empty());
    assert_eq!(
        reruns_text(&s),
        "Orbit reran this job, as it looks like bad luck: The runner went away."
    );
    // The new try fails, but its log isn't read yet.
    s.problems[0].failing[0].url = job(1002);
    refresh(&mut s.ops, &s.problems, t0());
    assert!(noticed(&mut s).is_empty());
    fail(&mut s, 1002, Some(LOST));
    let again = Event::FailedAgain {
        thread: "issue:p1".into(),
        number: 1,
        ask: true,
    };
    assert_eq!(noticed(&mut s), std::slice::from_ref(&again));
    assert!(noticed(&mut s).is_empty(), "once");
    assert_eq!(
        reruns_text(&s),
        "It failed again after it was rerun, so it's likely real."
    );
    let (_, _, text) = ops::event_text(&s.ops, &s.problems, &again).unwrap();
    assert_eq!(
        text,
        "I1 (CI / Rust) failed again on main in a/b after a rerun. It still looks like bad luck: The runner went away. Tap Rerun again on Operations to try once more."
    );
    assert!(all(&s, t0()).is_empty(), "Bit asks first");
    s.ops[0].items[0].rerun_ok = job(1002);
    let second = all(&s, t0()).remove(0);
    assert_eq!(second.rerun.as_ref().unwrap().url, job(1002));
    let note = rerun(&mut s, &second, Ok(())).unwrap();
    let (_, _, text) = ops::event_text(&s.ops, &s.problems, &note).unwrap();
    assert_eq!(
        text,
        "Orbit reran I1 (CI / Rust) on main in a/b a second time, as it still looks like bad luck: The runner went away."
    );
    assert_eq!(
        reruns_text(&s),
        "Orbit reran this job a second time, as it still looks like bad luck: The runner went away."
    );

    fail(&mut s, 2002, Some(LOST));
    let real = Event::FailedAgain {
        thread: "issue:p1".into(),
        number: 1,
        ask: false,
    };
    assert_eq!(noticed(&mut s), std::slice::from_ref(&real));
    let (_, _, text) = ops::event_text(&s.ops, &s.problems, &real).unwrap();
    assert_eq!(
        text,
        "I1 (CI / Rust) failed again on main in a/b after a rerun, so it's likely real."
    );
    s.ops[0].items[0].rerun_ok = job(2002);
    assert!(all(&s, t0()).is_empty(), "never a third");
    assert_eq!(
        reruns_text(&s),
        "It failed again after two reruns, so it's likely real."
    );
    let body = issue_body(&s.ops[0], &s.problems[0], false);
    assert!(body.contains(&format!(
        "[details]({})\n  It failed again after two reruns, so it's likely real.\n",
        job(2002)
    )));
    assert!(body.contains(r#""reruns":2"#));
}

/// A failure again that doesn't look like bad luck is real, and with Do it
/// for the second rerun Orbit reruns it without a note first.
#[test]
fn what_follows_a_failure_again() {
    let mut s = state(Some(LOST));
    let first = all(&s, t0()).remove(0);
    rerun(&mut s, &first, Ok(()));
    fail(&mut s, 1002, None);
    let real = Event::FailedAgain {
        thread: "issue:p1".into(),
        number: 1,
        ask: false,
    };
    assert_eq!(noticed(&mut s), [real]);
    assert!(all(&s, t0()).is_empty());

    let mut s = state(Some(LOST));
    s.settings.buddy.rules.rerun_again = "do_it".into();
    let first = all(&s, t0()).remove(0);
    rerun(&mut s, &first, Ok(()));
    fail(&mut s, 1002, Some(LOST));
    assert!(noticed(&mut s).is_empty());
    assert!(item(&s).reruns[0].failed_again);
    assert_eq!(all(&s, t0()).len(), 1);

    // Leave it to me: real, and no tap.
    s.settings.buddy.rules.rerun_again = "leave_it".into();
    s.ops[0].items[0].reruns[0].failed_again = false;
    assert!(matches!(
        noticed(&mut s)[..],
        [Event::FailedAgain { ask: false, .. }]
    ));
}

/// GitHub refusing a rerun is kept, and not asked again; a rerun GitHub
/// didn't answer for, or that hit its rate limit, is tried again after a
/// wait.
#[test]
fn a_refused_rerun_stays_refused_and_a_failed_one_waits() {
    let mut s = state(Some(LOST));
    let planned = all(&s, t0()).remove(0);
    let refused = "/bin/gh: gh: Resource not accessible by integration (HTTP 403)";
    assert_eq!(rerun(&mut s, &planned, Err(refused)), None);
    let r = &item(&s).reruns[0];
    assert!(r.lasting && !r.done && r.retry_at.is_none());
    assert_eq!(
        r.error.as_deref(),
        Some("GitHub won't rerun this job: Resource not accessible by integration (HTTP 403).")
    );
    assert_eq!(s.journal[0].state, WriteState::Failed);
    assert!(all(&s, t0() + Duration::days(1)).is_empty());
    assert_eq!(reruns_text(&s), "", "only reruns that went through");

    for error in [
        "gh: Bad Gateway (HTTP 502)",
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)",
    ] {
        let mut s = state(Some(LOST));
        let planned = all(&s, t0()).remove(0);
        rerun(&mut s, &planned, Err(error));
        let r = &item(&s).reruns[0];
        assert!(!r.lasting && !r.done, "{error}");
        assert_eq!(
            r.error.as_deref().unwrap(),
            format!(
                "Orbit couldn't rerun it yet: {}. It tries again later.",
                error[4..].trim_end_matches('.')
            )
        );
        assert_eq!(r.tries, 1);
        assert_eq!(
            r.retry_at.as_deref(),
            Some(&*stamp(t0() + Duration::minutes(1)))
        );
        assert!(all(&s, t0()).is_empty());
        assert_eq!(all(&s, t0() + Duration::minutes(1)).len(), 1);
        // Each failure in a row waits longer.
        rerun(&mut s, &planned, Err(error));
        assert_eq!(item(&s).reruns[0].tries, 2);
        assert_eq!(
            item(&s).reruns[0].retry_at.as_deref(),
            Some(&*stamp(t0() + Duration::minutes(2)))
        );
        rerun(&mut s, &planned, Ok(()));
        let r = &item(&s).reruns[0];
        assert!(r.done && r.error.is_none() && r.retry_at.is_none() && r.tries == 0);
    }
}

/// What Orbit looks for on GitHub before rerunning a job.
#[test]
fn orbit_looks_before_it_reruns() {
    let ours = json!({ "id": 2, "name": "Rust", "run_attempt": 1, "status": "completed" });
    let done = |jobs: Value| looked_at(&ours, &json!({ "jobs": jobs }));
    let docs = json!({ "id": 3, "name": "Docs", "run_attempt": 1, "status": "completed" });
    assert_eq!(done(json!([ours, docs])), Look::Ready);
    let newer = json!({ "id": 1002, "name": "Rust", "run_attempt": 2, "status": "queued" });
    assert_eq!(done(json!([newer, docs])), Look::Already);
    let busy = json!({ "id": 3, "name": "Docs", "run_attempt": 1, "status": "in_progress" });
    assert_eq!(done(json!([ours, busy])), Look::Running);
    // Another job's newer try isn't this one's.
    let other = json!({ "id": 1003, "name": "Docs", "run_attempt": 2, "status": "completed" });
    assert_eq!(done(json!([ours, other])), Look::Ready);
    assert_eq!(done(json!(null)), Look::Ready);

    let mut s = state(Some(LOST));
    let planned = all(&s, t0()).remove(0);
    looked(&mut s.ops, &planned, Ok(Look::Running), t0());
    let r = item(&s).reruns[0].clone();
    assert_eq!(
        r.error.as_deref(),
        Some("Its workflow is still running, so Orbit reruns it once that finishes.")
    );
    assert!(!r.done && !r.lasting);
    assert!(all(&s, t0()).is_empty());
    looked(&mut s.ops, &planned, Ok(Look::Ready), t0());
    assert_eq!(item(&s).reruns[0], r, "nothing to record");
    looked(
        &mut s.ops,
        &planned,
        Err("gh: Not Found (HTTP 404)".into()),
        t0(),
    );
    assert_eq!(
        item(&s).reruns[0].error.as_deref(),
        Some("GitHub no longer has this job.")
    );

    let mut s = state(Some(LOST));
    looked(&mut s.ops, &planned, Ok(Look::Already), t0());
    let r = &item(&s).reruns[0];
    assert!(r.done && r.already);
    assert_eq!(reruns_text(&s), "This job was already rerun on GitHub.");
    assert!(all(&s, t0()).is_empty());
    assert!(s.journal.is_empty(), "looking isn't a write");
}

/// Reruns are of one commit's jobs: a new commit starts again, and a rerun
/// that settles once the item has moved on leaves it be.
#[test]
fn reruns_start_again_at_a_new_commit() {
    let mut s = state(Some(LOST));
    let planned = all(&s, t0()).remove(0);
    rerun(&mut s, &planned, Ok(()));
    s.ops[0].items[0].rerun_ok = job(2);
    s.problems[0].sha = sha('2');
    s.ci[0].head.sha = sha('2');
    fail(&mut s, 3, Some(LOST));
    assert!(item(&s).reruns.is_empty() && item(&s).rerun_ok.is_empty());
    let planned = all(&s, t0()).remove(0);
    assert_eq!(planned.rerun.as_ref().unwrap().sha, sha('2'));

    // Moved on while it was sent.
    s.problems[0].sha = sha('3');
    refresh(&mut s.ops, &s.problems, t0());
    assert_eq!(rerun(&mut s, &planned, Ok(())), None);
    assert!(item(&s).reruns.is_empty());
    assert_eq!(s.journal.last().unwrap().state, WriteState::Done);

    // A check that passes after a rerun says so.
    let mut s = state(Some(LOST));
    let planned = all(&s, t0()).remove(0);
    rerun(&mut s, &planned, Ok(()));
    let p = &mut s.problems[0];
    p.closed_at = Some(stamp(t0()));
    p.closed_reason = Some("Passed at 1111111.".into());
    p.closed_sha = sha('1');
    refresh(&mut s.ops, &s.problems, t0());
    let body = issue_body(&s.ops[0], &s.problems[0], false);
    assert!(body.contains("· passes at `1111111` after a rerun\n"));
}

fn reruns_text(s: &Snapshot) -> String {
    text(item(s)).unwrap_or_default()
}
