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
