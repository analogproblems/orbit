use super::Engine;
use crate::buddy::NoteKind;
use crate::types::*;
use serde_json::json;
use std::path::Path;

/// GitHub's answer for `a/b`, whose default branch's one check is `result`.
fn answer(sha: &str, result: &str) -> String {
    checks(sha, &[("Rust", result)])
}
/// GitHub's answer for `a/b`, with these checks, by job and result, on its
/// default branch.
fn checks(sha: &str, checks: &[(&str, &str)]) -> String {
    let nodes: Vec<_> = checks
        .iter()
        .map(|(job, result)| {
            json!({
                "__typename": "CheckRun",
                "name": job,
                "status": "COMPLETED",
                "conclusion": result,
                "detailsUrl": "https://github.com/a/b/actions/runs/1/job/2",
                "checkSuite": { "workflowRun": { "workflow": { "name": "CI" } } }
            })
        })
        .collect();
    json!({ "data": { "repository": {
        "defaultBranchRef": { "name": "main", "target": {
            "oid": sha,
            "messageHeadline": "Update the build",
            "statusCheckRollup": { "contexts": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": nodes
            } }
        } },
        "pullRequests": { "pageInfo": { "hasNextPage": false }, "nodes": [] }
    } } })
    .to_string()
}
/// A gh that answers with `answer.json`, or fails while `down` exists. It
/// leaves `asked` when called, and waits while `hold` exists.
#[cfg(unix)]
fn fake_gh(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let gh = dir.join("gh");
    let script = format!(
        "#!/bin/sh\ntouch {0}/asked\nwhile [ -e {0}/hold ]; do sleep 0.02; done\nif [ -e {0}/down ]; then echo 'HTTP 502: Bad Gateway' >&2; exit 1; fi\ncat {0}/answer.json\n",
        dir.display()
    );
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    gh
}
async fn connected(dir: &Path) -> std::sync::Arc<Engine> {
    let e = Engine::new(dir.join("state"), |_| {}).unwrap();
    e.update(|s| {
        if s.settings.repositories.is_empty() {
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
        }
    })
    .await;
    e
}
async fn heads_ups(e: &Engine) -> usize {
    let s = e.snapshot().await;
    s.buddy_feed
        .notes
        .iter()
        .filter(|n| n.kind == NoteKind::NeedsYou && n.text.contains("CI is failing"))
        .count()
}

/// A red default branch is one Needs you item, told once even across a
/// restart; a failed read changes nothing; green closes it.
#[cfg(unix)]
#[tokio::test]
async fn a_red_default_branch_needs_you_once_and_green_closes_it() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let red = "1".repeat(40);
    std::fs::write(d.path().join("answer.json"), answer(&red, "FAILURE")).unwrap();
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.ci.len(), 1);
    assert_eq!(shown.ci[0].head.failing[0].name, "CI / Rust");
    assert_eq!(shown.problems.len(), 1);
    assert!(shown.problems[0].is_open());
    assert_eq!(shown.attention.len(), 1);
    assert_eq!(shown.attention[0].kind, AttentionKind::CiFailing);
    assert_eq!(
        shown.attention[0].reason,
        "CI is failing on main in a/b since 1111111: CI / Rust."
    );
    assert_eq!(heads_ups(&e).await, 1);

    // Orbit restarts and CI is still red: nothing new to say.
    drop(e);
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(heads_ups(&e).await, 1);
    assert_eq!(e.snapshot().await.problems.len(), 1);

    // GitHub doesn't answer: what was read is kept, with why.
    std::fs::write(d.path().join("down"), "").unwrap();
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let s = e.snapshot().await;
    assert!(s.ci[0].error.as_deref().unwrap().contains("Bad Gateway"));
    assert_eq!(s.ci[0].head.sha, red);
    assert!(s.problems[0].is_open());
    std::fs::remove_file(d.path().join("down")).unwrap();

    // A fix lands and passes.
    let green = "2".repeat(40);
    std::fs::write(d.path().join("answer.json"), answer(&green, "SUCCESS")).unwrap();
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.ci[0].error.is_none());
    assert_eq!(
        shown.problems[0].closed_reason.as_deref(),
        Some("Passed at 2222222.")
    );
    assert!(shown.attention.is_empty());

    // With Watch CI off, Orbit forgets what it read.
    e.update(|s| s.settings.watch_ci = false).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    assert!(e.snapshot().await.ci.is_empty());
}

/// A repository disconnected while its CI was read is left out, and its
/// problems close.
#[cfg(unix)]
#[tokio::test]
async fn disconnecting_a_repository_closes_its_problems() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "FAILURE"),
    )
    .unwrap();
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    e.update(|s| s.settings.repositories.clear()).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.ci.is_empty());
    assert_eq!(
        shown.problems[0].closed_reason.as_deref(),
        Some("The repository was disconnected.")
    );
    assert!(shown.attention.is_empty());
}

/// Watch CI turned off while a read is on its way: what it brings back is
/// dropped, so nothing opens and nothing tells you.
#[cfg(unix)]
#[tokio::test]
async fn turning_watch_ci_off_during_a_read_drops_what_it_returns() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "FAILURE"),
    )
    .unwrap();
    std::fs::write(d.path().join("hold"), "").unwrap();
    let e = connected(d.path()).await;
    let reading = e.clone();
    let task = tokio::spawn(crate::github::GH.scope(gh, async move {
        reading.watch_ci().await;
    }));
    for _ in 0..500 {
        if d.path().join("asked").exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(d.path().join("asked").exists(), "the read started");
    e.update(|s| s.settings.watch_ci = false).await;
    std::fs::remove_file(d.path().join("hold")).unwrap();
    task.await.unwrap();
    let shown = e.shown_snapshot().await;
    assert!(shown.ci.is_empty());
    assert!(shown.problems.is_empty());
    assert!(shown.attention.is_empty());
    assert_eq!(heads_ups(&e).await, 0);
}

/// A flaky default branch tells you once while it flips between red and
/// green, and again only when it comes back worse.
#[cfg(unix)]
#[tokio::test]
async fn a_flaky_branch_tells_you_once_until_it_comes_back_worse() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    let read = |answer: String| {
        std::fs::write(d.path().join("answer.json"), answer).unwrap();
        crate::github::GH.scope(gh.clone(), e.watch_ci())
    };
    read(answer(&"1".repeat(40), "FAILURE")).await;
    let key = e.shown_snapshot().await.attention[0].key.clone();
    read(answer(&"2".repeat(40), "SUCCESS")).await;
    assert!(e.shown_snapshot().await.attention.is_empty());
    read(answer(&"3".repeat(40), "FAILURE")).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.attention[0].key, key, "the same item, back");
    assert_eq!(shown.problems.len(), 1);
    assert_eq!(heads_ups(&e).await, 1, "and not told again");
    read(checks(
        &"4".repeat(40),
        &[("Rust", "FAILURE"), ("Lint", "FAILURE")],
    ))
    .await;
    assert_eq!(heads_ups(&e).await, 2, "worse is news");
    read(checks(
        &"4".repeat(40),
        &[("Rust", "FAILURE"), ("Lint", "FAILURE")],
    ))
    .await;
    assert_eq!(heads_ups(&e).await, 2);
}
