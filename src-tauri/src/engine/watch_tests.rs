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
        // The morning brief comes by the clock, so only where a test
        // turns it on.
        s.settings.buddy.brief = false;
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

/// GitHub as `gh_fixture.cjs` fakes it, which knows runners and runs.
#[cfg(unix)]
fn fixture_gh(dir: &Path) -> std::path::PathBuf {
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
/// A queued run of `a/b`, made these many minutes ago, and its latest try
/// begun these many minutes ago, if later.
fn run(id: u64, name: &str, made: i64, began: Option<i64>) -> serde_json::Value {
    let ago = |m: i64| (chrono::Utc::now() - chrono::Duration::minutes(m)).to_rfc3339();
    json!({
        "id": id,
        "name": name,
        "status": "queued",
        "created_at": ago(made),
        "run_started_at": ago(began.unwrap_or(made)),
        "html_url": format!("https://github.com/a/b/actions/runs/{id}"),
    })
}
/// The runs of `a/b`.
fn queue(dir: &Path, runs: &[serde_json::Value]) {
    std::fs::write(dir.join("queued.json"), json!(runs).to_string()).unwrap();
}
/// Runs of `a/b` queued since these many minutes ago, by ID and name.
fn queued(dir: &Path, runs: &[(u64, &str, i64)]) {
    let runs: Vec<_> = runs
        .iter()
        .map(|(id, name, ago)| run(*id, name, *ago, None))
        .collect();
    queue(dir, &runs);
}
/// How many times the fake GitHub was sent this call.
fn sent(dir: &Path, call: &str) -> usize {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == call)
        .count()
}
/// Everything but reads that Orbit sent GitHub.
fn writes(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("GET ") && !l.contains("graphql"))
        .map(str::to_string)
        .collect()
}
/// Makes every place due to be read again.
async fn read_long_ago(e: &Engine) {
    e.update(|s| {
        for p in &mut s.runners {
            p.read_at = "2020-01-01T00:00:00.000Z".into();
        }
    })
    .await;
}
async fn waits_told(e: &Engine) -> usize {
    let s = e.snapshot().await;
    s.buddy_feed
        .notes
        .iter()
        .filter(|n| n.kind == NoteKind::NeedsYou && n.text.contains("for a runner"))
        .count()
}

/// A run that waits half an hour for a runner needs you once, with what
/// Orbit sees of the runners, until none wait. Nothing goes to GitHub, and
/// a person's account is read once.
#[cfg(unix)]
#[tokio::test]
async fn runs_waiting_for_a_runner_need_you_until_none_wait() {
    use crate::problems::ProblemKind;
    use crate::runners::PlaceKind;
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    // One waited long enough, the other not yet.
    queued(d.path(), &[(7, "CI", 45), (8, "Docs", 5)]);
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    let places: Vec<_> = shown
        .runners
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.error.clone()))
        .collect();
    assert_eq!(
        places,
        [
            ("a/b", PlaceKind::Repository, None),
            ("a", PlaceKind::Person, None)
        ]
    );
    let open: Vec<_> = shown.problems.iter().filter(|p| p.is_open()).collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].kind, ProblemKind::Waiting);
    assert_eq!(open[0].waiting.len(), 1);
    assert_eq!(
        open[0].waiting[0].url,
        "https://github.com/a/b/actions/runs/7"
    );
    assert_eq!(shown.attention.len(), 1);
    assert_eq!(shown.attention[0].kind, AttentionKind::RunsWaiting);
    assert_eq!(
        shown.attention[0].reason,
        "A run of CI in a/b has waited more than 30 minutes for a runner. Orbit sees no self-hosted runners for it, so its runs wait for GitHub's own runners or for one it doesn't have."
    );
    assert_eq!(waits_told(&e).await, 1);
    assert!(shown.ops.is_empty(), "no agent hears of it");
    assert!(writes(d.path()).is_empty());

    assert_eq!(sent(d.path(), "GET repos/a/b/actions/runs"), 1, "one page");

    // Read again a moment later: nothing is due yet.
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/runners"), 1);

    // The run starts.
    std::fs::remove_file(d.path().join("queued.json")).unwrap();
    read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.problems.iter().all(|p| !p.is_open()));
    assert_eq!(
        shown.problems[0].closed_reason.as_deref(),
        Some("No runs wait for a runner now.")
    );
    assert!(shown.attention.is_empty());
    assert_eq!(waits_told(&e).await, 1);
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/runners"), 2);
    assert_eq!(sent(d.path(), "GET users/a"), 1, "a person is read once");
    assert!(writes(d.path()).is_empty());

    // Disconnected, its runners and its owner's are forgotten.
    e.update(|s| s.settings.repositories.clear()).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    assert!(e.shown_snapshot().await.runners.is_empty());
}

/// After GitHub's rate limit, the rest of the places wait for the next
/// pass.
#[cfg(unix)]
#[tokio::test]
async fn a_rate_limit_stops_the_pass() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    std::fs::write(
        d.path().join("runners-repo-error"),
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)\n",
    )
    .unwrap();
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.runners.len(), 1);
    assert!(
        shown.runners[0].unseen.is_none(),
        "a rate limit hides nothing"
    );
    assert!(shown.runners[0].error.is_some());
    assert_eq!(sent(d.path(), "GET users/a"), 0);
}

/// An organization's runners are read, though Orbit can't tell which of
/// its repositories may use them, and runners Orbit can't see are said so.
#[cfg(unix)]
#[tokio::test]
async fn an_organizations_runners_are_read_and_hidden_ones_are_said_so() {
    use crate::runners::PlaceKind;
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    std::fs::write(d.path().join("org"), "").unwrap();
    std::fs::write(
        d.path().join("runners-repo-error"),
        "gh: Must have admin rights to Repository. (HTTP 403)\n",
    )
    .unwrap();
    let runners = json!({ "total_count": 2, "runners": [
        { "id": 1, "name": "big-1", "os": "linux", "status": "offline", "busy": false,
          "labels": [{ "name": "self-hosted" }, { "name": "big" }] },
        { "id": 2, "name": "big-2", "os": "linux", "status": "offline", "busy": false, "labels": [] },
    ] });
    std::fs::write(d.path().join("runners-org.json"), runners.to_string()).unwrap();
    queued(d.path(), &[(7, "CI", 45)]);
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    let repo = &shown.runners[0];
    assert_eq!(repo.kind, PlaceKind::Repository);
    assert_eq!(
        repo.unseen.as_deref(),
        Some("Orbit can't see the self-hosted runners of a/b: that needs admin rights on it.")
    );
    let org = &shown.runners[1];
    assert_eq!(
        (org.name.as_str(), org.kind),
        ("a", PlaceKind::Organization)
    );
    assert_eq!(org.total, 2);
    assert_eq!(org.runners[0].labels, ["self-hosted", "big"]);
    assert_eq!(
        shown.attention[0].reason,
        "A run of CI in a/b has waited more than 30 minutes for a runner. Orbit can't see its own self-hosted runners. The a organization shares 2 self-hosted runners (0 online and free), but Orbit can't tell whether a/b may use them."
    );
    // Next time, Orbit knows a is an organization. Now a/b's own runner
    // is offline, and one of a's is free, in a runner group that may leave
    // a/b out: it isn't a/b's to use as far as Orbit can tell.
    std::fs::remove_file(d.path().join("runners-repo-error")).unwrap();
    let mine = json!({ "total_count": 1, "runners": [
        { "id": 3, "name": "mac-1", "os": "macos", "status": "offline", "busy": false, "labels": [] },
    ] });
    std::fs::write(d.path().join("runners-repo.json"), mine.to_string()).unwrap();
    let shared = json!({ "total_count": 2, "runners": [
        { "id": 1, "name": "big-1", "os": "linux", "status": "offline", "busy": false, "labels": [] },
        { "id": 2, "name": "big-2", "os": "linux", "status": "online", "busy": false, "labels": [] },
    ] });
    std::fs::write(d.path().join("runners-org.json"), shared.to_string()).unwrap();
    read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), "GET users/a"), 1);
    assert_eq!(sent(d.path(), "GET orgs/a/actions/runners"), 2);
    assert_eq!(
        e.shown_snapshot().await.attention[0].reason,
        "A run of CI in a/b has waited more than 30 minutes for a runner. Its one self-hosted runner is offline. The a organization shares 2 self-hosted runners (1 online and free), but Orbit can't tell whether a/b may use them."
    );
}

