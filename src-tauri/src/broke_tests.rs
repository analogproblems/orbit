use super::*;
use chrono::TimeZone;
use serde_json::json;

fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap()
}
/// A commit as GitHub's history gives it: its first parent, one check
/// with this conclusion (or none), and the pull requests it came with.
fn node(at: char, parent: Option<char>, check: Option<&str>, pulls: Value) -> Value {
    let checks: Vec<Value> = check
        .into_iter()
        .map(|conclusion| {
            json!({
                "__typename": "CheckRun",
                "name": "Rust",
                "status": "COMPLETED",
                "conclusion": conclusion,
                "detailsUrl": "",
                "checkSuite": { "workflowRun": { "workflow": { "name": "CI" } } }
            })
        })
        .collect();
    json!({
        "oid": sha(at),
        "parents": { "nodes": parent.map(|p| vec![json!({ "oid": sha(p) })]).unwrap_or_default() },
        "statusCheckRollup": { "contexts": {
            "pageInfo": { "hasNextPage": false, "endCursor": null },
            "nodes": checks
        } },
        "associatedPullRequests": { "nodes": pulls }
    })
}
fn merged(number: u64) -> Value {
    json!([{ "number": number, "merged": true, "baseRefName": "main" }])
}
fn none() -> Value {
    json!([])
}
fn answer(nodes: Vec<Value>) -> Value {
    json!({ "data": { "repository": { "object": { "history": { "nodes": nodes } } } } })
}
fn history(nodes: Vec<Value>) -> HashMap<String, Seen> {
    commits(&answer(nodes), "main").unwrap().unwrap()
}
fn found(start: char, nodes: Vec<Value>) -> Broke {
    match walk(&sha(start), &history(nodes)) {
        Walked::Found(b) => b,
        other => panic!("{other:?}"),
    }
}

/// One merge between the last green commit and the first red one names
/// its pull request.
#[test]
fn one_merge_names_its_pull_request() {
    let b = found(
        '3',
        vec![
            node('3', Some('2'), Some("FAILURE"), none()),
            node('2', Some('1'), Some("FAILURE"), merged(42)),
            node('1', Some('0'), Some("SUCCESS"), merged(41)),
        ],
    );
    assert_eq!(b.green, sha('1'));
    assert_eq!(b.red, sha('2'));
    assert_eq!(b.commits, [sha('2')]);
    assert_eq!(b.pull, Some(42));
    assert_eq!(
        text(&b).as_deref(),
        Some("It started failing with #42, merged as 2222222; 1111111 before it passed.")
    );
    assert_eq!(
        markdown(&b).as_deref(),
        Some("It started failing with #42, merged as `2222222`; `1111111` before it passed.")
    );
}

/// A commit whose checks didn't run or finish between the green one and
/// the first red one may have broken it too; commits after the first red
/// one didn't.
#[test]
fn every_commit_that_may_have_broken_it_counts() {
    let b = found(
        '5',
        vec![
            node('5', Some('4'), Some("FAILURE"), merged(45)),
            node('4', Some('3'), None, merged(44)),
            node('3', Some('2'), Some("FAILURE"), merged(43)),
            node('2', Some('1'), Some("CANCELLED"), none()),
            node('1', Some('0'), Some("SUCCESS"), none()),
        ],
    );
    assert_eq!(b.red, sha('3'));
    assert_eq!(b.commits, [sha('3'), sha('2')]);
    assert_eq!((b.pulls.clone(), b.direct, b.pull), (vec![43], 1, None));
    assert_eq!(
        text(&b).as_deref(),
        Some(
            "It started failing in the 2 commits after 1111111, which passed, up to 3333333: from #43 and 1 pushed without a pull request."
        )
    );
    let two = found(
        '3',
        vec![
            node('3', Some('2'), Some("FAILURE"), merged(43)),
            node('2', Some('1'), None, merged(42)),
            node('1', Some('0'), Some("SUCCESS"), none()),
        ],
    );
    assert_eq!((two.pulls.clone(), two.pull), (vec![43, 42], None));
    assert_eq!(
        text(&two).as_deref(),
        Some("It started failing in the 2 commits after 1111111, which passed, up to 3333333: from #43 and #42.")
    );
}

/// A pull request rebased onto the branch lands as several commits, and
/// one pushed without a pull request names none.
#[test]
fn a_rebased_pull_request_and_a_push() {
    let b = found(
        '3',
        vec![
            node('3', Some('2'), Some("FAILURE"), merged(42)),
            node('2', Some('1'), None, merged(42)),
            node('1', Some('0'), Some("SUCCESS"), none()),
        ],
    );
    assert_eq!(b.pull, Some(42));
    assert_eq!(
        text(&b).as_deref(),
        Some("It started failing with #42, merged as 2 commits up to 3333333; 1111111 before them passed.")
    );
    let push = found(
        '2',
        vec![
            node('2', Some('1'), Some("FAILURE"), none()),
            node('1', Some('0'), Some("SUCCESS"), none()),
        ],
    );
    assert_eq!((push.pull, push.direct), (None, 1));
    assert_eq!(
        text(&push).as_deref(),
        Some("It started failing at 2222222, pushed without a pull request; 1111111 before it passed.")
    );
    let pushes = Broke {
        direct: 2,
        commits: vec![sha('3'), sha('2')],
        ..push
    };
    assert!(text(&pushes)
        .unwrap()
        .ends_with("from 2 pushed without one."));
}

