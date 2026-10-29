//! Orbit's comments and issues, end to end with a fake GitHub
//! (`gh_fixture.cjs`) that keeps the issues and comments it is sent.
use super::Engine;
use crate::ops::{ThreadKind, WriteState};
use crate::types::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
fn run(name: &str, result: &str) -> Value {
    json!({
        "__typename": "CheckRun",
        "name": name,
        "status": "COMPLETED",
        "conclusion": result,
        "detailsUrl": "https://github.com/a/b/actions/runs/1/job/2",
        "checkSuite": { "workflowRun": { "workflow": { "name": "CI" } } }
    })
}
fn rollup(checks: &[(&str, &str)]) -> Value {
    let nodes: Vec<Value> = checks.iter().map(|(n, r)| run(n, r)).collect();
    json!({ "contexts": {
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": nodes
    } })
}
/// GitHub's CI for `a/b`: its default branch at `head` with these checks,
/// and pull request 12 at `pull` with these, if any.
fn answer(dir: &Path, head: char, checks: &[(&str, &str)], pull: Option<(char, &[(&str, &str)])>) {
    let pulls: Vec<Value> = pull
        .into_iter()
        .map(|(at, checks)| {
            json!({
                "number": 12,
                "isDraft": false,
                "headRefOid": sha(at),
                "commits": { "nodes": [{ "commit": {
                    "oid": sha(at),
                    "statusCheckRollup": rollup(checks)
                } }] }
            })
        })
        .collect();
    let v = json!({ "data": { "repository": {
        "defaultBranchRef": { "name": "main", "target": {
            "oid": sha(head),
            "messageHeadline": "Update the build",
            "statusCheckRollup": rollup(checks)
        } },
        "pullRequests": { "pageInfo": { "hasNextPage": false }, "nodes": pulls }
    } } });
    std::fs::write(dir.join("answer.json"), v.to_string()).unwrap();
}
#[cfg(unix)]
fn fake_gh(dir: &Path) -> PathBuf {
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
/// What the fake GitHub holds.
fn github(dir: &Path) -> Value {
    std::fs::read_to_string(dir.join("github.json"))
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or(json!({ "issues": [], "comments": [] }))
}
/// The writes the fake GitHub was sent.
fn writes(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("GET ") && !l.contains("graphql"))
        .map(str::to_string)
        .collect()
}
async fn connected(dir: &Path) -> Arc<Engine> {
    let e = Engine::new(dir.join("state"), |_| {}).unwrap();
    e.update(|s| {
        s.github_user = Some("me".into());
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
fn pr(head: char) -> PullRequest {
    PullRequest {
        id: "a/b#12".into(),
        repo: "a/b".into(),
        number: 12,
        title: "Fix it".into(),
        author: "agent".into(),
        body: String::new(),
        url: "https://github.com/a/b/pull/12".into(),
        head_sha: sha(head),
        base_sha: sha('0'),
        head_ref: "fix-it".into(),
        base_ref: "main".into(),
        updated_at: now(),
        draft: false,
        additions: 1,
        deletions: 0,
        changed_files: 1,
        review_requested: false,
        labels: vec![],
        head_repo: "a/b".into(),
        author_association: "MEMBER".into(),
        base_repo: "a/b".into(),
        head_seen_at: String::new(),
    }
}
#[cfg(unix)]
async fn watch(e: &Engine, gh: &Path) {
    crate::github::GH
        .scope(gh.to_path_buf(), e.watch_ci())
        .await;
}
/// As if Orbit stopped once its first post reached GitHub, before GitHub
/// answered.
async fn cut_off(e: &Engine) {
    e.update(|s| {
        let t = &mut s.ops[0];
        if t.kind == ThreadKind::Issue {
            t.number = None;
        }
        t.comment_id = None;
        t.url.clear();
        t.posted.clear();
        let w = &mut s.journal[0];
        w.state = WriteState::Sending;
        w.url.clear();
        w.done_at = None;
    })
    .await;
    e.flush().await.unwrap();
}

/// A broken default branch opens one problem, one issue and one Needs you
/// item; a restart posts nothing again; green says so on the issue, and an
/// hour of green closes the issue with a note.
#[cfg(unix)]
#[tokio::test]
async fn a_broken_default_branch_opens_one_issue_and_green_closes_it() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.problems.len(), 1);
    assert_eq!(shown.attention.len(), 1);
    assert_eq!(shown.attention[0].kind, AttentionKind::CiFailing);
    let issues = github(d.path())["issues"].clone();
    assert_eq!(issues.as_array().unwrap().len(), 1);
    assert_eq!(issues[0]["title"], "CI is failing on main");
    assert_eq!(issues[0]["labels"][0]["name"], "orbit");
    let body = issues[0]["body"].as_str().unwrap();
    assert!(body.contains("- **I1** · `CI / Rust` · failing at `1111111`"));
    assert_eq!(shown.ops.len(), 1);
    assert_eq!(shown.ops[0].kind, ThreadKind::Issue);
    assert_eq!(shown.ops[0].number, issues[0]["number"].as_u64());
    assert_eq!(shown.journal.len(), 1);
    assert_eq!(shown.journal[0].state, WriteState::Done);
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);

    // Orbit restarts and CI is still red: nothing is posted again.
    drop(e);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 1);

    // A fix lands: the issue says so, and stays open for now.
    answer(d.path(), '2', &[("Rust", "SUCCESS")], None);
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.attention.is_empty());
    assert!(!shown.problems[0].is_open());
    let issue = &github(d.path())["issues"][0];
    assert_eq!(issue["state"], "open");
    assert!(issue["body"]
        .as_str()
        .unwrap()
        .contains("Every check passes on `main` at `2222222`."));
    let number = issue["number"].as_u64().unwrap();
    assert_eq!(writes(d.path()).len(), 2);

    // An hour later it is still green: a note, then the issue closes.
    e.update(|s| {
        let an_hour_ago = chrono::Utc::now() - crate::ops::CLOSE_AFTER;
        s.problems[0].closed_at = Some(crate::problems::stamp(an_hour_ago));
    })
    .await;
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"][0]["state"], "closed");
    assert_eq!(g["issues"][0]["state_reason"], "completed");
    let notes = g["comments"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["issue"].as_u64(), Some(number));
    assert!(notes[0]["body"]
        .as_str()
        .unwrap()
        .starts_with("Every check passed on `main` at `2222222` and stayed green for an hour"));
    assert_eq!(writes(d.path()).len(), 4);
    drop(e);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 4, "and nothing after a restart");
    let journal = e.snapshot().await.journal;
    assert!(journal.iter().all(|w| w.state == WriteState::Done));
}