/// Orbit reads every page of the queued runs made long enough ago to have
/// waited, as GitHub lists the one that has waited longest last, and a
/// queue longer than it reads never says none wait.
#[cfg(unix)]
#[tokio::test]
async fn every_page_of_the_queue_is_read_and_a_longer_one_never_clears() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    // Runs made a moment ago can't have waited long, nor reruns whose try
    // began a moment ago, and runs that started aren't queued.
    let mut runs: Vec<_> = (0..300).map(|i| run(1000 + i, "New", 1, None)).collect();
    runs.extend((0..250).map(|i| run(2000 + i, "Rerun", 300, Some(5))));
    runs.extend((0..50).map(|i| {
        let mut r = run(3000 + i, "Started", 350, None);
        r["status"] = json!("in_progress");
        r
    }));
    runs.push(run(9, "Release", 400, None));
    queue(d.path(), &runs);
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    let open: Vec<_> = shown.problems.iter().filter(|p| p.is_open()).collect();
    assert_eq!(open.len(), 1);
    let ids: Vec<u64> = open[0].waiting.iter().map(|w| w.id).collect();
    assert_eq!(ids, [9], "on the third page");
    assert!(!shown.runners[0].more_queued);
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/runs"), 3);

    // Now more are queued than Orbit reads, and the one that waited
    // longest is past its last page.
    runs.extend((0..50).map(|i| run(4000 + i, "Rerun", 300, Some(5))));
    queue(d.path(), &runs);
    read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/runs"), 6);
    let shown = e.shown_snapshot().await;
    assert!(shown.runners[0].more_queued);
    let open: Vec<_> = shown.problems.iter().filter(|p| p.is_open()).collect();
    assert_eq!(open.len(), 1, "it may wait still");
    assert_eq!(open[0].waiting.len(), 1, "as Orbit saw it last");
    assert_eq!(
        shown.attention[0].reason,
        "A run of Release in a/b has waited more than 30 minutes for a runner. Orbit couldn't read every queued run, so more may wait. Orbit sees no self-hosted runners for it, so its runs wait for GitHub's own runners or for one it doesn't have."
    );

    // The queue empties.
    std::fs::remove_file(d.path().join("queued.json")).unwrap();
    read_long_ago(&e).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.problems.iter().all(|p| !p.is_open()));
    assert!(!shown.runners[0].more_queued);
}

/// A read GitHub doesn't answer keeps what Orbit saw, and turning Watch CI
/// off forgets the runners and closes the problem.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_read_keeps_the_runners_and_watch_ci_off_forgets_them() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    queued(d.path(), &[(7, "CI", 45)]);
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let key = e.shown_snapshot().await.attention[0].key.clone();

    std::fs::write(d.path().join("down"), "").unwrap();
    read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.attention.len(), 1);
    assert_eq!(
        shown.attention[0].key, key,
        "still waiting, as far as Orbit knows"
    );
    assert_eq!(shown.problems[0].waiting.len(), 1);
    let repo = &shown.runners[0];
    assert_eq!(repo.tries, 1);
    assert_eq!(
        repo.error.as_deref(),
        Some("Orbit couldn't read the runners yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    assert!(!repo.read_at.is_empty(), "what was read before is kept");

    std::fs::remove_file(d.path().join("down")).unwrap();
    e.update(|s| s.settings.watch_ci = false).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.runners.is_empty());
    assert_eq!(
        shown.problems[0].closed_reason.as_deref(),
        Some("Orbit stopped watching CI.")
    );
    assert!(shown.attention.is_empty());
}

/// What comes back from reading runners after the repository was
/// disconnected, or Watch CI turned off, is dropped, and with Watch CI off
/// Orbit doesn't ask GitHub about runners at all.
#[cfg(unix)]
#[tokio::test]
async fn runners_read_after_a_change_are_dropped() {
    for change in ["disconnect", "watch off"] {
        let d = tempfile::tempdir().unwrap();
        let gh = fixture_gh(d.path());
        std::fs::write(
            d.path().join("answer.json"),
            answer(&"1".repeat(40), "SUCCESS"),
        )
        .unwrap();
        queued(d.path(), &[(7, "CI", 45)]);
        // GitHub takes its time over the runners.
        std::fs::write(d.path().join("hold"), "").unwrap();
        let e = connected(d.path()).await;
        let reading = tokio::spawn(crate::github::GH.scope(gh.clone(), {
            let e = e.clone();
            async move { e.watch_ci().await }
        }));
        let held = d.path().join("held");
        for _ in 0..500 {
            if held.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let calls = std::fs::read_to_string(d.path().join("calls")).unwrap();
        assert_eq!(
            calls.lines().last(),
            Some("GET repos/a/b/actions/runners"),
            "{change}"
        );
        e.update(|s| match change {
            "disconnect" => s.settings.repositories.clear(),
            _ => s.settings.watch_ci = false,
        })
        .await;
        std::fs::remove_file(d.path().join("hold")).unwrap();
        reading.await.unwrap();
        let shown = e.shown_snapshot().await;
        assert!(shown.runners.is_empty(), "{change}");
        assert!(shown.problems.is_empty(), "{change}");
        assert!(shown.attention.is_empty(), "{change}");
    }
    // With Watch CI off from the start, nothing is asked.
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.settings.watch_ci = false).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/runners"), 0);
    assert_eq!(sent(d.path(), "GET users/a"), 0);
}

/// `n` artifacts of `a/b`, a megabyte each, the first `expired` of them
/// expired, by ID from `from`.
fn artifacts(dir: &Path, from: u64, n: u64, expired: u64) {
    let list: Vec<_> = (from..from + n)
        .map(|id| json!({ "id": id, "size_in_bytes": 1 << 20, "expired": id < from + expired }))
        .collect();
    std::fs::write(dir.join("artifacts.json"), json!(list).to_string()).unwrap();
}
/// Makes every repository's storage due to be read again.
async fn storage_read_long_ago(e: &Engine) {
    e.update(|s| {
        for r in &mut s.actions_storage {
            r.read_at = "2020-01-01T00:00:00.000Z".into();
        }
    })
    .await;
}

/// Orbit reads a repository's caches and the artifacts it keeps, every
/// page of them, then again only half an hour later. A read GitHub doesn't
/// answer keeps what Orbit saw, and turning Watch CI off forgets it.
#[cfg(unix)]
#[tokio::test]
async fn actions_storage_is_read_and_kept_when_github_fails() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    let usage =
        json!({ "active_caches_size_in_bytes": 9_800_000_000u64, "active_caches_count": 42 });
    std::fs::write(d.path().join("cache.json"), usage.to_string()).unwrap();
    artifacts(d.path(), 1, 150, 10);
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    let s = &shown.actions_storage[0];
    assert_eq!(s.repo, "a/b");
    assert_eq!((s.caches, s.cache_bytes), (42, 9_800_000_000));
    assert_eq!((s.artifacts, s.artifact_bytes), (140, 140 << 20));
    assert!(!s.more_artifacts);
    assert!(s.unseen.is_none() && s.error.is_none());
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/artifacts"), 2);
    assert!(writes(d.path()).is_empty());

    // A moment later, nothing is due.
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/cache/usage"), 1);

    // GitHub fails: what was read is kept.
    std::fs::write(d.path().join("cache-error"), "gh: Bad Gateway (HTTP 502)\n").unwrap();
    storage_read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let s = e.shown_snapshot().await.actions_storage[0].clone();
    assert_eq!(s.caches, 42);
    assert_eq!(s.tries, 1);
    assert_eq!(
        s.error.as_deref(),
        Some("Orbit couldn't read the storage yet: Bad Gateway (HTTP 502). It tries again later.")
    );
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/artifacts"), 2);

    e.update(|s| s.settings.watch_ci = false).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    assert!(e.shown_snapshot().await.actions_storage.is_empty());
}

