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
/// A job's link: `.../actions/runs/<run>/job/<id>`.
fn job(run: u64, id: u64) -> String {
    format!("https://github.com/a/b/actions/runs/{run}/job/{id}")
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
/// `main` at `sha` read with each check by name, as Orbit reads it, and
/// passing as a whole: `checks` say where each one stands.
fn read(sha: &str, checks: &[(&str, CiState)]) -> RepoCi {
    let mut ci = main_ci(sha, CiState::Passing, &[]);
    ci.head.named = true;
    ci.head.checks = checks.iter().map(|(n, state)| (check(n), *state)).collect();
    ci
}
/// `main` at `sha` read with each check by name, failing these.
fn red(sha: &str, failing: &[&str]) -> RepoCi {
    let mut ci = main_ci(sha, CiState::Failing, failing);
    ci.head.named = true;
    ci.head.checks = failing
        .iter()
        .map(|n| (check(n), CiState::Failing))
        .collect();
    ci
}
/// What Orbit does on each pass: reads CI, then follows the problems.
fn see(s: &mut Snapshot, ci: RepoCi, minutes: i64) -> (bool, Vec<Lesson>) {
    observe(&mut s.problems, &ci, &[], at(minutes));
    s.ci = vec![ci];
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
    s.ci = vec![red(A, &[RUST])];
    assert!(record(&mut s));
    assert_eq!(s.cases[0].step, "Run cargo test");
    assert_eq!(s.cases[0].failed, [check(RUST)]);
    assert!(!s.cases[0].reran);
    // Orbit reruns its job, and it passes at the same commit.
    t.items[0].reruns.push(Rerun {
        done: true,
        ..Default::default()
    });
    s.ops[0] = t;
    close(&mut s.problems[0], 30, A, "Passed at 1111111.");
    // GitHub says the check that failed passed there.
    s.ci = vec![read(
        A,
        &[(RUST, CiState::Passing), ("Lint", CiState::Passing)],
    )];
    assert!(record(&mut s));
    let c = s.cases[0].clone();
    assert_eq!((c.end, c.reran, c.passed.as_str()), (End::Again, true, A));
    // It keeps the job that failed until it can't come back.
    assert_eq!(c.failed, [check(RUST)]);
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

/// A check cancelled or skipped when it ran again doesn't fail, so its
/// problem closes, but it didn't pass either: Orbit can't call that bad
/// luck, and learns nothing from it.
#[test]
fn a_failed_check_orbit_didnt_see_pass_isnt_bad_luck() {
    let ending = |ci: Option<RepoCi>| {
        let mut s = Snapshot::default();
        s.problems.push(problem("p1", A));
        s.ci = vec![red(A, &[RUST])];
        record(&mut s);
        close(&mut s.problems[0], 30, A, "Passed at 1111111.");
        s.ci = ci.into_iter().collect();
        record(&mut s);
        s.cases[0].clone()
    };
    use CiState::{Failing, None as Neither, Passing, Running};
    // Cancelled or skipped beside one that passed, or not read at all.
    for ci in [
        Some(read(A, &[(RUST, Neither), ("Lint", Passing)])),
        Some(read(A, &[("Lint", Passing)])),
        Some(read(A, &[(RUST, Passing), (RUST, Running)])),
        Some(read(A, &[(RUST, Passing), (RUST, Failing)])),
        Some(read(B, &[(RUST, Passing)])),
        Some(RepoCi {
            head: CommitCi {
                incomplete: true,
                ..read(A, &[(RUST, Passing)]).head
            },
            ..read(A, &[])
        }),
        Some(main_ci(A, CiState::Passing, &[])),
        None,
    ] {
        assert_eq!(ending(ci.clone()).end, End::Unclear, "{ci:?}");
    }
    assert_eq!(ending(Some(read(A, &[(RUST, Passing)]))).end, End::Again);
    let c = ending(None);
    let text = last_time_text(
        &LastTime {
            case: c.clone(),
            at: stamp(at(90)),
            text: String::new(),
        },
        &str::to_string,
    );
    assert_eq!(
        text,
        "Last time, an hour earlier: CI / Rust failed on main. It stopped failing at the same commit, 30 minutes later, but Orbit didn't see what failed pass there: GitHub may have cancelled or skipped it, or started it over as a new run."
    );
    // Three of them teach nothing.
    let mut s = Snapshot::default();
    for (id, days) in [("c1", 3), ("c2", 2), ("c3", 1)] {
        s.cases.push(case(id, End::Unclear, days));
    }
    assert!(learn(&mut s, now()).is_empty());

    // A release build, by the run each failed job is in.
    let run = |id: u64, name: &str, conclusion: &str| crate::builds::Run {
        id,
        name: name.into(),
        url: format!("https://github.com/a/b/actions/runs/{id}"),
        state: match conclusion {
            "failure" => CiState::Failing,
            _ => CiState::Passing,
        },
        conclusion: conclusion.into(),
        ..Default::default()
    };
    let build = |runs: Vec<crate::builds::Run>| crate::builds::Build {
        repo: "a/b".into(),
        tag: "v1.0.0".into(),
        sha: A.into(),
        read_at: stamp(at(30)),
        runs,
        ..Default::default()
    };
    let release = |b: crate::builds::Build| {
        let mut s = Snapshot::default();
        s.problems.push(Problem {
            kind: ProblemKind::ReleaseBuild,
            key: "build:a/b:v1.0.0".into(),
            branch: "v1.0.0".into(),
            failing: vec![Check {
                name: "Release / Linux / build".into(),
                url: job(11, 5),
            }],
            ..problem("p1", A)
        });
        s.builds = vec![build(vec![run(11, "Release / Linux", "failure")])];
        record(&mut s);
        close(&mut s.problems[0], 30, A, "Passed.");
        s.builds = vec![b];
        record(&mut s);
        s.cases[0].end
    };
    let docs = run(13, "Docs", "success");
    assert_eq!(
        release(build(vec![
            run(11, "Release / Linux", "success"),
            docs.clone()
        ])),
        End::Again
    );
    assert_eq!(
        release(build(vec![
            run(11, "Release / Linux", "skipped"),
            docs.clone()
        ])),
        End::Unclear
    );
    // Its own workflow's newest run was skipped, and another workflow
    // whose name starts the same passed: that isn't its run passing.
    assert_eq!(
        release(build(vec![
            run(12, "Release / Linux", "skipped"),
            run(14, "Release", "success")
        ])),
        End::Unclear
    );
    assert_eq!(
        release(crate::builds::Build {
            tag: "v1.1.0".into(),
            ..build(vec![run(11, "Release / Linux", "success")])
        }),
        End::Unclear
    );
}

/// GitHub reruns a job as a new attempt in its workflow run, so a check
/// passing counts only in the failed job's own run, and only when it is
/// the one check by its name there.
#[test]
fn a_check_counts_as_passing_again_only_in_the_failed_jobs_own_run() {
    use CiState::{None as Neither, Passing};
    let ending = |checks: Vec<(Check, CiState)>| {
        let mut s = Snapshot::default();
        s.problems.push(Problem {
            failing: vec![Check {
                name: RUST.into(),
                url: job(7, 6),
            }],
            ..problem("p1", A)
        });
        let mut red = red(A, &[]);
        red.head.checks = vec![(
            Check {
                name: RUST.into(),
                url: job(7, 6),
            },
            CiState::Failing,
        )];
        s.ci = vec![red];
        record(&mut s);
        close(&mut s.problems[0], 30, A, "Passed at 1111111.");
        let mut ci = read(A, &[]);
        ci.head.checks = checks;
        s.ci = vec![ci];
        record(&mut s);
        s.cases[0].end
    };
    let at = |name: &str, run: u64, id: u64, state: CiState| {
        (
            Check {
                name: name.into(),
                url: job(run, id),
            },
            state,
        )
    };
    // Its rerun passed.
    assert_eq!(ending(vec![at(RUST, 7, 9, Passing)]), End::Again);
    // Two jobs in its run share its name: one passed, but the rerun was
    // cancelled or skipped, and they can't be told apart.
    assert_eq!(
        ending(vec![at(RUST, 7, 8, Passing), at(RUST, 7, 9, Neither)]),
        End::Unclear
    );
    assert_eq!(
        ending(vec![at(RUST, 7, 8, Passing), at(RUST, 7, 9, Passing)]),
        End::Unclear
    );
    // One by its name passed in another run, and its own didn't.
    assert_eq!(
        ending(vec![at(RUST, 8, 9, Passing), at(RUST, 7, 10, Neither)]),
        End::Unclear
    );
    assert_eq!(ending(vec![at(RUST, 8, 9, Passing)]), End::Unclear);
}

/// GitHub's answer to Orbit's CI query: `main` at `sha` with these checks
/// of the workflow CI, each a job's name, run, id and how it ended.
fn graphql(sha: &str, jobs: &[(&str, u64, u64, &str)]) -> RepoCi {
    let nodes: Vec<serde_json::Value> = jobs
        .iter()
        .map(|(name, run, id, conclusion)| {
            serde_json::json!({
                "__typename": "CheckRun",
                "name": name,
                "status": "COMPLETED",
                "conclusion": conclusion,
                "detailsUrl": job(*run, *id),
                "checkSuite": { "workflowRun": { "workflow": { "name": "CI" } } }
            })
        })
        .collect();
    let v = serde_json::json!({ "data": { "repository": {
        "defaultBranchRef": { "name": "main", "target": {
            "oid": sha,
            "messageHeadline": "Update the build",
            "statusCheckRollup": { "contexts": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": nodes
            } }
        } },
        "pullRequests": { "pageInfo": { "hasNextPage": false }, "nodes": [] }
    } } });
    crate::ci::parse("a/b", &v).unwrap()
}

/// Two jobs failing by one name are kept as one failure, so both have to
/// pass again, read as GitHub sends them: each in its own run, which
/// isn't the case when one's run is gone or a new run replaced it.
#[test]
fn two_jobs_failing_by_one_name_both_have_to_pass_again() {
    let ending = |again: &[(&str, u64, u64, &str)]| {
        let mut s = Snapshot::default();
        let failed = graphql(A, &[("Rust", 7, 70, "FAILURE"), ("Rust", 8, 80, "FAILURE")]);
        see(&mut s, failed, 0);
        assert_eq!(s.problems[0].failing.len(), 1, "kept once, by name");
        assert_eq!(s.cases[0].failed.len(), 2, "both kept on the case");
        see(&mut s, graphql(A, again), 10);
        assert!(!s.problems[0].is_open());
        s.cases[0].end
    };
    let passed = ("Rust", 7, 71, "SUCCESS");
    for second in ["CANCELLED", "SKIPPED"] {
        assert_eq!(ending(&[passed, ("Rust", 8, 81, second)]), End::Unclear);
    }
    // The other run was deleted, or a new run replaced it.
    assert_eq!(ending(&[passed]), End::Unclear);
    assert_eq!(ending(&[passed, ("Rust", 9, 90, "SUCCESS")]), End::Unclear);
    assert_eq!(ending(&[passed, ("Rust", 8, 81, "SUCCESS")]), End::Again);
}

/// One incident on `main` at `A`, read as GitHub sends it: `jobs` fail at
/// `start`, each in a run of its own, and pass again in the same runs ten
/// minutes later.
fn incident(s: &mut Snapshot, jobs: &[&str], start: i64) {
    let base = 1000 * (start as u64 + 1);
    for (later, conclusion) in [(0, "FAILURE"), (10, "SUCCESS")] {
        let read: Vec<(&str, u64, u64, &str)> = (0..)
            .zip(jobs)
            .map(|(n, job)| (*job, base + n, base + 100 + n + later, conclusion))
            .collect();
        see(s, graphql(A, &read), start + later as i64);
    }
}

/// One failure counts once for a lesson, however its jobs are named: by
/// one name in another case, as GitHub tells them apart, or alike in
/// their first 160 characters.
#[test]
fn one_failure_counts_once_however_its_jobs_are_named() {
    let long = "x".repeat(200);
    let (one, two) = (format!("{long} 1"), format!("{long} 2"));
    let mut s = Snapshot::default();
    incident(&mut s, &["Rust", "rust", "RUST", &one, &two], 0);
    let c = &s.cases[0];
    assert_eq!((c.end, c.failed.len()), (End::Again, 5));
    assert_eq!(c.last.len(), 5, "each by its full name");
    assert_eq!(c.last[..3], ["CI / Rust", "CI / rust", "CI / RUST"]);
    let (_, learned) = follow(&mut s, at(80));
    assert!(learned.is_empty() && s.lessons.is_empty());
    // Two more times for one of them, and that one is a lesson.
    incident(&mut s, &["Rust"], 100);
    incident(&mut s, &["Rust"], 200);
    let (_, learned) = follow(&mut s, at(280));
    assert_eq!(ids(&learned), ["luck:a/b:CI / Rust"]);
    assert_eq!(learned[0].count, 3);
    assert_eq!(s.lessons.len(), 1);
    // However often a case names a check, it counts once.
    let mut s = Snapshot::default();
    s.cases.push(Case {
        last: vec![RUST.into(); REPEATS],
        ..case("c1", End::Again, 1)
    });
    assert!(learn(&mut s, now()).is_empty());
}

/// Checks alike in their first 160 characters are still told apart, each
/// by its full name: a lesson counts only its own, and applies only to it.
#[test]
fn checks_alike_in_their_first_160_characters_are_told_apart() {
    let long = |n: u32| format!("{} {n}", "x".repeat(200));
    let mut s = Snapshot::default();
    for (n, start) in [(1, 0), (2, 100), (3, 200)] {
        incident(&mut s, &[&long(n)], start);
    }
    assert!(s.cases.iter().all(|c| c.end == End::Again));
    let (_, learned) = follow(&mut s, at(280));
    assert!(learned.is_empty() && s.lessons.is_empty(), "each once");
    // Two more times for the first, and it is a lesson of its own.
    incident(&mut s, &[&long(1)], 300);
    incident(&mut s, &[&long(1)], 400);
    let (_, learned) = follow(&mut s, at(480));
    assert_eq!(learned.len(), 1);
    let l = &learned[0];
    let name = format!("CI / {}", long(1));
    assert_eq!((l.check.as_str(), l.count), (name.as_str(), 3));
    assert_eq!(l.id, luck_id("A/B", &name));
    assert!(l.id.starts_with("luck:a/b#"), "{}", l.id);
    assert_ne!(l.id, luck_id("a/b", &format!("CI / {}", long(2))));
    assert!(l.id.len() < 140, "a long name's id is its start and a hash");
    assert!(l.what.starts_with(&format!("{}… in a/b", &name[..160])));
    // It applies to that check failing, not to another named alike.
    let failing = |job: &str| Problem {
        failing: vec![check(&format!("CI / {job}"))],
        ..problem(job, A)
    };
    assert!(about(l, &failing(&long(1))));
    assert!(!about(l, &failing(&long(9))));
    // A short, plain name is its own key.
    assert_eq!(luck_id("a/b", RUST), "luck:a/b:CI / Rust");
    assert!(luck_id("a/b", "CI /\nRust").starts_with("luck:a/b#"));
    // A short name that spells a hashed id's end is still its own.
    let hashed = l.id.trim_start_matches("luck:a/b");
    assert_ne!(luck_id("a/b", hashed), l.id);
}

#[cfg(unix)]
fn fake_gh(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let gh = dir.join("gh");
    let script = format!(
        "#!/bin/sh\nexec node {} {} \"$@\"\n",
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/gh_fixture.cjs"),
        dir.display()
    );
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    gh
}

/// How a release build ends when two workflows that name their runs alike
/// (runs 11 and 12) both fail their `build` job at `v1.0.0`, and then
/// GitHub lists `after`, each run's id, workflow and how it ended, read
/// from a fake GitHub as Orbit reads it.
#[cfg(unix)]
async fn two_release_runs(after: &[(u64, u64, &str)]) -> End {
    use serde_json::{json, Value};
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let write = |name: &str, v: Value| std::fs::write(d.path().join(name), v.to_string()).unwrap();
    let run = |id: u64, workflow: u64, conclusion: &str| {
        json!({
            "id": id, "workflow_id": workflow, "name": "Release",
            "head_branch": "v1.0.0", "head_sha": A, "event": "push",
            "status": "completed", "conclusion": conclusion,
            "html_url": format!("https://github.com/a/b/actions/runs/{id}"),
            "created_at": "2026-11-03T11:00:00Z",
        })
    };
    let failed_job = |id: u64, run: u64, conclusion: &str| {
        json!({
            "id": id, "run_id": run, "name": "build", "status": "completed",
            "conclusion": conclusion, "html_url": job(run, id),
        })
    };
    let tag = crate::releases::Tag {
        name: "v1.0.0".into(),
        sha: A.into(),
    };
    let read = || crate::github::GH.scope(gh.clone(), crate::builds::read("a/b", &tag, now()));
    let mut s = Snapshot::default();
    write(
        "queued.json",
        json!([run(11, 1, "failure"), run(12, 2, "failure")]),
    );
    write("job-101.json", failed_job(101, 11, "failure"));
    write("job-102.json", failed_job(102, 12, "failure"));
    let b = read().await.unwrap();
    assert_eq!(b.ci.failing.len(), 1, "kept once, by name");
    crate::problems::see_build(&mut s.problems, &b, at(0));
    s.builds = vec![b];
    follow(&mut s, at(0));
    assert_eq!(s.cases[0].failed.len(), 2, "both runs kept on the case");
    let after: Vec<Value> = after
        .iter()
        .map(|(id, workflow, conclusion)| run(*id, *workflow, conclusion))
        .collect();
    write("queued.json", json!(after));
    let b = read().await.unwrap();
    crate::problems::see_build(&mut s.problems, &b, at(10));
    s.builds = vec![b];
    follow(&mut s, at(10));
    assert!(!s.problems[0].is_open());
    s.cases[0].end
}

#[cfg(unix)]
#[tokio::test]
async fn two_release_runs_failing_by_one_name_both_have_to_pass_again() {
    let passed = (11, 1, "success");
    assert_eq!(
        two_release_runs(&[passed, (12, 2, "skipped")]).await,
        End::Unclear
    );
    // The other run was deleted, or a newer one in its workflow replaced
    // it: that one passing isn't the failed run passing again.
    assert_eq!(two_release_runs(&[passed]).await, End::Unclear);
    assert_eq!(
        two_release_runs(&[passed, (12, 2, "failure"), (13, 2, "success")]).await,
        End::Unclear
    );
    assert_eq!(
        two_release_runs(&[passed, (12, 2, "success")]).await,
        End::Again
    );
}

/// Orbit says it reran a check only when it did: one it found already
/// rerun may have been someone else's.
#[test]
fn only_orbits_own_reruns_are_its() {
    let mut s = Snapshot::default();
    s.problems.push(problem("p1", A));
    let mut t = thread("p1", A);
    t.items[0].reruns.push(Rerun {
        done: true,
        already: true,
        ..Default::default()
    });
    s.ops.push(t);
    s.ci = vec![red(A, &[RUST])];
    record(&mut s);
    assert!(!s.cases[0].reran);
    close(&mut s.problems[0], 30, A, "Passed at 1111111.");
    s.ci = vec![read(A, &[(RUST, CiState::Passing)])];
    record(&mut s);
    let c = s.cases[0].clone();
    assert_eq!((c.end, c.reran), (End::Again, false));
    let text = last_time_text(
        &LastTime {
            case: c,
            at: stamp(at(90)),
            text: String::new(),
        },
        &str::to_string,
    );
    assert!(text.ends_with("It passed when it ran again at the same commit, 30 minutes later."));
}

/// A case counts only once its problem can't come back as the same one,
/// which would undo how it ended.
#[test]
fn a_case_counts_only_once_its_problem_cant_come_back() {
    let mut s = Snapshot::default();
    s.cases.push(case("c1", End::Again, 3));
    s.cases.push(case("c2", End::Again, 2));
    // It fails, and passes when it runs again at the same commit.
    see(&mut s, red(A, &[RUST]), 0);
    see(&mut s, read(A, &[(RUST, CiState::Passing)]), 10);
    assert_eq!(s.cases[2].end, End::Again);
    assert!(s.lessons.is_empty(), "it may still come back");
    // It comes back within the hour: it wasn't bad luck after all.
    see(&mut s, red(A, &[RUST]), 30);
    assert_eq!(s.cases[2].end, End::Open);
    see(&mut s, red(A, &[RUST]), 120);
    assert!(s.lessons.is_empty());
    // It passes for good, and once the hour is up it counts.
    see(&mut s, read(A, &[(RUST, CiState::Passing)]), 130);
    assert!(see(&mut s, read(A, &[(RUST, CiState::Passing)]), 189)
        .1
        .is_empty());
    let (_, learned) = see(&mut s, read(A, &[(RUST, CiState::Passing)]), 191);
    assert_eq!(ids(&learned), ["luck:a/b:CI / Rust"]);
    assert_eq!(learned[0].count, 3);
}

/// Orbit keeps at most so many lessons. One with no room is let go as if
/// you forgot it, so the same repeats never tell of it again.
#[test]
fn lessons_past_the_most_kept_are_let_go_and_not_learned_again() {
    let mut s = Snapshot::default();
    let names: Vec<String> = (0..MAX_LESSONS + 2)
        .map(|n| format!("check {n:02}"))
        .collect();
    for (id, days) in [("c1", 3), ("c2", 2), ("c3", 1)] {
        s.cases.push(Case {
            last: names.clone(),
            checks: names.clone(),
            ..case(id, End::Again, days)
        });
    }
    let learned = learn(&mut s, now());
    assert_eq!(learned.len(), MAX_LESSONS);
    assert_eq!(s.lessons.len(), MAX_LESSONS);
    // All were seen at once, so the last by name have no room.
    let gone: Vec<String> = s.forgotten_lessons.iter().map(|f| f.id.clone()).collect();
    assert_eq!(gone, ["luck:a/b:check 50", "luck:a/b:check 51"]);
    assert!(learned.iter().all(|l| !gone.contains(&l.id)));
    // Learning and pruning again, nothing new and nothing let go.
    for minutes in [60, 120, 180] {
        assert!(learn(&mut s, at(minutes)).is_empty());
        assert!(!prune(&mut s, at(minutes)));
        assert_eq!(s.lessons.len(), MAX_LESSONS);
        assert_eq!(s.forgotten_lessons.len(), 2);
    }
    // A newer repeat takes the place of one seen longest ago.
    for (id, minutes) in [("n1", -300), ("n2", -200), ("n3", -100)] {
        s.cases.push(Case {
            last: vec!["new".into()],
            closed_at: Some(stamp(at(minutes))),
            ..case(id, End::Again, 0)
        });
    }
    let learned = learn(&mut s, now());
    assert_eq!(ids(&learned), ["luck:a/b:new"]);
    assert_eq!(s.lessons.len(), MAX_LESSONS);
    assert_eq!(s.lessons[0].id, "luck:a/b:new");
    assert!(s
        .forgotten_lessons
        .iter()
        .any(|f| f.id == "luck:a/b:check 49"));
    assert!(learn(&mut s, at(60)).is_empty());
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
    // What failed there isn't carried along.
    s.cases.push(Case {
        failed: vec![check(RUST)],
        unsure: true,
        ..case("old", End::Passed, 9)
    });
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
    let l = s.problems[0].last_time.as_ref().unwrap();
    assert_eq!((l.case.failed.len(), l.case.unsure), (0, false));
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
    assert_eq!(ids(&learned), ["luck:a/b:CI / Rust"]);
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
    assert_eq!(ids(&s.problems[0].lessons), ["luck:a/b:CI / Rust"]);
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
    assert!(forget(&mut s, "luck:a/b:CI / Rust", now()));
    assert!(s.lessons.is_empty());
    assert!(s.problems[0].lessons.is_empty());
    assert_eq!(
        s.forgotten_lessons,
        [Forgot {
            id: "luck:a/b:CI / Rust".into(),
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
    assert!(learn(&mut s, at(120)).is_empty());
    s.cases.push(Case {
        closed_at: Some(stamp(at(30))),
        ..case("c6", End::Again, 0)
    });
    // Each counts once its hour to come back is up.
    assert!(learn(&mut s, at(60)).is_empty());
    let learned = learn(&mut s, at(90));
    assert_eq!(ids(&learned), ["luck:a/b:CI / Rust"]);
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
    // The jobs a case saw fail go once its problem can't come back.
    let failed = vec![check(RUST)];
    for c in &mut s.cases {
        c.failed = failed.clone();
        c.unsure = true;
    }
    s.cases.push(Case {
        failed: failed.clone(),
        closed_at: Some(stamp(at(-30))),
        ..case("settling", End::Again, 0)
    });
    assert!(prune(&mut s, now()));
    let kept = |id: &str| {
        let c = s.cases.iter().find(|c| c.id == id).unwrap();
        (c.failed.len(), c.unsure)
    };
    assert_eq!(
        (kept("open"), kept("kept"), kept("settling")),
        ((1, true), (0, false), (1, false))
    );
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