/// A write GitHub got just before Orbit stopped, before Orbit heard back,
/// is found on GitHub and not sent twice.
#[cfg(unix)]
#[tokio::test]
async fn a_write_cut_off_by_a_restart_is_not_sent_twice() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    cut_off(&e).await;
    drop(e);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"].as_array().unwrap().len(), 1);
    assert_eq!(
        writes(d.path()),
        [
            "POST repos/a/b/issues",
            &format!("PATCH repos/a/b/issues/{}", g["issues"][0]["number"])
        ]
    );
    let s = e.snapshot().await;
    assert_eq!(s.ops[0].number, g["issues"][0]["number"].as_u64());
    assert_eq!(s.journal[0].state, WriteState::Failed);
    assert!(s.journal[0]
        .error
        .as_deref()
        .unwrap()
        .contains("nothing is posted twice"));
    assert_eq!(s.journal[1].state, WriteState::Done);
}

/// A pull request whose checks fail gets one comment from Orbit, edited as
/// they change, until they pass.
#[cfg(unix)]
#[tokio::test]
async fn a_failing_pull_request_gets_one_comment_kept_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Rust", "FAILURE")])),
    );
    watch(&e, &gh).await;
    let comments = github(d.path())["comments"].clone();
    assert_eq!(comments.as_array().unwrap().len(), 1);
    assert_eq!(comments[0]["issue"], 12);
    assert!(comments[0]["body"]
        .as_str()
        .unwrap()
        .contains("1 check fails on this pull request at `5555555`."));
    assert!(
        e.shown_snapshot().await.attention.is_empty(),
        "the agent's to fix"
    );

    e.update(|s| s.prs = vec![pr('6')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('6', &[("Rust", "SUCCESS"), ("Lint", "FAILURE")])),
    );
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["comments"].as_array().unwrap().len(), 1);
    let body = g["comments"][0]["body"].as_str().unwrap();
    assert!(body.contains("- **I2** · `CI / Lint` · failing at `6666666`"));
    assert!(body.contains("- **I1** · `CI / Rust` · not failing at `6666666`"));

    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('6', &[("Lint", "SUCCESS")])),
    );
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("Every check passes at `6666666`."));
    let id = github(d.path())["comments"][0]["id"].clone();
    assert_eq!(
        writes(d.path()),
        [
            "POST repos/a/b/issues/12/comments".to_string(),
            format!("PATCH repos/a/b/issues/comments/{id}"),
            format!("PATCH repos/a/b/issues/comments/{id}"),
        ]
    );
}