/// Storage GitHub won't show is said so, and more artifacts than Orbit
/// reads are said so too. Disconnected, a repository's storage is
/// forgotten.
#[cfg(unix)]
#[tokio::test]
async fn storage_orbit_cant_see_or_read_in_full_is_said_so() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    let hidden =
        "Orbit can't see the Actions storage of a/b: GitHub doesn't show it to Orbit's sign-in.";
    // Its artifacts hidden.
    std::fs::write(
        d.path().join("artifacts-error"),
        "gh: Not Found (HTTP 404)\n",
    )
    .unwrap();
    let e = connected(d.path()).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let s = e.shown_snapshot().await.actions_storage[0].clone();
    assert_eq!(s.unseen.as_deref(), Some(hidden));
    assert!(s.error.is_none());
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/artifacts"), 1);

    // Its caches hidden: nothing more is asked.
    std::fs::remove_file(d.path().join("artifacts-error")).unwrap();
    std::fs::write(
        d.path().join("cache-error"),
        "gh: Resource not accessible by integration (HTTP 403)\n",
    )
    .unwrap();
    storage_read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let s = e.shown_snapshot().await.actions_storage[0].clone();
    assert_eq!(s.unseen.as_deref(), Some(hidden));
    assert!(s.error.is_none());
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/cache/usage"), 2);
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/artifacts"), 1);

    // More than three pages: Orbit reads three, and says there's more.
    std::fs::remove_file(d.path().join("cache-error")).unwrap();
    artifacts(d.path(), 1, 301, 0);
    storage_read_long_ago(&e).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let s = e.shown_snapshot().await.actions_storage[0].clone();
    assert!(s.unseen.is_none());
    assert_eq!(s.artifacts, 300);
    assert!(s.more_artifacts);
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/artifacts"), 4);

    e.update(|s| s.settings.repositories.clear()).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    assert!(e.shown_snapshot().await.actions_storage.is_empty());
}

/// After GitHub's rate limit, the other repositories wait for the next
/// pass, and with Watch CI off nothing is asked.
#[cfg(unix)]
#[tokio::test]
async fn a_rate_limit_stops_the_storage_pass() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    std::fs::write(
        d.path().join("answer.json"),
        answer(&"1".repeat(40), "SUCCESS"),
    )
    .unwrap();
    std::fs::write(
        d.path().join("cache-error"),
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)\n",
    )
    .unwrap();
    let e = connected(d.path()).await;
    e.update(|s| {
        let mut other = s.settings.repositories[0].clone();
        other.name = "a/c".into();
        s.settings.repositories.push(other);
    })
    .await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.actions_storage.len(), 1);
    assert!(
        shown.actions_storage[0].unseen.is_none(),
        "a rate limit hides nothing"
    );
    assert!(shown.actions_storage[0].error.is_some());
    assert_eq!(sent(d.path(), "GET repos/a/c/actions/cache/usage"), 0);

    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.settings.watch_ci = false).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    assert_eq!(sent(d.path(), "GET repos/a/b/actions/cache/usage"), 0);
}

/// Storage read while the repository is disconnected, or Watch CI turned
/// off, is dropped.
#[cfg(unix)]
#[tokio::test]
async fn storage_read_after_a_change_is_dropped() {
    for change in ["disconnect", "watch off"] {
        let d = tempfile::tempdir().unwrap();
        let gh = fixture_gh(d.path());
        std::fs::write(
            d.path().join("answer.json"),
            answer(&"1".repeat(40), "SUCCESS"),
        )
        .unwrap();
        // GitHub takes its time over the caches.
        std::fs::write(d.path().join("hold"), "repos/a/b/actions/cache/usage").unwrap();
        let e = connected(d.path()).await;
        let reading = tokio::spawn(crate::github::GH.scope(gh.clone(), {
            let e = e.clone();
            async move { e.watch_ci().await }
        }));
        let held = d.path().join("held");
        for _ in 0..500 {
            if held.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(held.exists(), "{change}");
        e.update(|s| match change {
            "disconnect" => s.settings.repositories.clear(),
            _ => s.settings.watch_ci = false,
        })
        .await;
        std::fs::remove_file(d.path().join("hold")).unwrap();
        reading.await.unwrap();
        assert!(
            e.shown_snapshot().await.actions_storage.is_empty(),
            "{change}"
        );
    }
}

/// GitHub's CI for `a/b`: main at commit `1`, made an hour ago, and pull
/// request 1 at `head`, whose one check passed, begun half an hour ago.
fn passing_pull(dir: &Path, head: &str) {
    passing_pulls(dir, &[(1, head)]);
}
/// The same, with these pull requests, by number and head.
fn passing_pulls(dir: &Path, pulls: &[(u64, &str)]) {
    let ago = |m: i64| (chrono::Utc::now() - chrono::Duration::minutes(m)).to_rfc3339();
    let check = json!({
        "__typename": "CheckRun",
        "name": "Rust",
        "status": "COMPLETED",
        "conclusion": "SUCCESS",
        "detailsUrl": "https://github.com/a/b/actions/runs/1/job/2",
        "checkSuite": { "createdAt": ago(30), "workflowRun": { "workflow": { "name": "CI" } } }
    });
    let rollup = json!({ "contexts": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [check]
    } });
    let nodes: Vec<_> = pulls
        .iter()
        .map(|(number, head)| {
            json!({
                "number": number,
                "isDraft": false,
                "headRefOid": head,
                "commits": { "nodes": [{ "commit": { "oid": head, "statusCheckRollup": rollup } }] }
            })
        })
        .collect();
    let v = json!({ "data": { "repository": {
        "defaultBranchRef": { "name": "main", "target": {
            "oid": "1".repeat(40),
            "committedDate": ago(60),
            "messageHeadline": "Update the build",
            "statusCheckRollup": rollup
        } },
        "pullRequests": { "pageInfo": { "hasNextPage": false }, "nodes": nodes }
    } } });
    std::fs::write(dir.join("answer.json"), v.to_string()).unwrap();
}
/// What GitHub says about pull request 1 at `head`, against main's latest
/// commit in `passing_pulls`, with nobody asked to review it.
fn mergeable(dir: &Path, head: &str, mergeable: serde_json::Value, state: &str) {
    let v = json!({
        "user": { "login": "author" },
        "head": { "sha": head },
        "base": { "sha": "1".repeat(40), "ref": "main", "repo": { "owner": { "login": "a" } } },
        "mergeable": mergeable,
        "mergeable_state": state,
        "requested_reviewers": [],
        "requested_teams": []
    });
    std::fs::write(dir.join("pull-1.json"), v.to_string()).unwrap();
}
/// Pull request 1, by a member from a branch in `a/b` against main's latest
/// commit in `passing_pulls`, and a clean round of it as it is.
async fn reviewed(e: &Engine) -> PullRequest {
    let mut p = crate::types::tests::pr();
    p.head_repo = "a/b".into();
    p.author_association = "MEMBER".into();
    p.base_sha = "1".repeat(40);
    e.update(|s| {
        s.prs = vec![p.clone()];
        let mut r = Review::new(&p, &s.settings, true);
        r.status = "posted".into();
        r.report = "Report".into();
        r.findings = Some(vec![]);
        crate::review_loop::decide(&mut r, Some(1));
        s.reviews.push(r);
    })
    .await;
    p
}
async fn line(e: &Engine) -> String {
    e.shown_snapshot().await.readiness[0].line()
}

