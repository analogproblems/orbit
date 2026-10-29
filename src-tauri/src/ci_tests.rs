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
    ] {
        assert!(q.contains(part), "{part}");
    }
    // Balanced braces, or GitHub refuses it.
    assert_eq!(q.matches('{').count(), q.matches('}').count());
}