/// Only first parents are the branch's own commits: a merged branch's
/// commits, green or not, aren't where it broke.
#[test]
fn only_first_parents_count() {
    let b = found(
        'e',
        vec![
            node('e', Some('1'), Some("FAILURE"), merged(42)),
            // The pull request's head, which passed on the pull request.
            node('f', Some('1'), Some("SUCCESS"), merged(42)),
            node('1', Some('0'), Some("SUCCESS"), none()),
        ],
    );
    assert_eq!(
        (b.green, b.commits, b.pull),
        (sha('1'), vec![sha('e')], Some(42))
    );
}

/// A pull request that isn't merged, or merged into another branch,
/// didn't bring a commit to the default branch.
#[test]
fn only_a_pull_request_merged_here_counts() {
    for pulls in [
        json!([{ "number": 7, "merged": false, "baseRefName": "main" }]),
        json!([{ "number": 7, "merged": true, "baseRefName": "release" }]),
    ] {
        let b = found(
            '2',
            vec![
                node('2', Some('1'), Some("FAILURE"), pulls),
                node('1', Some('0'), Some("SUCCESS"), none()),
            ],
        );
        assert_eq!((b.pull, b.direct), (None, 1));
    }
}

/// A commit whose checks aren't all read, or that only Orbit's own status
/// marks, never counts as green or red.
#[test]
fn a_commit_counts_only_by_checks_orbit_read() {
    let mut partial = node('2', Some('1'), Some("SUCCESS"), merged(42));
    partial["statusCheckRollup"]["contexts"]["pageInfo"] =
        json!({ "hasNextPage": true, "endCursor": "x" });
    let mut own = node('3', Some('2'), None, merged(43));
    own["statusCheckRollup"]["contexts"]["nodes"] = json!([{ "__typename": "StatusContext", "context": "orbit/review", "state": "FAILURE", "targetUrl": "" }]);
    let b = found(
        '4',
        vec![
            node('4', Some('3'), Some("FAILURE"), merged(44)),
            own,
            partial,
            node('1', Some('0'), Some("SUCCESS"), none()),
        ],
    );
    // Neither is green, nor is the one only Orbit marks red: both may
    // have broken it.
    assert_eq!(b.green, sha('1'));
    assert_eq!(b.red, sha('4'));
    assert_eq!(b.commits, [sha('4'), sha('3'), sha('2')]);
}

/// When the history read so far doesn't go back far enough, Orbit reads
/// on from where it stopped; with no green commit within `BACK`, or before
/// the branch began, it says so and names nothing.
#[test]
fn how_far_back_orbit_looks() {
    assert_eq!(walk(&sha('9'), &history(vec![])), Walked::Need(sha('9')));
    assert_eq!(
        walk(
            &sha('2'),
            &history(vec![node('2', Some('1'), Some("FAILURE"), none())])
        ),
        Walked::Need(sha('1'))
    );
    // A chain longer than BACK, all failing.
    let ids: Vec<String> = (0..=BACK + 2).map(|i| format!("{i:040x}")).collect();
    let mut chain = vec![];
    for i in (1..ids.len()).rev() {
        let mut n = node('0', None, Some("FAILURE"), none());
        n["oid"] = json!(ids[i]);
        n["parents"]["nodes"] = json!([{ "oid": ids[i - 1] }]);
        chain.push(n);
    }
    let start = ids.last().unwrap();
    let Walked::Found(b) = walk(start, &history(chain)) else {
        panic!()
    };
    assert!(b.done && b.commits.is_empty() && b.pull.is_none());
    assert_eq!(
        b.error.as_deref(),
        Some(
            format!(
                "Orbit found no commit that passed in the 20 before {}.",
                &start[..7]
            )
            .as_str()
        )
    );
    assert_eq!(text(&b), None);
    let first = found('1', vec![node('1', None, Some("FAILURE"), none())]);
    assert!(first.commits.is_empty() && first.error.is_some());
    // GitHub has it passing now, such as after a rerun.
    let passing = found('1', vec![node('1', Some('0'), Some("SUCCESS"), none())]);
    assert!(passing.commits.is_empty());
    assert_eq!(
        passing.error.as_deref(),
        Some("GitHub no longer has these commits failing.")
    );
}