/// Once nothing else is in the way, Orbit asks GitHub whether a pull
/// request merges, again a little later while GitHub is still working it
/// out, and then not for a while. A failed read keeps the answer, and
/// Watch CI off forgets it. Nothing is written to GitHub.
#[cfg(unix)]
#[tokio::test]
async fn github_is_asked_whether_a_clean_pull_request_merges() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    let p = reviewed(&e).await;
    let ask = "GET repos/a/b/pulls/1";
    // Its check fails: GitHub isn't asked.
    answer_pull_failing(d.path(), &p.head_sha);
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), ask), 0);
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. CI / Rust fails on aaaaaaa."
    );

    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(null), "unknown");
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), ask), 1);
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. GitHub is still checking whether it merges."
    );
    // Not again until its wait is over.
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), ask), 1);
    e.update(|s| s.merges[0].retry_at = Some("2020-01-01T00:00:00.000Z".into()))
        .await;
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), ask), 2);
    let shown = e.shown_snapshot().await;
    assert!(shown.readiness[0].clear);
    assert_eq!(
        shown.readiness[0].line(),
        "Clear to merge at aaaaaaa against main at 1111111."
    );
    // A definite answer stands for a while.
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), ask), 2);

    // GitHub fails: the answer stands, with the error kept.
    e.update(|s| s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into())
        .await;
    std::fs::write(d.path().join("pull-error"), "gh: Bad Gateway (HTTP 502)\n").unwrap();
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(sent(d.path(), ask), 3);
    let shown = e.shown_snapshot().await;
    assert!(shown.readiness[0].clear);
    assert_eq!(
        shown.merges[0].error.as_deref(),
        Some("Orbit couldn't ask GitHub whether it merges: Bad Gateway (HTTP 502). It tries again later.")
    );
    assert!(writes(d.path()).is_empty());

    // Back to draft: Orbit no longer reviews it, so its answer goes.
    e.update(|s| s.prs[0].draft = true).await;
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.merges.is_empty());
    assert!(shown.readiness.is_empty());
    assert_eq!(sent(d.path(), ask), 3);
    e.update(|s| s.prs[0].draft = false).await;

    e.update(|s| s.settings.watch_ci = false).await;
    crate::github::GH.scope(gh, e.watch_ci()).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.merges.is_empty());
    assert_eq!(
        shown.readiness[0].line(),
        "Not clear to merge yet. Orbit can't see its checks while Watch CI is off."
    );
    assert_eq!(sent(d.path(), ask), 3);
}
/// GitHub's CI for `a/b` with pull request 1's one check still running
/// at `head`.
fn answer_pull_running(dir: &Path, head: &str) {
    passing_pull(dir, head);
    let text = std::fs::read_to_string(dir.join("answer.json")).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let check = &mut v["data"]["repository"]["pullRequests"]["nodes"][0]["commits"]["nodes"][0]
        ["commit"]["statusCheckRollup"]["contexts"]["nodes"][0];
    check["status"] = json!("IN_PROGRESS");
    check["conclusion"] = json!(null);
    std::fs::write(dir.join("answer.json"), v.to_string()).unwrap();
}
/// The one comment the fake GitHub holds.
fn comment(dir: &Path) -> String {
    let text = std::fs::read_to_string(dir.join("github.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let comments = v["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    comments[0]["body"].as_str().unwrap().to_string()
}

/// The agent's view, end to end: Orbit's comment on the pull request comes
/// once it is clear to merge, a push takes it back with one edit, and it
/// turns clear again with one more, however many steps Orbit, CI and
/// GitHub take in between.
#[cfg(unix)]
#[tokio::test]
async fn the_agent_is_told_once_each_time_the_answer_changes() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.github_user = Some("me".into())).await;
    let p = reviewed(&e).await;
    let watch = || crate::github::GH.scope(gh.clone(), e.watch_ci());
    let retry = || e.update(|s| s.merges[0].retry_at = Some("2020-01-01T00:00:00.000Z".into()));
    // Nothing is started while Watch CI is off, though it waits on you.
    e.update(|s| s.settings.watch_ci = false).await;
    watch().await;
    assert!(e.snapshot().await.ops.is_empty());
    e.update(|s| s.settings.watch_ci = true).await;
    // GitHub still working out whether it merges: nothing to tell yet.
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(null), "unknown");
    watch().await;
    assert!(writes(d.path()).is_empty());
    retry().await;
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    watch().await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/1/comments"]);
    let body = comment(d.path());
    assert!(body.starts_with(&format!(
        "## Orbit\n\n**Clear to merge** at `aaaaaaa` against `main` at `1111111`.\n\n<!-- orbit-ready {{\"base\":\"main\",\"baseSha\":\"{}\",\"clear\":true,\"head\":\"{}\",\"turn\":null,\"version\":1}} -->",
        "1".repeat(40),
        p.head_sha
    )));
    watch().await;
    assert_eq!(writes(d.path()).len(), 1);

    // A push takes it back: one edit.
    let new = "b".repeat(40);
    e.update(|s| s.prs[0].head_sha = new.clone()).await;
    answer_pull_running(d.path(), &new);
    watch().await;
    assert_eq!(writes(d.path()).len(), 2);
    assert!(comment(d.path())
        .contains("**Not clear to merge yet.** Orbit hasn't reviewed bbbbbbb yet."));
    // Orbit reviews it clean, the checks finish and GitHub works it out:
    // nothing until it is clear again, then one edit.
    e.update(|s| {
        let mut r = Review::new(&s.prs[0], &s.settings, true);
        r.status = "posted".into();
        r.report = "Report".into();
        r.findings = Some(vec![]);
        crate::review_loop::decide(&mut r, Some(1));
        s.reviews.push(r);
    })
    .await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. Checks are still running on bbbbbbb."
    );
    passing_pull(d.path(), &new);
    mergeable(d.path(), &new, json!(null), "unknown");
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. GitHub is still checking whether it merges."
    );
    assert_eq!(writes(d.path()).len(), 2);
    retry().await;
    mergeable(d.path(), &new, json!(true), "clean");
    watch().await;
    let w = writes(d.path());
    assert_eq!(w.len(), 3);
    assert!(w[1].starts_with("PATCH repos/a/b/issues/comments/"));
    assert_eq!(w[2], w[1]);
    assert!(
        comment(d.path()).contains("**Clear to merge** at `bbbbbbb` against `main` at `1111111`.")
    );
}
/// A review of pull request 1 by `login` at `commit`.
fn review(login: &str, state: &str, commit: &str) -> serde_json::Value {
    json!({ "user": { "login": login }, "state": state, "commit_id": commit })
}
fn reviews(dir: &Path, reviews: &[serde_json::Value]) {
    std::fs::write(dir.join("reviews-1.json"), json!(reviews).to_string()).unwrap();
}

