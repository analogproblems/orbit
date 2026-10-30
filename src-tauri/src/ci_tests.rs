use super::*;
use serde_json::json;

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";

fn run(workflow: &str, name: &str, status: &str, conclusion: Option<&str>) -> Value {
    json!({
        "__typename": "CheckRun",
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "detailsUrl": format!("https://github.com/o/r/actions/runs/1/job/{name}"),
        "checkSuite": { "workflowRun": { "workflow": { "name": workflow } } }
    })
}
fn status(context: &str, state: &str) -> Value {
    json!({
        "__typename": "StatusContext",
        "context": context,
        "state": state,
        "targetUrl": "https://example.com/status"
    })
}
fn commit(sha: &str, contexts: Vec<Value>) -> Value {
    json!({
        "oid": sha,
        "messageHeadline": "Fix the build",
        "statusCheckRollup": { "contexts": { "nodes": contexts } }
    })
}
fn answer(head: Value, pulls: Vec<Value>, more: bool) -> Value {
    json!({ "data": { "repository": {
        "defaultBranchRef": { "name": "main", "target": head },
        "pullRequests": { "pageInfo": { "hasNextPage": more }, "nodes": pulls }
    } } })
}

#[test]
fn reads_the_default_branch_and_each_pull_request() {
    let v = answer(
        commit(
            A,
            vec![
                run("CI", "Rust", "COMPLETED", Some("FAILURE")),
                run("CI", "Frontend", "COMPLETED", Some("SUCCESS")),
            ],
        ),
        vec![json!({
            "number": 7,
            "isDraft": true,
            "headRefOid": B,
            "commits": { "nodes": [ { "commit": commit(B, vec![run("CI", "Rust", "IN_PROGRESS", None)]) } ] }
        })],
        false,
    );
    let ci = parse("o/r", &v).unwrap();
    assert_eq!(ci.branch, "main");
    assert_eq!(ci.title, "Fix the build");
    assert_eq!(ci.head.sha, A);
    assert_eq!(ci.head.state, CiState::Failing);
    assert_eq!(ci.head.failing[0].name, "CI / Rust");
    assert!(ci.head.failing[0].url.ends_with("/job/Rust"));
    assert_eq!(ci.head.passing, 1);
    assert!(ci.complete);
    let pull = ci.pull(7).unwrap();
    assert!(pull.draft);
    assert_eq!(pull.ci.state, CiState::Running);
}

#[test]
fn leaves_out_orbits_own_status() {
    let v = answer(
        commit(
            A,
            vec![
                status(OWN_STATUS, "FAILURE"),
                status("ci/legacy", "SUCCESS"),
            ],
        ),
        vec![],
        false,
    );
    let ci = parse("o/r", &v).unwrap();
    assert_eq!(ci.head.state, CiState::Passing);
    assert!(ci.head.failing.is_empty());
}

#[test]
fn a_failure_counts_while_others_still_run_and_skips_count_for_nothing() {
    let head = commit(
        A,
        vec![
            run("CI", "Lint", "QUEUED", None),
            run("CI", "Test", "COMPLETED", Some("TIMED_OUT")),
            run("CI", "Deploy", "COMPLETED", Some("SKIPPED")),
            status("legacy", "ERROR"),
        ],
    );
    let ci = parse("o/r", &answer(head, vec![], false)).unwrap();
    assert_eq!(ci.head.state, CiState::Failing);
    let names: Vec<&str> = ci.head.failing.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["CI / Test", "legacy"]);
    assert_eq!(ci.head.running, 1);
    // Only skipped or cancelled checks: neither way.
    let head = commit(A, vec![run("CI", "Deploy", "COMPLETED", Some("CANCELLED"))]);
    let ci = parse("o/r", &answer(head, vec![], false)).unwrap();
    assert_eq!(ci.head.state, CiState::None);
}

#[test]
fn says_when_the_list_of_pull_requests_is_cut_short() {
    let ci = parse("o/r", &answer(commit(A, vec![]), vec![], true)).unwrap();
    assert!(!ci.complete);
    assert_eq!(ci.head.state, CiState::None);
}