/// With Leave it to me nothing is posted; with Ask first nothing is posted
/// until you say yes; with Bit off nothing new is posted.
#[cfg(unix)]
#[tokio::test]
async fn the_rules_decide_what_bit_posts() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.rules.open_issues = "leave_it".into())
        .await;
    watch(&e, &gh).await;
    assert!(writes(d.path()).is_empty());
    assert_eq!(e.snapshot().await.ops.len(), 1, "kept, to post later");

    e.update(|s| s.settings.buddy.enabled = false).await;
    e.update(|s| s.settings.buddy.rules.open_issues = "do_it".into())
        .await;
    watch(&e, &gh).await;
    assert!(writes(d.path()).is_empty(), "Bit is off");

    e.update(|s| s.settings.buddy.enabled = true).await;
    e.update(|s| s.settings.buddy.rules.open_issues = "ask_first".into())
        .await;
    watch(&e, &gh).await;
    assert!(writes(d.path()).is_empty());
    let key = e.snapshot().await.ops[0].key.clone();
    assert!(e.allow_telling("issue:nothing").await.is_err());
    e.allow_telling(&key).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
}

/// When GitHub doesn't answer, the write is recorded as failed and tried
/// again later, not on every pass.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_write_is_tried_again_later() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let issues = github(d.path())["issues"].clone();
    assert_eq!(issues.as_array().unwrap().len(), 1);
    // The next change can't reach GitHub.
    answer(d.path(), '2', &[("Rust", "FAILURE")], None);
    crate::github::GH
        .scope(gh.clone(), async {
            e.watch_ci().await;
        })
        .await;
    std::fs::write(d.path().join("down"), "").unwrap();
    e.update(|s| s.ops[0].posted.clear()).await;
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    let s = e.snapshot().await;
    let failed = s.journal.last().unwrap();
    assert_eq!(failed.state, WriteState::Failed);
    assert!(failed.error.as_deref().unwrap().contains("Bad Gateway"));
    assert!(s.ops[0].error.as_deref().unwrap().contains("Bad Gateway"));
    assert!(s.ops[0].retry_at.is_some());
    let tried = writes(d.path()).len();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(writes(d.path()).len(), tried, "not again straight away");
}

/// A post cut off by a restart is looked for first, whatever changed while
/// Orbit was stopped: CI went green, or the rule became Leave it to me.
/// What it finds is kept true, and an issue still closes when it should.
#[cfg(unix)]
#[tokio::test]
async fn a_cut_off_post_is_found_whatever_changed_meanwhile() {
    // CI went green.
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    cut_off(&e).await;
    drop(e);
    answer(d.path(), '2', &[("Rust", "SUCCESS")], None);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"].as_array().unwrap().len(), 1);
    let number = g["issues"][0]["number"].as_u64().unwrap();
    assert!(g["issues"][0]["body"]
        .as_str()
        .unwrap()
        .contains("Every check passes on `main` at `2222222`."));
    assert_eq!(
        writes(d.path()),
        [
            "POST repos/a/b/issues".to_string(),
            format!("PATCH repos/a/b/issues/{number}")
        ]
    );
    let s = e.snapshot().await;
    assert_eq!(s.ops[0].number, Some(number));
    assert!(!s.ops[0].unsure);
    // An hour of green closes it, as if nothing had been cut off.
    e.update(|s| {
        let an_hour_ago = chrono::Utc::now() - crate::ops::CLOSE_AFTER;
        s.problems[0].closed_at = Some(crate::problems::stamp(an_hour_ago));
    })
    .await;
    watch(&e, &gh).await;
    assert_eq!(github(d.path())["issues"][0]["state"], "closed");

    // The rule became Leave it to me, and the pull request failed again.
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Rust", "FAILURE")])),
    );
    watch(&e, &gh).await;
    cut_off(&e).await;
    drop(e);
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('6')];
        s.settings.buddy.rules.comment_on_prs = "leave_it".into();
    })
    .await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('6', &[("Rust", "FAILURE")])),
    );
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["comments"].as_array().unwrap().len(), 1);
    assert!(g["comments"][0]["body"]
        .as_str()
        .unwrap()
        .contains("1 check fails on this pull request at `6666666`."));
    let id = &g["comments"][0]["id"];
    assert_eq!(
        writes(d.path()),
        [
            "POST repos/a/b/issues/12/comments".to_string(),
            format!("PATCH repos/a/b/issues/comments/{id}")
        ]
    );
    assert_eq!(e.snapshot().await.ops[0].comment_id, id.as_u64());
}