/// Where main's rules need an approval, one of an older commit keeps the
/// line waiting on its reviewer, and their approval of the latest commit
/// turns it clear. Rules Orbit can't see are said so, and left to GitHub.
#[cfg(unix)]
#[tokio::test]
async fn an_approval_of_an_older_commit_keeps_it_waiting_on_its_reviewer() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.github_user = Some("me".into())).await;
    let p = reviewed(&e).await;
    let watch = || crate::github::GH.scope(gh.clone(), e.watch_ci());
    let again = || e.update(|s| s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into());
    let old = "b".repeat(40);
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    // Protected by a ruleset alone, which needs an approval.
    std::fs::write(d.path().join("branch-main.json"), r#"{"protected":true}"#).unwrap();
    std::fs::write(
        d.path().join("rules-main.json"),
        r#"[{"type":"pull_request","parameters":{"required_approving_review_count":1}}]"#,
    )
    .unwrap();
    reviews(d.path(), &[review("alice", "APPROVED", &old)]);
    watch().await;
    let waiting =
        "main needs 1 approval of aaaaaaa and it has none: alice approved bbbbbbb but not aaaaaaa.";
    assert_eq!(line(&e).await, format!("Not clear to merge yet. {waiting}"));
    let shown = e.shown_snapshot().await;
    let r = shown.readiness[0].reviews.as_ref().unwrap();
    assert_eq!(r.rule.as_ref().map(|r| r.approvals), Some(1));
    assert_eq!(r.reviewers[0].stand, crate::readiness::Stand::ApprovedOlder);
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/1/comments"]);
    assert!(comment(d.path()).contains(&format!("**Not clear to merge yet.** {waiting}")));
    assert!(comment(d.path()).contains(r#""turn":"person""#));
    // Read again later, still waiting: nothing more is sent.
    again().await;
    watch().await;
    assert_eq!(writes(d.path()).len(), 1);
    // Once each time, and once before each write, for whether another
    // Orbit posted since (`crate::orbits`).
    let reads = |d: &Path| sent(d, "GET repos/a/b/pulls/1/reviews") - writes(d).len();
    assert_eq!(reads(d.path()), 2);

    // Alice approves the latest commit: one edit, to clear.
    reviews(
        d.path(),
        &[
            review("alice", "APPROVED", &old),
            review("alice", "APPROVED", &p.head_sha),
        ],
    );
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Clear to merge at aaaaaaa against main at 1111111."
    );
    assert_eq!(writes(d.path()).len(), 2);
    assert!(comment(d.path())
        .contains("**Clear to merge** at `aaaaaaa` against `main` at `1111111`.\n\n<!--"));

    // Orbit's sign-in can't see main's protection: it says so, leaves the
    // reviews to GitHub, and doesn't edit the comment just for that.
    std::fs::write(
        d.path().join("protection-error"),
        "gh: Not Found (HTTP 404)\n",
    )
    .unwrap();
    again().await;
    watch().await;
    let shown = e.shown_snapshot().await;
    assert_eq!(
        shown.readiness[0].reviews.as_ref().unwrap().unseen.as_deref(),
        Some("Orbit's GitHub sign-in can't see main's rules, so Orbit can't tell whether it needs reviews.")
    );
    assert_eq!(
        shown.readiness[0].line(),
        "Clear to merge at aaaaaaa against main at 1111111. Orbit can't see main's rules, so it left its reviews to GitHub."
    );
    assert_eq!(writes(d.path()).len(), 2);
    // Or GitHub fails to say.
    std::fs::write(
        d.path().join("protection-error"),
        "gh: Bad Gateway (HTTP 502)\n",
    )
    .unwrap();
    again().await;
    watch().await;
    let shown = e.shown_snapshot().await;
    assert_eq!(
        shown.readiness[0].reviews.as_ref().unwrap().unseen.as_deref(),
        Some("Orbit couldn't read main's rules, so it can't tell whether it needs reviews: Bad Gateway (HTTP 502).")
    );
    // Or doesn't say whether main is protected, or which branch it is
    // into.
    std::fs::write(d.path().join("branch-main.json"), "{}").unwrap();
    again().await;
    watch().await;
    let shown = e.shown_snapshot().await;
    assert_eq!(
        shown.readiness[0].reviews.as_ref().unwrap().unseen.as_deref(),
        Some("Orbit couldn't read main's rules, so it can't tell whether it needs reviews: GitHub didn't say whether it is protected.")
    );
    std::fs::write(d.path().join("branch-main.json"), r#"{"protected":true}"#).unwrap();
    let mut pull: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("pull-1.json")).unwrap())
            .unwrap();
    pull["base"]["ref"] = json!(null);
    std::fs::write(d.path().join("pull-1.json"), pull.to_string()).unwrap();
    again().await;
    watch().await;
    let shown = e.shown_snapshot().await;
    assert_eq!(
        shown.readiness[0]
            .reviews
            .as_ref()
            .unwrap()
            .unseen
            .as_deref(),
        Some("GitHub didn't say its base branch, so Orbit can't tell whether it needs reviews.")
    );
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    // GitHub then says a rule blocks it: who it may wait for.
    let mut pull: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("pull-1.json")).unwrap())
            .unwrap();
    pull["mergeable_state"] = json!("blocked");
    pull["requested_reviewers"] = json!([{ "login": "bob" }]);
    std::fs::write(d.path().join("pull-1.json"), pull.to_string()).unwrap();
    again().await;
    watch().await;
    let blocked = "GitHub says a rule blocks merging it, and Orbit can't see main's rules to tell which. So far, bob hasn't reviewed aaaaaaa yet.";
    assert_eq!(line(&e).await, format!("Not clear to merge yet. {blocked}"));
    assert_eq!(writes(d.path()).len(), 3);
    // Its rules read with an admin's sign-in: two approvals.
    std::fs::remove_file(d.path().join("protection-error")).unwrap();
    std::fs::write(
        d.path().join("protection-main.json"),
        r#"{"required_pull_request_reviews":{"required_approving_review_count":2}}"#,
    )
    .unwrap();
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. main needs 2 approvals of aaaaaaa and it has 1: bob hasn't reviewed aaaaaaa yet."
    );
    assert_eq!(reads(d.path()), 9);
    assert_eq!(
        sent(d.path(), "GET repos/a/b/collaborators/alice/permission"),
        7
    );
}

/// A pull request left waiting gets one nudge for whoever owes the next
/// step, end to end: one on its agent gets one edit of Orbit's comment
/// reminding it, and one on a reviewer lands in Needs you, announced once,
/// without an edit. Each leaves once the answer changes.
#[cfg(unix)]
#[tokio::test]
async fn a_stuck_pull_request_nudges_whoever_owes_the_next_step_once() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.github_user = Some("me".into())).await;
    let p = reviewed(&e).await;
    let watch = || crate::github::GH.scope(gh.clone(), e.watch_ci());
    let again = || e.update(|s| s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into());
    // As if its answer had stood for `hours` since the agent saw it.
    let left = |hours: i64| {
        e.update(move |s| {
            let since = chrono::Utc::now() - chrono::Duration::hours(hours);
            s.ops[0].ready.as_mut().unwrap().since = crate::problems::stamp(since);
        })
    };
    let stuck = || async {
        let s = e.shown_snapshot().await;
        let notes = s
            .buddy_feed
            .notes
            .iter()
            .filter(|n| n.kind == NoteKind::NeedsYou && n.text.contains("waited on a person"))
            .count();
        let items: Vec<_> = s
            .attention
            .into_iter()
            .filter(|a| a.kind == AttentionKind::Stuck)
            .collect();
        (items, notes)
    };
    // It conflicts with main: the agent's move.
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(false), "dirty");
    watch().await;
    let conflict = "It conflicts with main: merge main in and fix the conflicts.";
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/1/comments"]);
    assert!(comment(d.path()).contains(&format!("**Not clear to merge yet.** {conflict}\n\n")));
    left(1).await;
    watch().await;
    assert_eq!(writes(d.path()).len(), 1);
    // Two hours on: one edit reminding the agent.
    left(2).await;
    watch().await;
    let w = writes(d.path());
    assert_eq!(w.len(), 2);
    assert!(w[1].starts_with("PATCH repos/a/b/issues/comments/"));
    assert!(comment(d.path()).contains(&format!(
        "**Not clear to merge yet.** {conflict} **Reminder:** it has waited on the author for more than 2 hours.\n\n<!-- orbit-ready "
    )));
    let r = e.snapshot().await.ops[0].ready.clone().unwrap();
    assert!(r.nudged.is_some() && r.reminded.is_some());
    // Once, however long it waits, and it isn't yours in Needs you.
    left(48).await;
    again().await;
    watch().await;
    assert_eq!(writes(d.path()).len(), 2);
    assert_eq!(stuck().await, (vec![], 0));

    // Fixed, and main's rules need an approval bob hasn't given: bob's
    // move, so one edit without the reminder.
    std::fs::write(d.path().join("branch-main.json"), r#"{"protected":true}"#).unwrap();
    std::fs::write(
        d.path().join("rules-main.json"),
        r#"[{"type":"pull_request","parameters":{"required_approving_review_count":1}}]"#,
    )
    .unwrap();
    mergeable(d.path(), &p.head_sha, json!(true), "blocked");
    let mut pull: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("pull-1.json")).unwrap())
            .unwrap();
    pull["requested_reviewers"] = json!([{ "login": "bob" }]);
    std::fs::write(d.path().join("pull-1.json"), pull.to_string()).unwrap();
    again().await;
    watch().await;
    let waiting =
        "main needs 1 approval of aaaaaaa and it has none: bob hasn't reviewed aaaaaaa yet.";
    assert_eq!(line(&e).await, format!("Not clear to merge yet. {waiting}"));
    assert_eq!(writes(d.path()).len(), 3);
    assert!(comment(d.path()).contains(&format!("**Not clear to merge yet.** {waiting}\n\n")));
    assert_eq!(stuck().await, (vec![], 0));
    // Almost a day: nothing yet.
    left(23).await;
    again().await;
    watch().await;
    assert_eq!(stuck().await, (vec![], 0));
    // A day on: in Needs you, told once, and the comment isn't edited.
    left(24).await;
    again().await;
    watch().await;
    let (items, notes) = stuck().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].pr_id.as_deref(), Some(p.id.as_str()));
    assert_eq!(
        items[0].reason,
        format!("It has waited on a person for more than a day. {waiting}")
    );
    assert_eq!(notes, 1);
    assert_eq!(writes(d.path()).len(), 3);
    again().await;
    watch().await;
    assert_eq!(stuck().await, (items, 1));
    assert_eq!(writes(d.path()).len(), 3);

    // Bob approves: clear, with one edit, and it leaves Needs you.
    reviews(d.path(), &[review("bob", "APPROVED", &p.head_sha)]);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Clear to merge at aaaaaaa against main at 1111111."
    );
    assert_eq!(writes(d.path()).len(), 4);
    assert_eq!(stuck().await, (vec![], 1));
}