/// What GitHub's answer must hold.
#[test]
fn githubs_answer() {
    assert_eq!(
        commits(
            &json!({ "data": { "repository": { "object": null } } }),
            "main"
        ),
        Ok(None)
    );
    assert!(commits(&json!({ "errors": [{ "message": "Nope" }] }), "main").is_err());
    assert!(commits(
        &json!({ "data": { "repository": { "object": {} } } }),
        "main"
    )
    .is_err());
    let mut bad = node('1', None, None, none());
    bad["oid"] = json!("not a sha");
    assert!(commits(&answer(vec![bad]), "main").is_err());
    assert!(query().contains("history(first: 40)"));
    assert!(query().contains("parents(first: 1)"));
}

fn problem(id: &str, kind: ProblemKind) -> Problem {
    Problem {
        id: id.into(),
        kind,
        repo: "a/b".into(),
        branch: "main".into(),
        sha: sha('3'),
        since_sha: sha('2'),
        ..Default::default()
    }
}

/// Which problems Orbit reads the history of: open default branches it
/// hasn't an answer for, those not tried first, a few a pass.
#[test]
fn the_problems_orbit_reads() {
    let mut ps = vec![
        problem("a", ProblemKind::DefaultBranch),
        problem("pr", ProblemKind::PullRequest),
        problem("b", ProblemKind::DefaultBranch),
        problem("c", ProblemKind::DefaultBranch),
    ];
    let ids =
        |ps: &[Problem], now| -> Vec<String> { wanted(ps, now).into_iter().map(|w| w.0).collect() };
    assert_eq!(ids(&ps, t0()), ["a", "b"]);
    assert_eq!(
        wanted(&ps, t0())[0],
        ("a".into(), "a/b".into(), "main".into(), sha('2'))
    );
    let earlier = t0() - Duration::minutes(5);
    record(
        &mut ps,
        "a",
        &sha('2'),
        Err("gh: Bad Gateway (HTTP 502)".into()),
        earlier,
    );
    assert_eq!(ids(&ps, t0()), ["b", "c"], "those not tried first");
    record(
        &mut ps,
        "b",
        &sha('2'),
        Ok(Broke {
            done: true,
            ..Default::default()
        }),
        t0(),
    );
    record(
        &mut ps,
        "c",
        &sha('2'),
        Ok(Broke {
            done: true,
            ..Default::default()
        }),
        t0(),
    );
    assert_eq!(ids(&ps, t0()), ["a"]);
    record(
        &mut ps,
        "a",
        &sha('2'),
        Err("gh: Bad Gateway (HTTP 502)".into()),
        t0(),
    );
    assert!(ids(&ps, t0()).is_empty(), "a waits");
    assert_eq!(ids(&ps, t0() + Duration::minutes(2)), ["a"]);
    ps[0].closed_at = Some(stamp(t0()));
    assert!(ids(&ps, t0() + Duration::hours(2)).is_empty());
    let mut no_branch = problem("d", ProblemKind::DefaultBranch);
    no_branch.branch.clear();
    assert!(ids(&[no_branch], t0()).is_empty());
}

/// A history GitHub didn't give waits longer each time.
#[test]
fn what_orbit_records() {
    let mut ps = vec![problem("a", ProblemKind::DefaultBranch)];
    assert!(record(
        &mut ps,
        "a",
        &sha('2'),
        Err("gh: Bad Gateway (HTTP 502)".into()),
        t0()
    ));
    let b = ps[0].broke.clone().unwrap();
    assert!(!b.done);
    assert_eq!(b.tries, 1);
    assert_eq!(b.retry_at, Some(stamp(t0() + Duration::minutes(1))));
    assert_eq!(
        b.error.as_deref(),
        Some("Orbit couldn't read the branch's history yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    record(
        &mut ps,
        "a",
        &sha('2'),
        Err("gh: Bad Gateway (HTTP 502)".into()),
        t0(),
    );
    assert_eq!(
        ps[0].broke.as_ref().unwrap().retry_at,
        Some(stamp(t0() + Duration::minutes(2)))
    );
    let one = Broke {
        done: true,
        green: sha('1'),
        red: sha('2'),
        commits: vec![sha('2')],
        pulls: vec![42],
        pull: Some(42),
        ..Default::default()
    };
    record(&mut ps, "a", &sha('2'), Ok(one.clone()), t0());
    assert_eq!(ps[0].broke, Some(one.clone()));
    // It passed and failed again from another commit while Orbit read.
    ps[0].since_sha = sha('4');
    ps[0].broke = None;
    assert!(!record(&mut ps, "a", &sha('2'), Ok(one.clone()), t0()));
    assert_eq!(ps[0].broke, None);
    assert!(record(&mut ps, "a", &sha('4'), Ok(one.clone()), t0()));
    ps[0].closed_at = Some(stamp(t0()));
    assert!(!record(&mut ps, "a", &sha('4'), Ok(Broke::default()), t0()));
    assert_eq!(ps[0].broke, Some(one));
}