/// Saying yes to Ask first posts nothing once Orbit stopped watching the
/// repository, even before the next CI read: it was disconnected, or Watch
/// CI was turned off.
#[cfg(unix)]
#[tokio::test]
async fn saying_yes_posts_nothing_once_orbit_stopped_watching() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.rules.open_issues = "ask_first".into())
        .await;
    watch(&e, &gh).await;
    assert!(writes(d.path()).is_empty());
    let s = e.snapshot().await;
    let key = s.ops[0].key.clone();
    let repositories = s.settings.repositories.clone();

    e.update(|s| s.settings.repositories.clear()).await;
    e.allow_telling(&key).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert!(
        writes(d.path()).is_empty(),
        "the repository was disconnected"
    );

    e.update(|s| {
        s.settings.repositories = repositories;
        s.settings.watch_ci = false;
    })
    .await;
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert!(writes(d.path()).is_empty(), "Watch CI is off");

    e.update(|s| s.settings.watch_ci = true).await;
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
}

/// A rule changed while Orbit looks on GitHub before posting stops the post.
#[cfg(unix)]
#[tokio::test]
async fn a_rule_changed_during_the_search_stops_the_post() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    // GitHub takes its time to answer the search.
    std::fs::write(d.path().join("hold"), "").unwrap();
    let watching = tokio::spawn(crate::github::GH.scope(gh.clone(), {
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
    assert!(held.exists(), "Orbit looks before posting");
    e.update(|s| s.settings.buddy.rules.open_issues = "leave_it".into())
        .await;
    std::fs::remove_file(d.path().join("hold")).unwrap();
    watching.await.unwrap();
    assert!(writes(d.path()).is_empty());
    let calls = std::fs::read_to_string(d.path().join("calls")).unwrap();
    assert!(calls.contains("GET repos/a/b/issues\n"));
}

/// Looking for Orbit's earlier post fails when there are more full pages
/// than it checks, rather than passing for finding nothing.
#[cfg(unix)]
#[tokio::test]
async fn a_search_too_big_to_finish_finds_nothing_it_can_trust() {
    use crate::github::{find_comment_within, find_issue_within};
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let marker = crate::ops::tag("pr:a/b#12");
    // `total` comments by Orbit on pull request 12, the one at `found`
    // holding the marker; or as many issues.
    let seed = |total: u64, found: Option<u64>| {
        let items: Vec<Value> = (1..=total)
            .map(|n| {
                let body = if Some(n) == found {
                    marker.as_str()
                } else {
                    "Something else."
                };
                json!({
                    "id": n,
                    "number": n,
                    "issue": 12,
                    "user": { "login": "me" },
                    "body": body,
                    "html_url": format!("https://github.com/a/b/issues/{n}"),
                })
            })
            .collect();
        let db = json!({ "next": 1000, "issues": items, "comments": items });
        std::fs::write(d.path().join("github.json"), db.to_string()).unwrap();
    };
    let comment =
        || crate::github::GH.scope(gh.clone(), find_comment_within("a/b", 12, &marker, "me", 2));
    let issue = || crate::github::GH.scope(gh.clone(), find_issue_within("a/b", &marker, "me", 2));

    seed(201, Some(201));
    assert!(comment()
        .await
        .unwrap_err()
        .starts_with("Too many to check"));
    seed(200, None);
    assert!(comment().await.is_err(), "two full pages may not be all");
    seed(200, Some(150));
    assert_eq!(comment().await.unwrap().unwrap().id, 150);
    seed(150, None);
    assert_eq!(comment().await.unwrap(), None);

    // Issues come newest first, so the oldest is on the last page.
    seed(250, Some(1));
    assert!(issue().await.unwrap_err().starts_with("Too many to check"));
    seed(150, Some(1));
    assert_eq!(issue().await.unwrap().unwrap().number, Some(1));
    seed(150, None);
    assert_eq!(issue().await.unwrap(), None);
}