/// What Orbit can't read never keeps an earlier clear answer: too many
/// reviews, reviews GitHub fails to give, an approval by someone without
/// write access or whose access Orbit can't tell, and rules on a later
/// page each take it back, and GitHub's fresh answer always stands.
#[cfg(unix)]
#[tokio::test]
async fn what_orbit_cant_read_never_keeps_an_earlier_clear_answer() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.github_user = Some("me".into())).await;
    let p = reviewed(&e).await;
    let watch = || crate::github::GH.scope(gh.clone(), e.watch_ci());
    let again = || e.update(|s| s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into());
    let old = "b".repeat(40);
    let one_approval =
        r#"[{"type":"pull_request","parameters":{"required_approving_review_count":1}}]"#;
    let clear = "**Clear to merge** at `aaaaaaa` against `main` at `1111111`.";
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    let approved = [review("alice", "APPROVED", &p.head_sha)];
    reviews(d.path(), &approved);
    // Rules that need no reviews: nobody's access is asked about.
    watch().await;
    assert!(comment(d.path()).contains(clear));
    let alice = "GET repos/a/b/collaborators/alice/permission";
    assert_eq!(sent(d.path(), alice), 0);
    std::fs::write(d.path().join("rules-main.json"), one_approval).unwrap();
    again().await;
    watch().await;
    assert_eq!(sent(d.path(), alice), 1);
    assert_eq!(writes(d.path()).len(), 1);

    // Too many reviews to read them all, with GitHub clean or blocked.
    let mut many: Vec<_> = (0..299)
        .map(|i| review(&format!("r{i}"), "COMMENTED", &old))
        .collect();
    many.push(approved[0].clone());
    reviews(d.path(), &many);
    let too_many = "Not clear to merge yet. It has more reviews than Orbit reads, so Orbit can't tell whether enough people approved aaaaaaa.";
    for state in ["clean", "blocked"] {
        mergeable(d.path(), &p.head_sha, json!(true), state);
        again().await;
        watch().await;
        assert_eq!(line(&e).await, too_many, "{state}");
        assert_eq!(e.shown_snapshot().await.merges[0].state, state);
    }
    assert_eq!(writes(d.path()).len(), 2);
    assert!(comment(d.path()).contains("**Not clear to merge yet.** It has more reviews"));
    // Readable again: clear.
    reviews(d.path(), &approved);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    again().await;
    watch().await;
    assert!(comment(d.path()).contains(clear));
    assert_eq!(writes(d.path()).len(), 3);

    // GitHub fails to give the reviews: its fresh blocked answer stands,
    // and the clear comment is taken back.
    std::fs::write(
        d.path().join("reviews-error"),
        "gh: Bad Gateway (HTTP 502)\n",
    )
    .unwrap();
    mergeable(d.path(), &p.head_sha, json!(true), "blocked");
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. GitHub says a rule blocks merging it, such as a required review."
    );
    assert_eq!(writes(d.path()).len(), 4);
    assert!(comment(d.path()).contains(r#""turn":"person""#));
    // With GitHub clean, Orbit can't tell yet.
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. Orbit couldn't read its reviews, so it can't tell yet whether enough people approved aaaaaaa: Bad Gateway (HTTP 502)."
    );
    std::fs::remove_file(d.path().join("reviews-error")).unwrap();

    // An older approval GitHub may count and a latest one by someone
    // without write access: not clear, though GitHub says clean.
    reviews(
        d.path(),
        &[
            review("carol", "APPROVED", &old),
            review("mallory", "APPROVED", &p.head_sha),
        ],
    );
    std::fs::write(
        d.path().join("permission-mallory.json"),
        r#"{"permission":"read"}"#,
    )
    .unwrap();
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. main needs 1 approval of aaaaaaa and it has none: carol approved bbbbbbb but not aaaaaaa and mallory approved aaaaaaa, but without write access that doesn't count."
    );
    let shown = e.shown_snapshot().await;
    let r = shown.readiness[0].reviews.as_ref().unwrap();
    assert_eq!(r.reviewers[1].may_approve, Some(false));
    // Only approvals of the latest commit and requests for changes are
    // asked about.
    assert_eq!(
        sent(d.path(), "GET repos/a/b/collaborators/carol/permission"),
        0
    );
    // Whose access Orbit can't tell doesn't count either.
    std::fs::write(
        d.path().join("permission-error"),
        "gh: Not Found (HTTP 404)\n",
    )
    .unwrap();
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. main needs 1 approval of aaaaaaa and it has none: carol approved bbbbbbb but not aaaaaaa and mallory approved aaaaaaa, but Orbit can't tell whether they have write access."
    );
    // Nor does an approval by someone whose login Orbit wouldn't put in a
    // path.
    reviews(
        d.path(),
        &[review("dependabot[bot]", "APPROVED", &p.head_sha)],
    );
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. main needs 1 approval of aaaaaaa and it has none: dependabot[bot] approved aaaaaaa, but Orbit can't tell whether they have write access."
    );
    assert_eq!(
        sent(
            d.path(),
            "GET repos/a/b/collaborators/dependabot[bot]/permission"
        ),
        0
    );
    // A request for changes by someone without write access doesn't hold
    // it back.
    std::fs::remove_file(d.path().join("permission-error")).unwrap();
    reviews(
        d.path(),
        &[
            review("alice", "APPROVED", &p.head_sha),
            review("mallory", "CHANGES_REQUESTED", &p.head_sha),
        ],
    );
    again().await;
    watch().await;
    assert!(comment(d.path()).contains(clear));

    // A stricter rule on the rules' second page counts.
    let mut rules: Vec<_> = (0..100)
        .map(|_| json!({ "type": "required_linear_history" }))
        .collect();
    rules.push(
        json!({ "type": "pull_request", "parameters": { "required_approving_review_count": 2 } }),
    );
    std::fs::write(d.path().join("rules-main.json"), json!(rules).to_string()).unwrap();
    again().await;
    watch().await;
    assert_eq!(
        line(&e).await,
        "Not clear to merge yet. main needs 2 approvals of aaaaaaa and it has 1: mallory asked for changes on aaaaaaa, but without write access that doesn't hold it back."
    );
    // More rules than Orbit reads: it says so, and leaves the reviews to
    // GitHub.
    let rules: Vec<_> = (0..300)
        .map(|_| json!({ "type": "required_linear_history" }))
        .collect();
    std::fs::write(d.path().join("rules-main.json"), json!(rules).to_string()).unwrap();
    again().await;
    watch().await;
    let shown = e.shown_snapshot().await;
    assert_eq!(
        shown.readiness[0]
            .reviews
            .as_ref()
            .unwrap()
            .unseen
            .as_deref(),
        Some("main has more rules than Orbit reads, so Orbit can't tell whether it needs reviews.")
    );
    assert!(shown.readiness[0].clear);
    // Orbit asks about a few people's access at most.
    std::fs::write(d.path().join("rules-main.json"), one_approval).unwrap();
    let crowd: Vec<_> = (0..11)
        .map(|i| review(&format!("r{i}"), "APPROVED", &p.head_sha))
        .collect();
    reviews(d.path(), &crowd);
    again().await;
    watch().await;
    let asked = |who: &str| {
        sent(
            d.path(),
            &format!("GET repos/a/b/collaborators/{who}/permission"),
        )
    };
    assert_eq!((asked("r0"), asked("r9"), asked("r10")), (1, 1, 0));
    let shown = e.shown_snapshot().await;
    let r = shown.readiness[0].reviews.as_ref().unwrap();
    assert_eq!(r.reviewers[10].may_approve, None);
    assert!(shown.readiness[0].clear);
}
/// Waits for the fake GitHub to hold a call.
async fn held(dir: &Path) {
    let held = dir.join("held");
    for _ in 0..500 {
        if held.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let calls = std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
    panic!("GitHub was never asked: {calls}");
}
/// Turns automatic reviews off for `a/b` while GitHub holds a call during
/// `watch`, then lets it go.
async fn stop_reviewing_while_held(
    e: &std::sync::Arc<Engine>,
    dir: &Path,
    gh: &std::path::Path,
    endpoint: &str,
) {
    std::fs::write(dir.join("hold"), endpoint).unwrap();
    let watching = tokio::spawn(crate::github::GH.scope(gh.to_path_buf(), {
        let e = e.clone();
        async move { e.watch_ci().await }
    }));
    held(dir).await;
    e.update(|s| s.settings.repositories[0].automatic = false)
        .await;
    std::fs::remove_file(dir.join("hold")).unwrap();
    watching.await.unwrap();
}

/// An answer worked out before Orbit waits on GitHub isn't sent once it
/// has gone stale meanwhile: a pull request no longer reviewed
/// automatically while Orbit looks for its comment gets none, and one whose
/// replies Orbit is reading says Orbit stopped instead.
#[cfg(unix)]
#[tokio::test]
async fn an_answer_gone_stale_while_orbit_waits_isnt_sent() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.github_user = Some("me".into())).await;
    let p = reviewed(&e).await;
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    stop_reviewing_while_held(&e, d.path(), &gh, "repos/a/b/issues/1/comments").await;
    assert!(writes(d.path()).is_empty());
    let calls = std::fs::read_to_string(d.path().join("calls")).unwrap();
    assert!(calls.contains("GET repos/a/b/issues/1/comments\n"));

    // A comment about a failing check, whose replies Orbit reads first each
    // time. A push changes the answer, and Orbit stops reviewing it while
    // it reads them.
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.github_user = Some("me".into())).await;
    let p = reviewed(&e).await;
    answer_pull_failing(d.path(), &p.head_sha);
    crate::github::GH.scope(gh.clone(), e.watch_ci()).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/1/comments"]);
    let new = "b".repeat(40);
    e.update(|s| s.prs[0].head_sha = new.clone()).await;
    answer_pull_failing(d.path(), &new);
    stop_reviewing_while_held(&e, d.path(), &gh, "repos/a/b/issues/1/comments").await;
    assert_eq!(writes(d.path()).len(), 2);
    let body = comment(d.path());
    assert!(!body.contains("Orbit hasn't reviewed"), "{body}");
    assert!(
        body.contains("**Not clear to merge.** Orbit stopped working out whether it is clear to merge, as it no longer reviews it automatically."),
        "{body}"
    );
}