#[test]
fn an_empty_repository_has_nothing_to_check() {
    let v = json!({ "data": { "repository": {
        "defaultBranchRef": null,
        "pullRequests": { "pageInfo": { "hasNextPage": false }, "nodes": [] }
    } } });
    let ci = parse("o/r", &v).unwrap();
    assert!(ci.head.sha.is_empty());
}

#[test]
fn reports_what_github_refused() {
    let v = json!({ "errors": [ { "message": "Could not resolve to a Repository with the name 'o/r'." } ] });
    assert!(parse("o/r", &v).unwrap_err().contains("Could not resolve"));
    let v = json!({ "data": { "repository": null } });
    assert!(parse("o/r", &v).is_err());
    let v = answer(commit("not-a-sha", vec![]), vec![], false);
    assert!(parse("o/r", &v).is_err());
}

/// A commit with one page of these checks, and `next` where the next page
/// starts, if there is one.
fn paged(sha: &str, contexts: Vec<Value>, next: Option<&str>) -> Value {
    json!({
        "oid": sha,
        "messageHeadline": "Fix the build",
        "statusCheckRollup": { "contexts": {
            "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
            "nodes": contexts
        } }
    })
}
fn passes(n: usize) -> Vec<Value> {
    (0..n)
        .map(|i| run("CI", &format!("Job {i}"), "COMPLETED", Some("SUCCESS")))
        .collect()
}

#[test]
fn checks_left_to_read_keep_a_commit_from_passing() {
    let v = answer(paged(A, passes(100), Some("page-2")), vec![], false);
    let ci = parse("o/r", &v).unwrap();
    assert_eq!(ci.head.cursor.as_deref(), Some("page-2"));
    assert_eq!(ci.head.state, CiState::Running);
    // A failure on the first page fails it already.
    let mut first = passes(99);
    first.push(run("CI", "Rust", "COMPLETED", Some("FAILURE")));
    let v = answer(paged(A, first, Some("page-2")), vec![], false);
    assert_eq!(parse("o/r", &v).unwrap().head.state, CiState::Failing);
    // More, but nowhere to read them from.
    let mut v = answer(paged(A, passes(100), Some("x")), vec![], false);
    v["data"]["repository"]["defaultBranchRef"]["target"]["statusCheckRollup"]["contexts"]
        ["pageInfo"]["endCursor"] = Value::Null;
    let ci = parse("o/r", &v).unwrap();
    assert!(ci.head.incomplete);
    assert_eq!(ci.head.state, CiState::Running);
}