/// A pull request that closes with a check failing keeps Orbit's comment
/// as it was. Reopened with its checks passing and Orbit's review clean,
/// its comment says it is clear to merge, with nothing about CI from
/// before it closed. That holds when it is reopened while Orbit reads
/// another repository's CI, after reading that it closed: GitHub's time
/// for the reopening is then before Orbit's for the closing.
#[cfg(unix)]
#[tokio::test]
async fn a_reopened_pull_request_is_told_it_is_clear() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| {
        s.github_user = Some("me".into());
        let mut other = s.settings.repositories[0].clone();
        other.name = "c/d".into();
        other.automatic = false;
        s.settings.repositories.push(other);
    })
    .await;
    let p = reviewed(&e).await;
    let watch = || crate::github::GH.scope(gh.clone(), e.watch_ci());
    answer_pull_failing(d.path(), &p.head_sha);
    watch().await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/1/comments"]);
    assert!(comment(d.path()).contains("### CI"));
    // It closes. Orbit reads that, then waits on c/d's CI, and it is
    // reopened meanwhile.
    e.update(|s| s.prs.clear()).await;
    passing_pulls(d.path(), &[]);
    std::fs::write(d.path().join("hold"), "graphql c/d").unwrap();
    let watching = tokio::spawn(crate::github::GH.scope(gh.clone(), {
        let e = e.clone();
        async move { e.watch_ci().await }
    }));
    held(d.path()).await;
    let reopened = chrono::Utc::now().to_rfc3339();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    std::fs::remove_file(d.path().join("hold")).unwrap();
    watching.await.unwrap();
    let s = e.snapshot().await;
    let problem = &s.problems[0];
    assert_eq!(
        problem.closed_reason.as_deref(),
        Some("The pull request closed.")
    );
    assert!(problem.closed_at.as_deref().unwrap() > reopened.as_str());
    assert_eq!(writes(d.path()).len(), 1);
    // A sync finds it open, CI lists it with its checks passing, and
    // GitHub says it merges.
    let open = PullRequest {
        updated_at: reopened,
        ..p.clone()
    };
    e.update(|s| s.prs = vec![open]).await;
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    watch().await;
    let w = writes(d.path());
    assert_eq!(w.len(), 2);
    assert!(w[1].starts_with("PATCH repos/a/b/issues/comments/"));
    let body = comment(d.path());
    assert!(
        body.starts_with(
            "## Orbit\n\n**Clear to merge** at `aaaaaaa` against `main` at `1111111`."
        ),
        "{body}"
    );
    assert!(!body.contains("### CI"), "{body}");
    watch().await;
    assert_eq!(writes(d.path()).len(), 2);
}

/// GitHub's CI for `a/b` with pull request 1's one check failing at `head`.
fn answer_pull_failing(dir: &Path, head: &str) {
    passing_pull(dir, head);
    let text = std::fs::read_to_string(dir.join("answer.json")).unwrap();
    let text = text.replacen(
        "\"conclusion\":\"SUCCESS\"",
        "\"conclusion\":\"FAILURE\"",
        2,
    );
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    // The default branch still passes.
    v["data"]["repository"]["defaultBranchRef"]["target"]["statusCheckRollup"]["contexts"]
        ["nodes"][0]["conclusion"] = json!("SUCCESS");
    std::fs::write(dir.join("answer.json"), v.to_string()).unwrap();
}

/// GitHub's answer that comes back after Watch CI went off, the
/// repository was disconnected or the pull request became a draft is
/// dropped.
#[cfg(unix)]
#[tokio::test]
async fn a_merge_answer_after_a_change_is_dropped() {
    for change in ["disconnect", "watch off", "draft"] {
        let d = tempfile::tempdir().unwrap();
        let gh = fixture_gh(d.path());
        let e = connected(d.path()).await;
        let p = reviewed(&e).await;
        passing_pull(d.path(), &p.head_sha);
        mergeable(d.path(), &p.head_sha, json!(true), "clean");
        // GitHub takes its time over the pull request.
        std::fs::write(d.path().join("hold"), "repos/a/b/pulls/1").unwrap();
        let reading = tokio::spawn(crate::github::GH.scope(gh.clone(), {
            let e = e.clone();
            async move { e.watch_ci().await }
        }));
        let held = d.path().join("held");
        for _ in 0..500 {
            if held.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(held.exists(), "{change}");
        e.update(|s| match change {
            "disconnect" => s.settings.repositories.clear(),
            "draft" => s.prs[0].draft = true,
            _ => s.settings.watch_ci = false,
        })
        .await;
        std::fs::remove_file(d.path().join("hold")).unwrap();
        reading.await.unwrap();
        assert!(e.shown_snapshot().await.merges.is_empty(), "{change}");
    }
}

/// A rate limit stops the pass: the next pull request waits.
#[cfg(unix)]
#[tokio::test]
async fn a_rate_limit_stops_asking_whether_pull_requests_merge() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    let p = reviewed(&e).await;
    let mut second = p.clone();
    second.number = 2;
    second.id = pr_id("a/b", 2);
    second.head_sha = "2".repeat(40);
    e.update(|s| {
        let mut r = Review::new(&second, &s.settings, true);
        r.status = "posted".into();
        r.report = "Report".into();
        r.findings = Some(vec![]);
        crate::review_loop::decide(&mut r, Some(1));
        s.reviews.push(r);
        s.prs.push(second.clone());
    })
    .await;
    passing_pulls(d.path(), &[(1, &p.head_sha), (2, &second.head_sha)]);
    std::fs::write(
        d.path().join("pull-error"),
        "gh: API rate limit exceeded for user ID 1. (HTTP 403)\n",
    )
    .unwrap();
    crate::github::GH.scope(gh, e.watch_ci()).await;
    let asked = sent(d.path(), "GET repos/a/b/pulls/1") + sent(d.path(), "GET repos/a/b/pulls/2");
    assert_eq!(asked, 1);
    assert_eq!(e.shown_snapshot().await.merges.len(), 1);
}

/// The morning brief's Done when, end to end with the fake `gh`: on a day
/// with changes, one brief comes after Orbit reads CI, told once, in Needs
/// you and in Bit's conversation, and no other that day. A quiet day sends
/// none and takes it out of Needs you. A day with something settled sends
/// one again, until you mark it read.
#[cfg(unix)]
#[tokio::test]
async fn a_day_with_changes_gives_one_brief_and_a_quiet_day_none() {
    let d = tempfile::tempdir().unwrap();
    let gh = fixture_gh(d.path());
    let e = connected(d.path()).await;
    let titles = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let told = titles.clone();
    e.on_notice(move |n| told.lock().unwrap().push(n.title.clone()));
    // In a time zone you chose, whose day isn't this computer's: the brief
    // is for your day. (These two zones are 26 hours apart, so one of them
    // always is on another day.)
    let mine = |zone: &str| crate::clock::local(zone, chrono::Utc::now()).date();
    let computer = chrono::Local::now().date_naive();
    let zone = ["Pacific/Kiritimati", "Etc/GMT+12"]
        .into_iter()
        .find(|z| mine(z) != computer)
        .unwrap();
    e.update(|s| {
        s.github_user = Some("me".into());
        s.settings.buddy.brief = true;
        s.settings.buddy.brief_at = "00:00".into();
        s.settings.buddy.time_zone = zone.into();
    })
    .await;
    let p = reviewed(&e).await;
    passing_pull(d.path(), &p.head_sha);
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    let watch = || crate::github::GH.scope(gh.clone(), e.watch_ci());
    let briefs = || async {
        let s = e.shown_snapshot().await;
        let notes: Vec<_> = s
            .buddy_feed
            .notes
            .into_iter()
            .filter(|n| n.kind == NoteKind::Brief)
            .collect();
        let items: Vec<_> = s
            .attention
            .into_iter()
            .filter(|a| a.kind == AttentionKind::Brief)
            .collect();
        let told = titles
            .lock()
            .unwrap()
            .iter()
            .filter(|t| *t == "Your morning brief")
            .count();
        (s.brief, notes, items, told)
    };
    // GitHub fails that morning: no brief from what Orbit couldn't read,
    // and the day isn't used up.
    std::fs::write(d.path().join("down"), "").unwrap();
    watch().await;
    let (b, notes, items, told) = briefs().await;
    assert_eq!(b, crate::brief::Briefs::default());
    assert_eq!((notes.len(), items.len(), told), (0, 0, 0));
    std::fs::remove_file(d.path().join("down")).unwrap();
    // A day with a pull request clear to merge.
    watch().await;
    let (b, notes, items, told) = briefs().await;
    let today = mine(zone).format("%Y-%m-%d").to_string();
    let first = b.latest.unwrap();
    assert_eq!(
        (b.looked.as_str(), first.day.as_str()),
        (today.as_str(), today.as_str())
    );
    let text = "Here's your morning brief! 1 pull request is clear to merge.";
    assert_eq!(first.text, text);
    assert_eq!(
        first
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["b#1: Clear to merge at aaaaaaa against main at 1111111."]
    );
    assert_eq!((notes.len(), items.len(), told), (1, 1, 1));
    assert_eq!(notes[0].text, text);
    assert_eq!(notes[0].lines[0].text, first.lines[0].text);
    assert_eq!(items[0].reason, text);
    // Later that day, nothing more, even once something changes.
    mergeable(d.path(), &p.head_sha, json!(false), "dirty");
    e.update(|s| s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into())
        .await;
    watch().await;
    let (b, notes, items, told) = briefs().await;
    assert!(line(&e)
        .await
        .starts_with("Not clear to merge yet. It conflicts"));
    assert_eq!(b.latest.unwrap().day, today);
    assert_eq!((notes.len(), items.len(), told), (1, 1, 1));
    // A quiet day: clear again, as in the last brief. None, and the brief
    // leaves Needs you. (Orbit's record goes back a day, as if a day had
    // passed, with the brief told on that day.)
    mergeable(d.path(), &p.head_sha, json!(true), "clean");
    e.update(|s| {
        s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into();
        s.brief.looked = "2000-01-01".into();
        s.brief.latest.as_mut().unwrap().day = "2000-01-01".into();
        for key in &mut s.announced {
            if key.starts_with("brief:") {
                *key = "brief:2000-01-01".into();
            }
        }
    })
    .await;
    watch().await;
    let (b, notes, items, told) = briefs().await;
    assert!(line(&e).await.starts_with("Clear to merge"));
    assert_eq!(b.looked, today);
    let latest = b.latest.unwrap();
    assert_eq!((latest.day.as_str(), latest.open), ("2000-01-01", false));
    assert_eq!((notes.len(), items.len(), told), (1, 0, 1));
    // The next day it conflicts: the line from the last brief settled.
    mergeable(d.path(), &p.head_sha, json!(false), "dirty");
    e.update(|s| {
        s.merges[0].read_at = "2020-01-01T00:00:00.000Z".into();
        s.brief.looked = "2000-01-02".into();
    })
    .await;
    watch().await;
    let (b, notes, items, told) = briefs().await;
    let text = "Here's your morning brief! Nothing is clear to merge, stuck or failing. 1 thing from the last brief has settled.";
    let latest = b.latest.unwrap();
    assert_eq!(
        (latest.day.as_str(), latest.text.as_str()),
        (today.as_str(), text)
    );
    assert_eq!((notes.len(), items.len(), told), (2, 1, 2));
    assert_eq!(notes[1].text, text);
    // You read it.
    e.close_brief("2000-01-01").await.unwrap();
    assert_eq!(briefs().await.2.len(), 1);
    e.close_brief(&today).await.unwrap();
    let (b, _, items, told) = briefs().await;
    assert!(items.is_empty() && !b.latest.unwrap().open);
    assert_eq!(told, 2);
    // Saved: a restart neither brings it back nor tells it again.
    drop(e);
    let e = connected(d.path()).await;
    let s = e.shown_snapshot().await;
    assert!(!s.attention.iter().any(|a| a.kind == AttentionKind::Brief));
    assert_eq!(s.brief.looked, today);
}