/// A gh that answers the first request with `first.json` and each request
/// for more checks with `more.json`, counting those in `asked`.
#[cfg(unix)]
fn fake_gh(dir: &std::path::Path, first: &Value, more: &Value) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(dir.join("first.json"), first.to_string()).unwrap();
    std::fs::write(dir.join("more.json"), more.to_string()).unwrap();
    let gh = dir.join("gh");
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in\n*cursor=*) echo x >> {0}/asked; cat {0}/more.json ;;\n*) cat {0}/first.json ;;\nesac\n",
        dir.display()
    );
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    gh
}
#[cfg(unix)]
fn more(contexts: Vec<Value>, next: Option<&str>) -> Value {
    json!({ "data": { "repository": { "object": {
        "statusCheckRollup": { "contexts": {
            "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
            "nodes": contexts
        } }
    } } } })
}
#[cfg(unix)]
fn asked(dir: &std::path::Path) -> usize {
    std::fs::read_to_string(dir.join("asked"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[cfg(unix)]
#[tokio::test]
async fn a_failure_past_the_first_hundred_checks_is_found() {
    let d = tempfile::tempdir().unwrap();
    let first = answer(paged(A, passes(100), Some("page-2")), vec![], false);
    let gh = fake_gh(
        d.path(),
        &first,
        &more(vec![run("CI", "Rust", "COMPLETED", Some("FAILURE"))], None),
    );
    let ci = crate::github::GH.scope(gh, read("o/r")).await.unwrap();
    assert_eq!(ci.head.state, CiState::Failing);
    assert_eq!(ci.head.failing[0].name, "CI / Rust");
    assert_eq!(ci.head.passing, 100);
    assert!(ci.head.cursor.is_none());
    assert_eq!(asked(d.path()), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn all_pages_passing_pass_and_too_many_pages_never_do() {
    let d = tempfile::tempdir().unwrap();
    let first = answer(paged(A, passes(100), Some("page-2")), vec![], false);
    let gh = fake_gh(d.path(), &first, &more(passes(3), None));
    let ci = crate::github::GH.scope(gh, read("o/r")).await.unwrap();
    assert_eq!(ci.head.state, CiState::Passing);
    assert_eq!(ci.head.passing, 103);
    // Page after page of passes: Orbit stops reading, and doesn't pass it.
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path(), &first, &more(passes(100), Some("again")));
    let ci = crate::github::GH.scope(gh, read("o/r")).await.unwrap();
    assert!(ci.head.incomplete);
    assert_eq!(ci.head.state, CiState::Running);
    assert_eq!(asked(d.path()), MORE_PAGES);
    // A commit GitHub no longer shows the rest of.
    let d = tempfile::tempdir().unwrap();
    let gone = json!({ "data": { "repository": { "object": null } } });
    let gh = fake_gh(d.path(), &first, &gone);
    let ci = crate::github::GH.scope(gh, read("o/r")).await.unwrap();
    assert!(ci.head.incomplete);
    assert_eq!(ci.head.state, CiState::Running);
}

#[test]
fn the_query_reads_every_part_parse_needs() {
    let q = query();
    for part in [
        "defaultBranchRef",
        "messageHeadline",
        "hasNextPage",
        "isDraft",
        "headRefOid",
        "statusCheckRollup",
        "... on CheckRun",
        "... on StatusContext",
        "endCursor",
    ] {
        assert!(q.contains(part), "{part}");
    }
    let more = more_query();
    for part in [
        "$cursor",
        "after: $cursor",
        "object(oid: $oid)",
        "endCursor",
    ] {
        assert!(more.contains(part), "{part}");
    }
    // Balanced braces, or GitHub refuses it.
    for q in [q, more] {
        assert_eq!(q.matches('{').count(), q.matches('}').count());
    }
}

/// When a commit's passing checks began, the earliest, and when the
/// default branch's latest commit was made: what tells Orbit whether a
/// pull request's checks ran with its base's latest.
#[test]
fn when_the_checks_that_passed_began() {
    let timed = |mut v: Value, at: &str| {
        match v["__typename"].as_str() {
            Some("CheckRun") => v["checkSuite"]["createdAt"] = json!(at),
            _ => v["createdAt"] = json!(at),
        }
        v
    };
    let mut head = commit(
        A,
        vec![
            timed(
                run("CI", "Lint", "COMPLETED", Some("SUCCESS")),
                "2026-10-30T10:05:00Z",
            ),
            timed(
                run("CI", "Test", "COMPLETED", Some("SUCCESS")),
                "2026-10-30T10:01:00Z",
            ),
            timed(status("legacy", "SUCCESS"), "2026-10-30T10:03:00Z"),
            // Skipped: it didn't pass, so it doesn't count.
            timed(
                run("CI", "Deploy", "COMPLETED", Some("SKIPPED")),
                "2026-10-30T09:00:00Z",
            ),
        ],
    );
    head["committedDate"] = json!("2026-10-30T09:30:00Z");
    let ci = parse("o/r", &answer(head.clone(), vec![], false)).unwrap();
    assert_eq!(ci.head.state, CiState::Passing);
    assert_eq!(ci.head.ran_at, "2026-10-30T10:01:00.000Z");
    assert_eq!(ci.head.committed_at, "2026-10-30T09:30:00Z");
    // One that passed without saying when it began: Orbit can't tell.
    head["statusCheckRollup"]["contexts"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(run("CI", "Docs", "COMPLETED", Some("SUCCESS")));
    let ci = parse("o/r", &answer(head, vec![], false)).unwrap();
    assert_eq!(ci.head.state, CiState::Passing);
    assert_eq!(ci.head.ran_at, "");
    assert!(query().contains("committedDate"));
    assert!(NODES.contains("checkSuite { createdAt"));
    assert!(NODES.contains("targetUrl createdAt"));
}
