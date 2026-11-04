//! Orbit's comments and issues, end to end with a fake GitHub
//! (`gh_fixture.cjs`) that keeps the issues and comments it is sent.
use super::Engine;
use crate::ops::{ThreadKind, WriteState};
use crate::types::*;
use chrono::TimeZone;
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
pub(super) fn answer(
    dir: &Path,
    head: char,
    checks: &[(&str, &str)],
    pull: Option<(char, &[(&str, &str)])>,
) {
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
    // Orbit's sign-in is an admin, so its posts get no second look.
    let v = json!({ "data": { "repository": {
        "viewerPermission": "ADMIN",
        "isPrivate": false,
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
pub(super) fn fake_gh(dir: &Path) -> PathBuf {
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
pub(super) fn github(dir: &Path) -> Value {
    std::fs::read_to_string(dir.join("github.json"))
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or(json!({ "issues": [], "comments": [] }))
}
/// The writes the fake GitHub was sent.
pub(super) fn writes(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("GET ") && !l.contains("graphql"))
        .map(str::to_string)
        .collect()
}
pub(super) async fn connected(dir: &Path) -> Arc<Engine> {
    let e = Engine::new(dir.join("state"), |_| {}).unwrap();
    e.update(|s| {
        s.github_user = Some("me".into());
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
pub(super) fn pr(head: char) -> PullRequest {
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
pub(super) async fn watch(e: &Engine, gh: &Path) {
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
        t.shown_at.clear();
        t.read_to = 0;
        t.read_since.clear();
        for item in &mut t.items {
            item.heard_from.clear();
        }
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
    // With no history from GitHub, it names nothing, and doesn't ask again.
    let broke = shown.problems[0].broke.clone().unwrap();
    assert!(broke.done && broke.commits.is_empty());
    assert_eq!(
        broke.error.as_deref(),
        Some("GitHub no longer has commit 1111111.")
    );

    // Orbit restarts and CI is still red: nothing is posted again.
    drop(e);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 1);
    assert_eq!(calls(d.path(), "GET history/"), 1);

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

/// The fake GitHub's history from `at`: that one commit, with its first
/// parent, its Rust check and the pull request merged as it, so the next
/// commit back is read from its own file.
fn history(dir: &Path, at: char, parent: char, result: &str, pull: u64) {
    let node = json!({
        "oid": sha(at),
        "parents": { "nodes": [{ "oid": sha(parent) }] },
        "statusCheckRollup": rollup(&[("Rust", result)]),
        "associatedPullRequests": { "nodes": [
            { "number": pull, "merged": true, "baseRefName": "main" }
        ] }
    });
    std::fs::write(
        dir.join(format!("history-{}.json", sha(at))),
        json!({ "data": { "repository": { "object": { "history": { "nodes": [node] } } } } })
            .to_string(),
    )
    .unwrap();
}

/// A broken default branch's issue names the pull request that broke it,
/// from the branch's history, read once.
#[cfg(unix)]
#[tokio::test]
async fn a_broken_default_branch_names_the_pull_request_that_broke_it() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '2', &[("Rust", "FAILURE")], None);
    history(d.path(), '2', '1', "FAILURE", 42);
    history(d.path(), '1', '0', "SUCCESS", 41);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let issue = &github(d.path())["issues"][0];
    assert!(issue["body"].as_str().unwrap().contains(
        "\n\nIt started failing with #42, merged as `2222222`; `1111111` before it passed.\n\n"
    ));
    let s = e.snapshot().await;
    assert_eq!(s.problems[0].broke.as_ref().unwrap().pull, Some(42));
    let shown = e.shown_snapshot().await;
    assert!(shown.attention[0].reason.ends_with(
        ": CI / Rust. It started failing with #42, merged as 2222222; 1111111 before it passed."
    ));
    assert_eq!(calls(d.path(), "GET history/"), 2);

    // Still failing at a newer commit: read once, so not again.
    answer(d.path(), '3', &[("Rust", "FAILURE")], None);
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "GET history/"), 2);
    assert!(github(d.path())["issues"][0]["body"]
        .as_str()
        .unwrap()
        .contains("It started failing with #42"));
}

/// A branch that passes and breaks again within the hour is the same
/// problem, but what broke it is read again from the new failure.
#[cfg(unix)]
#[tokio::test]
async fn a_branch_that_breaks_again_soon_names_what_broke_it_this_time() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '2', &[("Rust", "FAILURE")], None);
    history(d.path(), '2', '1', "FAILURE", 42);
    history(d.path(), '1', '0', "SUCCESS", 41);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    answer(d.path(), '3', &[("Rust", "SUCCESS")], None);
    watch(&e, &gh).await;
    answer(d.path(), '4', &[("Rust", "FAILURE")], None);
    history(d.path(), '4', '3', "FAILURE", 44);
    history(d.path(), '3', '2', "SUCCESS", 43);
    watch(&e, &gh).await;

    let s = e.snapshot().await;
    assert_eq!(s.problems.len(), 1, "the same problem, back again");
    assert_eq!(s.problems[0].since_sha, sha('4'));
    let check = |issue: &Value, shown: &Snapshot| {
        let body = issue["body"].as_str().unwrap();
        assert!(body.contains("at `4444444`."), "{body}");
        assert!(body.contains(
            "\n\nIt started failing with #44, merged as `4444444`; `3333333` before it passed.\n\n"
        ));
        assert!(!body.contains("#42"));
        assert_eq!(
            shown.attention[0].reason,
            "CI is failing on main in a/b since 4444444: CI / Rust. \
             It started failing with #44, merged as 4444444; 3333333 before it passed."
        );
    };
    let g = github(d.path());
    assert_eq!(g["issues"].as_array().unwrap().len(), 1);
    check(&g["issues"][0], &e.shown_snapshot().await);
    assert_eq!(calls(d.path(), "GET history/"), 4);

    // After a restart, the same, without reading the history again.
    drop(e);
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"].as_array().unwrap().len(), 1);
    check(&g["issues"][0], &e.shown_snapshot().await);
    assert_eq!(calls(d.path(), "GET history/"), 4);
}

/// A branch that breaks again hours after it passed opens a new problem,
/// whose issue and Needs you item say what happened last time; once a
/// check has passed by bad luck three times, Orbit learns it, the issue
/// says so and Bit notes it.
#[cfg(unix)]
#[tokio::test]
async fn a_branch_that_breaks_again_says_what_happened_last_time() {
    use crate::cases::{Case, End};
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.enabled = true).await;
    watch(&e, &gh).await;
    answer(d.path(), '2', &[("Rust", "SUCCESS")], None);
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    assert_eq!(s.cases.len(), 1);
    assert_eq!(
        (s.cases[0].end, s.cases[0].passed.as_str()),
        (End::Passed, &*sha('2'))
    );
    // As if it failed three hours ago and passed two hours ago.
    e.update(|s| {
        let ago = |h: i64| crate::problems::stamp(chrono::Utc::now() - chrono::Duration::hours(h));
        s.problems[0].opened_at = ago(3);
        s.problems[0].closed_at = Some(ago(2));
        s.cases[0].opened_at = ago(3);
        s.cases[0].closed_at = Some(ago(2));
    })
    .await;
    watch(&e, &gh).await;
    assert_eq!(github(d.path())["issues"][0]["state"], "closed");

    answer(d.path(), '3', &[("Rust", "FAILURE")], None);
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(shown.problems.len(), 2);
    let last = "Last time, 2 hours earlier: CI / Rust failed on main. It passed again at 2222222, an hour later.";
    assert_eq!(
        shown.problems[1]
            .last_time
            .as_ref()
            .map(|l| l.text.as_str()),
        Some(last)
    );
    assert!(
        shown.attention[0].reason.ends_with(last),
        "{}",
        shown.attention[0].reason
    );
    let issues = github(d.path())["issues"].clone();
    assert_eq!(issues.as_array().unwrap().len(), 2);
    let body = issues[1]["body"].as_str().unwrap().to_string();
    assert!(body.contains("Orbit has seen this before:\n\n- Last time, 2 hours earlier: `CI / Rust` failed on `main`. It passed again at `2222222`, an hour later.\n\n"), "{body}");
    assert!(!body.contains("It was fixed before"));

    // It passed by bad luck three times lately: a lesson, on the issue and
    // in Bit's notes.
    e.update(|s| {
        for n in 0..3 {
            let at = |h: i64| {
                crate::problems::stamp(
                    chrono::Utc::now() - chrono::Duration::hours(24 * (n + 1) + h),
                )
            };
            s.cases.push(Case {
                id: format!("old{n}"),
                key: "ci:a/b".into(),
                repo: "a/b".into(),
                branch: "main".into(),
                checks: vec!["CI / Rust".into()],
                last: vec!["CI / Rust".into()],
                sha: sha('0'),
                opened_at: at(1),
                closed_at: Some(at(0)),
                end: End::Again,
                passed: sha('0'),
                ..Default::default()
            });
        }
    })
    .await;
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    assert_eq!(s.lessons.len(), 1);
    assert_eq!(s.problems[1].lessons.len(), 1);
    let lesson = "`CI / Rust` in a/b failed and then passed when it ran again at the same commit 3 times in 30 days.";
    let body = github(d.path())["issues"][1]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        body.contains(&format!("\n- {lesson} When it fails again")),
        "{body}"
    );
    let note = s
        .buddy_feed
        .notes
        .iter()
        .find(|n| n.key.as_deref() == Some("lesson:luck:a/b:CI / Rust"))
        .expect("Bit noted it");
    assert!(
        note.text
            .contains("CI / Rust in a/b failed and then passed"),
        "{}",
        note.text
    );
    // Learned once.
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let notes = s
        .buddy_feed
        .notes
        .iter()
        .filter(|n| n.key.as_deref().is_some_and(|k| k.starts_with("lesson:")));
    assert_eq!(notes.count(), 1);
}

/// A broken default branch's playbook, ticked as Orbit follows it, with
/// what done looks like in its issue while it is open.
#[tokio::test]
async fn a_broken_branch_follows_its_playbook_to_staying_fixed() {
    use crate::playbooks::{Mark::*, StepKind::*};
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.enabled = true).await;
    watch(&e, &gh).await;
    watch(&e, &gh).await;
    let marks = |p: &crate::problems::Problem| {
        p.playbook
            .iter()
            .map(|s| (s.step, s.mark))
            .collect::<Vec<_>>()
    };
    // The fake GitHub has no history, so Orbit couldn't tell what broke it.
    let s = e.snapshot().await;
    assert_eq!(
        marks(&s.problems[0]),
        [
            (Found, Missed),
            (Told, Done),
            (Taken, Todo),
            (Fixed, Todo),
            (Passes, Todo),
            (Holds, Todo)
        ]
    );
    let body = github(d.path())["issues"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        body.contains("What done looks like:\n\n1. An agent takes each item.\n2. A fix lands on `main`.\n3. Every check passes on `main`.\n4. It stays fixed for 3 days.\n\n"),
        "{body}"
    );

    answer(d.path(), '2', &[("Rust", "SUCCESS")], None);
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let p = &s.problems[0];
    assert_eq!(
        marks(p),
        [
            (Found, Missed),
            (Told, Done),
            (Taken, Todo),
            (Fixed, Done),
            (Passes, Done),
            (Holds, Todo)
        ]
    );
    assert_eq!(p.playbook[3].note, "It passed at 2222222.");
    let due = p.playbook[5]
        .due
        .clone()
        .expect("Orbit checks it stays fixed");
    assert!(due > p.closed_at.clone().unwrap());

    // Three days on, it still passes.
    e.update(|s| {
        let ago = crate::problems::stamp(chrono::Utc::now() - chrono::Duration::minutes(1));
        for o in &mut s.outcomes {
            o.due = Some(ago.clone());
        }
    })
    .await;
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let held = &s.problems[0].playbook[5];
    assert_eq!(
        (held.mark, held.note.as_str(), held.due.as_deref()),
        (Done, "It stayed fixed for 3 days.", None)
    );
    assert_eq!(held.at, s.outcomes[0].settled_at);
    // A closed problem's issue no longer says what done looks like.
    let body = github(d.path())["issues"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!body.contains("What done looks like"), "{body}");
}

/// Adds to the CI `answer` gave when `main`'s latest commit was made and
/// when pull request 12's checks began.
fn timed(dir: &Path, made: &str, began: &str) {
    let path = dir.join("answer.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let r = &mut v["data"]["repository"];
    r["defaultBranchRef"]["target"]["committedDate"] = json!(made);
    for pull in r["pullRequests"]["nodes"].as_array_mut().unwrap() {
        let checks = &mut pull["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"];
        for check in checks["nodes"].as_array_mut().unwrap() {
            check["checkSuite"]["createdAt"] = json!(began);
        }
    }
    std::fs::write(path, v.to_string()).unwrap();
}

/// A pull request whose checks passed before `main` moved on is asked, on
/// Orbit's one comment, to merge it in, until its checks run again with
/// it.
#[cfg(unix)]
#[tokio::test]
async fn a_pull_request_green_before_main_moved_on_is_asked_to_merge_it_in() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let green: &[(&str, &str)] = &[("Rust", "SUCCESS")];
    answer(d.path(), '1', green, Some(('5', green)));
    timed(d.path(), "2026-10-30T10:00:00Z", "2026-10-30T10:05:00Z");
    watch(&e, &gh).await;
    assert!(github(d.path())["comments"].as_array().unwrap().is_empty());

    // main moves on after the checks began.
    answer(d.path(), '2', green, Some(('5', green)));
    timed(d.path(), "2026-10-30T10:10:00Z", "2026-10-30T10:05:00Z");
    watch(&e, &gh).await;
    let comments = github(d.path())["comments"].clone();
    assert_eq!(comments.as_array().unwrap().len(), 1);
    assert_eq!(comments[0]["issue"], 12);
    let body = comments[0]["body"].as_str().unwrap();
    assert!(body.contains(
        "Every check passes at `5555555`, but `main` has moved on since they began. Merge `main` in and let CI run again"
    ));
    assert!(body.contains(
        "- **I1** · `main` moved on since the checks at `5555555` began · merge it in and let CI run again"
    ));
    let s = e.snapshot().await;
    assert_eq!(s.problems[0].kind, crate::problems::ProblemKind::BaseMoved);
    assert_eq!(
        s.buddy_feed.notes.last().unwrap().text,
        "Orbit told the agent on a/b #12 that main moved on since its checks began. 📮"
    );
    assert!(
        e.shown_snapshot().await.attention.is_empty(),
        "the agent's to do"
    );
    // Nothing new while it stays the same.
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 1);
    // The agent answers it like any item.
    say(
        d.path(),
        "agent",
        "Need a person on I1: may I merge main in now?",
    );
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    assert_eq!(
        shown.attention[0].reason,
        "The agent on a/b #12 needs a person for I1 (main moved on): may I merge main in now?"
    );
    assert_eq!(writes(d.path()).len(), 2);

    // The agent merges main in and CI runs again with it.
    e.update(|s| s.prs = vec![pr('6')]).await;
    answer(d.path(), '2', green, Some(('6', green)));
    timed(d.path(), "2026-10-30T10:10:00Z", "2026-10-30T10:15:00Z");
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(
        g["comments"].as_array().unwrap().len(),
        2,
        "Orbit's and the reply"
    );
    let body = g["comments"][0]["body"].as_str().unwrap();
    assert!(body.contains("Every check passes at `6666666`. Nothing is left to fix here."));
    assert!(body.contains("- **I1** · `main` moved on · done at `6666666`"));
    let s = e.snapshot().await;
    assert!(s.problems[0].passed());
    assert!(e.shown_snapshot().await.attention.is_empty());
    assert_eq!(writes(d.path()).len(), 3);
}

/// When the default branch is renamed, GitHub moves open pull requests
/// onto the new name, and Orbit's comment asks to merge that one in.
#[cfg(unix)]
#[tokio::test]
async fn a_renamed_base_is_named_as_it_is_now() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let green: &[(&str, &str)] = &[("Rust", "SUCCESS")];
    answer(d.path(), '2', green, Some(('5', green)));
    timed(d.path(), "2026-10-30T10:10:00Z", "2026-10-30T10:05:00Z");
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"].to_string();
    assert!(body.contains("- **I1** · `main` moved on"), "{body}");

    // main is renamed trunk; nothing else changes.
    e.update(|s| s.prs[0].base_ref = "trunk".into()).await;
    let path = d.path().join("answer.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["data"]["repository"]["defaultBranchRef"]["name"] = json!("trunk");
    std::fs::write(&path, v.to_string()).unwrap();
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["comments"].as_array().unwrap().len(), 1);
    let body = g["comments"][0]["body"].as_str().unwrap();
    assert!(body.contains(
        "Every check passes at `5555555`, but `trunk` has moved on since they began. Merge `trunk` in and let CI run again"
    ));
    assert!(body.contains(
        "- **I1** · `trunk` moved on since the checks at `5555555` began · merge it in and let CI run again"
    ));
    assert!(!body.contains("`main`"), "{body}");
    assert!(body.contains(r#""base":"trunk""#), "{body}");
    let s = e.snapshot().await;
    let open: Vec<&str> = s
        .problems
        .iter()
        .filter(|p| p.is_open())
        .map(|p| p.base.as_str())
        .collect();
    assert_eq!(open, ["trunk"]);
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

/// Who signed the comment on pull request 12.
pub(super) fn signed_by(dir: &Path) -> String {
    let body = github(dir)["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    crate::orbits::signer(&body).unwrap().to_string()
}
/// Two Orbits follow one pull request, such as the desktop app and an
/// always-on Orbit both running: the first to post keeps its comment up to
/// date; the other steps back before it writes and says so once. Once you
/// tap Post here again it takes over, and the first steps back.
#[cfg(unix)]
#[tokio::test]
async fn only_one_orbit_posts_on_a_pull_request() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let first = connected(&d.path().join("first")).await;
    let second = connected(&d.path().join("second")).await;
    for e in [&first, &second] {
        e.update(|s| s.prs = vec![pr('5')]).await;
    }
    let failing = |check: &str| {
        answer(
            d.path(),
            '1',
            &[("Rust", "SUCCESS")],
            Some(('5', &[(check, "FAILURE")])),
        )
    };
    let named = |s: &Snapshot| s.instance.name.clone();
    let stepped_back = |s: &Snapshot| {
        s.attention
            .iter()
            .find(|a| a.kind == AttentionKind::AnotherOrbit)
            .cloned()
    };
    failing("Rust");
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/12/comments"]);
    let first_name = named(&first.snapshot().await);
    assert_eq!(signed_by(d.path()), first_name);

    // The second finds that comment, and steps back rather than edit it.
    watch(&second, &gh).await;
    assert_eq!(writes(d.path()).len(), 1);
    let shown = second.shown_snapshot().await;
    let item = stepped_back(&shown).expect("a Needs you item");
    assert_eq!(item.pr_id.as_deref(), Some("a/b#12"));
    assert_eq!(item.fix, Some(crate::buddy::Fix::PostHereAgain));
    assert_eq!(item.reason, crate::attention::ANOTHER_ORBIT);
    assert!(stepped_back(&first.shown_snapshot().await).is_none());
    // Nor anything later, nor does it keep asking GitHub.
    failing("Lint");
    let asked = calls(d.path(), "GET repos/a/b/pulls/12/reviews");
    watch(&second, &gh).await;
    assert_eq!(writes(d.path()).len(), 1);
    assert_eq!(calls(d.path(), "GET repos/a/b/pulls/12/reviews"), asked);
    assert!(!second
        .snapshot()
        .await
        .journal
        .iter()
        .any(|w| w.thread == "pr:a/b#12"));

    // The first carries on.
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    assert!(writes(d.path())[1].starts_with("PATCH repos/a/b/issues/comments/"));
    assert_eq!(signed_by(d.path()), first_name);

    // You say the second posts here: it does at once, and the first steps
    // back from then on.
    crate::github::GH
        .scope(gh.clone(), second.fix(&item.key))
        .await
        .unwrap();
    assert_eq!(writes(d.path()).len(), 3);
    assert_eq!(signed_by(d.path()), named(&second.snapshot().await));
    assert!(stepped_back(&second.shown_snapshot().await).is_none());
    failing("Rust");
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()).len(), 3);
    let shown = first.shown_snapshot().await;
    assert!(stepped_back(&shown).is_some());
    watch(&second, &gh).await;
    assert_eq!(writes(d.path()).len(), 4);
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("- **I1** · `CI / Rust` · failing at `5555555`"));
    assert_eq!(github(d.path())["comments"].as_array().unwrap().len(), 1);
}

/// A comment on pull request 12 of the fake GitHub, signed `by`, at `at`,
/// by the same account as this Orbit and before anything the fake GitHub
/// makes itself.
fn commented_by(dir: &Path, by: &str, at: &str) {
    let comment = json!({
        "id": 50, "issue": 12, "user": { "login": "me" },
        "body": crate::orbits::signed("Every check passes.", by),
        "html_url": "https://github.com/a/b/pull/12#issuecomment-50",
        "created_at": at, "updated_at": at, "node_id": "IC_50",
    });
    let db = json!({ "next": 100, "issues": [], "comments": [comment] });
    std::fs::write(dir.join("github.json"), db.to_string()).unwrap();
}
/// A review of pull request 12 on the fake GitHub, signed `by`, at `at`.
fn reviewed_by(dir: &Path, by: &str, at: &str) {
    let review = json!([{
        "id": 7, "user": { "login": "me" },
        "body": crate::orbits::signed("<!-- orbit-review:r1 -->\nNo findings.", by),
        "html_url": "https://github.com/a/b/pull/12#pullrequestreview-7",
        "submitted_at": at,
    }]);
    std::fs::write(dir.join("reviews-12.json"), review.to_string()).unwrap();
}
/// Orbit reads the comments and the reviews before it decides, as a newer
/// post in either outweighs an older one in the other: of two Orbits, the
/// one whose review came after the other's comment carries on, and the
/// other steps back, whichever came last.
#[cfg(unix)]
#[tokio::test]
async fn comments_and_reviews_are_both_read_before_deciding() {
    for review_last in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let gh = fake_gh(d.path());
        let a = connected(&d.path().join("a")).await;
        let b = connected(&d.path().join("b")).await;
        let (na, nb) = (
            a.snapshot().await.instance.name,
            b.snapshot().await.instance.name,
        );
        // B's comment about CI, and A's review a second later, or before.
        let (comment, review) = match review_last {
            true => ("2026-10-31T10:00:01Z", "2026-10-31T10:00:02Z"),
            false => ("2026-10-31T10:00:02Z", "2026-10-31T10:00:01Z"),
        };
        commented_by(d.path(), &nb, comment);
        reviewed_by(d.path(), &na, review);
        let check = |e: &Arc<Engine>| {
            crate::github::GH.scope(gh.clone(), {
                let e = e.clone();
                async move { e.clear_to_post("a/b", 12, "a/b#12").await }
            })
        };
        assert_eq!(check(&a).await, Ok(review_last), "{review_last}");
        assert_eq!(check(&b).await, Ok(!review_last), "{review_last}");
        // Each kept what it read, so they agree from then on.
        for (e, clear) in [(&a, review_last), (&b, !review_last)] {
            let s = e.snapshot().await;
            assert_eq!(
                crate::orbits::stepped_back(&s.posting, "a/b#12"),
                !clear,
                "{review_last}"
            );
        }
        // With the reviews unreadable, what the comments show says whether
        // Orbit is behind, and nothing is kept.
        let c = connected(&d.path().join("c")).await;
        std::fs::write(
            d.path().join("reviews-error"),
            "gh: Bad Gateway (HTTP 502)\n",
        )
        .unwrap();
        let unsure = check(&c).await.unwrap_err();
        assert!(unsure.behind);
        assert!(unsure.error.contains("Bad Gateway"));
        assert!(c.snapshot().await.posting.is_empty());
    }
}

/// One tap on Post here again takes over, even where the other Orbit's
/// newest post is a review that came after its comment.
#[cfg(unix)]
#[tokio::test]
async fn one_tap_takes_over_after_the_other_orbits_comment_and_review() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let other = "f".repeat(32);
    commented_by(d.path(), &other, "2026-10-29T11:00:00Z");
    reviewed_by(d.path(), &other, "2026-10-29T11:00:05Z");
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Rust", "FAILURE")])),
    );
    watch(&e, &gh).await;
    assert!(writes(d.path()).is_empty());
    let item = e
        .shown_snapshot()
        .await
        .attention
        .into_iter()
        .find(|a| a.kind == AttentionKind::AnotherOrbit)
        .expect("stepped back");
    crate::github::GH
        .scope(gh.clone(), e.fix(&item.key))
        .await
        .unwrap();
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/12/comments"]);
    let s = e.snapshot().await;
    assert_eq!(
        s.posting[0].resumed.as_ref().map(|p| p.at.as_str()),
        Some("2026-10-29T11:00:05Z")
    );
    assert!(!crate::orbits::stepped_back(&s.posting, "a/b#12"));
    assert!(!e
        .shown_snapshot()
        .await
        .attention
        .iter()
        .any(|a| a.kind == AttentionKind::AnotherOrbit));
}

/// Two Orbits keep one comment on a pull request, each taking it over in
/// turn. While GitHub can't list the comments, only the Orbit that wrote it
/// last keeps it up to date; the other, which hasn't seen that yet, never
/// writes over it.
#[cfg(unix)]
#[tokio::test]
async fn while_comments_cant_be_read_only_the_orbit_that_wrote_last_edits() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let first = connected(&d.path().join("first")).await;
    let second = connected(&d.path().join("second")).await;
    for e in [&first, &second] {
        e.update(|s| s.prs = vec![pr('5')]).await;
    }
    let failing = |check: &str| {
        answer(
            d.path(),
            '1',
            &[("Rust", "SUCCESS")],
            Some(('5', &[(check, "FAILURE")])),
        )
    };
    failing("Rust");
    watch(&first, &gh).await;
    watch(&second, &gh).await;
    let key = second
        .shown_snapshot()
        .await
        .attention
        .into_iter()
        .find(|a| a.kind == AttentionKind::AnotherOrbit)
        .expect("the second stepped back")
        .key;
    failing("Lint");
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    // You hand the pull request to the second, which edits the comment.
    crate::github::GH
        .scope(gh.clone(), second.fix(&key))
        .await
        .unwrap();
    assert_eq!(writes(d.path()).len(), 3);
    let second_name = second.snapshot().await.instance.name;
    assert_eq!(signed_by(d.path()), second_name);

    // GitHub stops listing comments before the first has seen that.
    std::fs::write(
        d.path().join("comments-error"),
        "gh: Bad Gateway (HTTP 502)\n",
    )
    .unwrap();
    failing("Rust");
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()).len(), 3, "the first leaves it alone");
    assert_eq!(signed_by(d.path()), second_name);
    failing("Lint");
    watch(&second, &gh).await;
    assert_eq!(writes(d.path()).len(), 4, "the second keeps it true");
    assert_eq!(signed_by(d.path()), second_name);
    assert!(github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .contains("`CI / Lint` · failing at `5555555`"));

    // Once GitHub lists them again, the first sees it stepped back.
    std::fs::remove_file(d.path().join("comments-error")).unwrap();
    failing("Rust");
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()).len(), 4);
    assert!(first
        .shown_snapshot()
        .await
        .attention
        .iter()
        .any(|a| a.kind == AttentionKind::AnotherOrbit));
    assert_eq!(github(d.path())["comments"].as_array().unwrap().len(), 1);
}

/// While GitHub can't say who posted last, nothing new is posted on a pull
/// request, but Orbit's own comment is still kept true.
#[cfg(unix)]
#[tokio::test]
async fn nothing_new_is_posted_while_github_cant_say_who_posted_last() {
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
    std::fs::write(
        d.path().join("reviews-error"),
        "gh: Bad Gateway (HTTP 502)\n",
    )
    .unwrap();
    watch(&e, &gh).await;
    assert!(writes(d.path()).is_empty());
    std::fs::remove_file(d.path().join("reviews-error")).unwrap();
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/12/comments"]);
    std::fs::write(
        d.path().join("reviews-error"),
        "gh: Bad Gateway (HTTP 502)\n",
    )
    .unwrap();
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Rust", "SUCCESS")])),
    );
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    assert!(github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .contains("Every check passes at `5555555`."));
    assert!(e.shown_snapshot().await.attention.is_empty());

    // Once the comments show another Orbit posted after it, even its own
    // comment waits, though nothing is decided until the reviews are read.
    let mut db = github(d.path());
    let at = "2026-10-30T00:00:00Z";
    db["comments"].as_array_mut().unwrap().push(json!({
        "id": 60, "issue": 12, "user": { "login": "me" },
        "body": crate::orbits::signed("Every check passes.", &"f".repeat(32)),
        "html_url": "https://github.com/a/b/pull/12#issuecomment-60",
        "created_at": at, "updated_at": at, "node_id": "IC_60",
    }));
    std::fs::write(d.path().join("github.json"), db.to_string()).unwrap();
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Lint", "FAILURE")])),
    );
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    assert!(e.shown_snapshot().await.attention.is_empty());
    std::fs::remove_file(d.path().join("reviews-error")).unwrap();
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    assert!(e
        .shown_snapshot()
        .await
        .attention
        .iter()
        .any(|a| a.kind == AttentionKind::AnotherOrbit));
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
    // Not a GitHub Actions job, so the first thing Orbit asks for is the
    // search.
    links(d.path(), &[("Rust", "https://ci.example.com/build/1")]);
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
    // Nor does reading who posted last, for whether another Orbit did.
    {
        let d = tempfile::tempdir().unwrap();
        let gh = fake_gh(d.path());
        let seed = |total: u64| {
            let items: Vec<Value> = (1..=total)
                .map(|n| json!({ "id": n, "issue": 12, "body": "x", "created_at": made(n) }))
                .collect();
            let db = json!({ "next": 1000, "issues": [], "comments": items });
            std::fs::write(d.path().join("github.json"), db.to_string()).unwrap();
        };
        let read = || {
            let endpoint = "repos/a/b/issues/12/comments";
            let all = crate::github::all_or_nothing(endpoint, &[], 2, "Invalid comment list");
            crate::github::GH.scope(gh.clone(), all)
        };
        seed(200);
        assert!(read()
            .await
            .unwrap_err()
            .starts_with("There are too many posts"));
        seed(199);
        assert_eq!(read().await.unwrap().len(), 199);
    }
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
                    "node_id": format!("N_{n}"),
                    "issue": 12,
                    "user": { "login": "me" },
                    "body": body,
                    "created_at": made(n),
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

/// When the fake GitHub made comment or issue `id`: a second apart.
fn made(id: u64) -> String {
    let at = chrono::Utc
        .with_ymd_and_hms(2026, 10, 29, 12, 0, 0)
        .unwrap()
        + chrono::Duration::seconds(id as i64);
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
/// A reply on GitHub from `who` on pull request 12.
fn say(dir: &Path, who: &str, body: &str) {
    chat(dir, who, body, 1);
}
/// `n` replies on GitHub from `who` on pull request 12, a second apart.
fn chat(dir: &Path, who: &str, body: &str, n: u64) {
    let mut db = github(dir);
    let first = db["next"].as_u64().unwrap_or(100);
    db["next"] = json!(first + n);
    for id in first..first + n {
        db["comments"].as_array_mut().unwrap().push(json!({
            "id": id,
            "issue": 12,
            "user": { "login": who },
            "body": body,
            "created_at": made(id),
            "updated_at": made(id),
            "html_url": format!("https://github.com/a/b/issues/12#issuecomment-{id}"),
        }));
    }
    std::fs::write(dir.join("github.json"), db.to_string()).unwrap();
}
/// Edits the comments `which` picks, in turn, a second apart, as GitHub
/// would: when each was posted stays the same.
fn edit(dir: &Path, which: impl Fn(&Value) -> bool) {
    let mut db = github(dir);
    let mut next = db["next"].as_u64().unwrap();
    for c in db["comments"].as_array_mut().unwrap() {
        if which(c) {
            c["updated_at"] = json!(made(next));
            next += 1;
        }
    }
    db["next"] = json!(next);
    std::fs::write(dir.join("github.json"), db.to_string()).unwrap();
}
/// GitHub's checks at one commit, for checking a fix there.
fn checks_at(dir: &Path, at: char, checks: &[(&str, &str)]) {
    let v = json!({ "data": { "repository": { "object": {
        "statusCheckRollup": rollup(checks)
    } } } });
    std::fs::write(dir.join(format!("commit-{}.json", sha(at))), v.to_string()).unwrap();
}
/// Where commit `at`, named by its first seven characters, stands against
/// `head`.
fn compared(dir: &Path, at: char, head: char, status: &str) {
    let file = dir.join("compare.json");
    let mut v: Value = std::fs::read_to_string(&file)
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or(json!({}));
    let key = format!("{}...{}", &sha(at)[..7], sha(head));
    v[key] = json!({ "status": status, "base_commit": { "sha": sha(at) } });
    std::fs::write(file, v.to_string()).unwrap();
}
fn calls(dir: &Path, what: &str) -> usize {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(what))
        .count()
}

/// The agent answers on Orbit's comment: a fix that CI doesn't bear out
/// is said so on the comment and in a note from Bit, a question for a
/// person is a Needs you item until the agent takes it back, and replies
/// from accounts Orbit doesn't trust are left out.
#[cfg(unix)]
#[tokio::test]
async fn the_agent_answers_on_orbits_comment() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let both = [("Rust", "FAILURE"), ("Lint", "FAILURE")];
    answer(d.path(), '1', &[("Rust", "SUCCESS")], Some(('5', &both)));
    // From before Orbit's comment, so not about its items.
    say(d.path(), "agent", "Declined I1: an older reply");
    watch(&e, &gh).await;
    let notes = e.snapshot().await.buddy_feed.notes;
    assert_eq!(
        notes.last().unwrap().text,
        "Orbit told the agent on a/b #12 that CI / Rust and CI / Lint fail. 📮"
    );
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][1]["body"].to_string();
    assert!(!body.contains("declined"), "{body}");

    say(
        d.path(),
        "agent",
        "Thanks!\n- Fixed I2 in 7777777\n- Need a person on I1: the runner is out of disk.",
    );
    say(d.path(), "stranger", "Declined I1: not mine");
    // The agent pushed 7777777, where Lint still fails.
    e.update(|s| s.prs = vec![pr('7')]).await;
    answer(d.path(), '1', &[("Rust", "SUCCESS")], Some(('7', &both)));
    compared(d.path(), '7', '7', "identical");
    checks_at(d.path(), '7', &both);
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][1]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains(
        "- **I2** · `CI / Lint` · failing since `5555555`, still at `7777777` · [details](https://github.com/a/b/actions/runs/1/job/2) · said to be fixed in `7777777`, but it still fails there ([reply](https://github.com/a/b/issues/12#issuecomment-"
    ), "{body}");
    assert!(body.contains("· waiting for a person ([reply]("), "{body}");
    assert!(!body.contains("declined"), "{body}");
    assert!(
        !body.contains("out of disk"),
        "an agent's words aren't posted"
    );
    assert!(body.contains(r#""answer":{"check":"fails","commit":"7777777","kind":"fixed"}"#));
    let shown = e.shown_snapshot().await;
    let asks: Vec<&Attention> = shown
        .attention
        .iter()
        .filter(|a| a.kind == AttentionKind::CiQuestion)
        .collect();
    assert_eq!(asks.len(), 1);
    assert_eq!(
        asks[0].reason,
        "The agent on a/b #12 needs a person for I1 (CI / Rust): the runner is out of disk."
    );
    let notes = e.snapshot().await.buddy_feed.notes;
    assert!(notes.iter().any(|n| n.text
        == "The agent on a/b #12 said I2 (CI / Lint) is fixed in 7777777, but it still fails there. Worth a look."));
    assert_eq!(calls(d.path(), "compare/"), 1);

    // The agent takes I1 itself: nobody is needed any more, and a settled
    // fix isn't checked again.
    say(d.path(), "agent", "Taking I1");
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][1]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("· someone is on it ([reply]("), "{body}");
    let asks = |s: Snapshot| {
        s.attention
            .iter()
            .filter(|a| a.kind == AttentionKind::CiQuestion)
            .count()
    };
    assert_eq!(asks(e.shown_snapshot().await), 0);
    assert_eq!(calls(d.path(), "compare/"), 1);

    // A question about a check that then passes needs nobody.
    say(
        d.path(),
        "agent",
        "Need a person on I2: who owns the linter?",
    );
    watch(&e, &gh).await;
    assert_eq!(asks(e.shown_snapshot().await), 1);
    e.update(|s| s.prs = vec![pr('8')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('8', &[("Rust", "FAILURE"), ("Lint", "SUCCESS")])),
    );
    watch(&e, &gh).await;
    assert_eq!(asks(e.shown_snapshot().await), 0);
}

/// A fix in a commit newer than the one Orbit read waits until Orbit reads
/// a commit it leads to; then CI there decides. A commit that isn't on the
/// branch, or that GitHub doesn't know, is said so.
#[cfg(unix)]
#[tokio::test]
async fn a_fix_is_checked_against_ci_where_it_was_made() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let lint = |r| [("Rust", "SUCCESS"), ("Lint", r)];
    answer(d.path(), '1', &[], Some(('5', &lint("FAILURE"))));
    watch(&e, &gh).await;
    let body = || {
        github(d.path())["comments"][0]["body"]
            .as_str()
            .unwrap()
            .to_string()
    };
    say(d.path(), "agent", "Fixed I1 in 6666666");
    compared(d.path(), '6', '5', "behind");
    watch(&e, &gh).await;
    assert!(
        body().contains("said to be fixed in `6666666`; Orbit is waiting for its checks there"),
        "{}",
        body()
    );

    // The agent pushed 6666666, where Lint passed, then 7777777, where it
    // fails again.
    e.update(|s| s.prs = vec![pr('7')]).await;
    answer(d.path(), '1', &[], Some(('7', &lint("FAILURE"))));
    compared(d.path(), '6', '7', "ahead");
    checks_at(d.path(), '6', &lint("SUCCESS"));
    let notes = e.snapshot().await.buddy_feed.notes.len();
    watch(&e, &gh).await;
    assert!(
        body().contains("said to be fixed in `6666666`, and it passes there"),
        "{}",
        body()
    );
    assert_eq!(
        e.snapshot().await.buddy_feed.notes.len(),
        notes,
        "a fix that holds needs no note"
    );

    say(d.path(), "agent", "Fixed I1 in 8888888");
    compared(d.path(), '8', '7', "diverged");
    watch(&e, &gh).await;
    assert!(
        body().contains("said to be fixed in `8888888`, which isn't on `fix-it`"),
        "{}",
        body()
    );
    say(d.path(), "agent", "Fixed I1 in abcdef0");
    watch(&e, &gh).await;
    assert!(
        body().contains("said to be fixed in `abcdef0`, which Orbit can't find"),
        "{}",
        body()
    );
    let notes = e.snapshot().await.buddy_feed.notes;
    let texts: Vec<&str> = notes.iter().map(|n| n.text.as_str()).collect();
    assert!(texts.contains(
        &"The agent on a/b #12 said I1 (CI / Lint) is fixed in 8888888, but that commit isn't on fix-it. Worth a look."
    ), "{texts:?}");
    assert!(texts.contains(
        &"The agent on a/b #12 said I1 (CI / Lint) is fixed in abcdef0, but Orbit can't find that commit. Worth a look."
    ), "{texts:?}");
}

/// Orbit reads replies from its own comment on, however many comments came
/// before it.
#[cfg(unix)]
#[tokio::test]
async fn replies_are_read_after_orbits_comment_on_a_busy_pull_request() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    chat(d.path(), "someone", "Looks good.", 1050);
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    answer(d.path(), '1', &[], Some(('5', &[("Lint", "FAILURE")])));
    watch(&e, &gh).await;
    say(d.path(), "agent", "Taking I1");
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][1050]["body"].to_string();
    assert!(body.contains("someone is on it"), "{body}");
}

/// GitHub lists the comments edited since Orbit last read by when they
/// were posted, so a long run of old ones edited later comes before a new
/// reply. Orbit goes on through the list a page at a time until it reaches
/// the end, never past a reply it hasn't read, and reads the last page
/// again in case a comment was deleted meanwhile.
#[cfg(unix)]
#[tokio::test]
async fn edited_old_comments_dont_hide_new_replies() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    chat(d.path(), "someone", "Looks good.", 1050);
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    answer(d.path(), '1', &[], Some(('5', &[("Lint", "FAILURE")])));
    watch(&e, &gh).await;
    let id = |c: &Value| c["id"].as_u64().unwrap();
    let first = id(&github(d.path())["comments"][0]);
    // 999 old comments are edited in turn, and the agent answers halfway.
    // After them and Orbit's comment, the reply is 1,001st on the list.
    edit(d.path(), |c| id(c) < first + 600);
    say(d.path(), "agent", "Taking I1");
    edit(d.path(), |c| (first + 600..first + 999).contains(&id(c)));
    let body = || {
        let db = github(d.path());
        let mine = db["comments"].as_array().unwrap().iter();
        mine.filter(|c| c["user"]["login"] == "me")
            .map(|c| c["body"].to_string())
            .collect::<String>()
    };
    let read = |e: &Arc<Engine>| {
        let e = e.clone();
        async move {
            let s = e.snapshot().await;
            (s.ops[0].read_page, s.ops[0].read_since.clone())
        }
    };
    let since = read(&e).await.1;
    watch(&e, &gh).await;
    assert!(!body().contains("someone is on it"), "{}", body());
    assert_eq!(read(&e).await, (10, since), "part way through the list");
    // An old comment is deleted, so the reply moves back onto page 10.
    let mut db = github(d.path());
    db["comments"].as_array_mut().unwrap().remove(0);
    std::fs::write(d.path().join("github.json"), db.to_string()).unwrap();
    watch(&e, &gh).await;
    assert!(body().contains("someone is on it"), "{}", body());
    let last = github(d.path())["comments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| id(c) == first + 998)
        .unwrap()["updated_at"]
        .clone();
    assert_eq!(read(&e).await, (0, last.as_str().unwrap().to_string()));
}

/// A check's runs at a commit count only once every page of the commit's
/// checks was read, but a failure counts at once.
#[cfg(unix)]
#[tokio::test]
async fn a_check_is_not_passing_until_every_page_is_read() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let at = |checks: &[(&str, &str)], more: bool| {
        let mut rollup = rollup(checks);
        rollup["contexts"]["pageInfo"] = json!({ "hasNextPage": more, "endCursor": "next" });
        let v = json!({ "data": { "repository": { "object": { "statusCheckRollup": rollup } } } });
        std::fs::write(
            d.path().join(format!("commit-{}.json", sha('6'))),
            v.to_string(),
        )
        .unwrap();
    };
    let six = sha('6');
    let check = || {
        crate::github::GH.scope(
            gh.clone(),
            crate::ci::check_at_within("a/b", &six, "CI / Lint", 2),
        )
    };
    use crate::ci::CiState;
    at(&[("Lint", "SUCCESS")], true);
    assert_eq!(check().await.unwrap(), CiState::Running);
    at(&[("Lint", "FAILURE")], true);
    assert_eq!(check().await.unwrap(), CiState::Failing);
    at(&[("Lint", "SUCCESS")], false);
    assert_eq!(check().await.unwrap(), CiState::Passing);
    at(&[("Rust", "SUCCESS")], false);
    assert_eq!(check().await.unwrap(), CiState::None);
}

/// A check that fails again hears only replies from when Orbit's comment
/// shows it failing again. An answer left while the pull request was green
/// is about the failure before, even behind a long talk, and even edited
/// since, so that it comes on the same page as an answer about this one.
#[cfg(unix)]
#[tokio::test]
async fn a_reply_from_a_green_spell_doesnt_carry_over() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    let ci = |head: char, result: &str| {
        let checks = [("Rust", result), ("Lint", result)];
        answer(d.path(), '1', &[], Some((head, &checks)));
    };
    e.update(|s| s.prs = vec![pr('5')]).await;
    ci('5', "FAILURE");
    watch(&e, &gh).await;
    e.update(|s| s.prs = vec![pr('6')]).await;
    ci('6', "SUCCESS");
    watch(&e, &gh).await;
    chat(d.path(), "someone", "Looks good.", 1050);
    say(d.path(), "agent", "Declined I1: the runner was down");
    e.update(|s| s.prs = vec![pr('7')]).await;
    ci('7', "FAILURE");
    watch(&e, &gh).await;
    let body = || github(d.path())["comments"][0]["body"].to_string();
    assert!(body().contains("failing at `7777777`"), "{}", body());
    // Everything said while it was green is edited, then the agent takes
    // I2.
    edit(d.path(), |c| c["user"]["login"] != "me");
    say(d.path(), "agent", "Taking I2");
    watch(&e, &gh).await;
    watch(&e, &gh).await;
    assert!(body().contains("someone is on it"), "{}", body());
    assert!(!body().contains("declined"), "{}", body());
}

/// A comment that reached GitHub without Orbit hearing back is found
/// again. Its items have been on GitHub since it was posted, not since the
/// edit that brings it up to date, so an answer left before Orbit found it
/// counts, and one from before the comment doesn't.
#[cfg(unix)]
#[tokio::test]
async fn a_post_orbit_didnt_hear_back_about_keeps_its_time() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    say(d.path(), "agent", "Declined I1: from before the comment");
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    answer(d.path(), '1', &[], Some(('5', &[("Lint", "FAILURE")])));
    watch(&e, &gh).await;
    cut_off(&e).await;
    say(d.path(), "agent", "Taking I1");
    watch(&e, &gh).await;
    assert!(
        e.snapshot().await.ops[0].comment_id.is_some(),
        "found again"
    );
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][1]["body"].to_string();
    assert!(body.contains("someone is on it"), "{body}");
    assert!(!body.contains("declined"), "{body}");
}

/// Rewrites Orbit's saved state as a version that didn't read answers
/// saved it.
fn saved_before_answers(dir: &Path) {
    let file = dir.join("state").join("state.json");
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    for t in state["ops"].as_array_mut().unwrap() {
        let t = t.as_object_mut().unwrap();
        for key in ["readTo", "readSince", "readPage", "readNewest", "shownAt"] {
            assert!(t.remove(key).is_some(), "{key}");
        }
        for item in t["items"].as_array_mut().unwrap() {
            let item = item.as_object_mut().unwrap();
            assert!(item.remove("heardFrom").is_some());
            item.remove("answer");
        }
    }
    std::fs::write(&file, state.to_string()).unwrap();
}
/// State saved by a version of Orbit that didn't read answers has no time
/// for the items on its comments and issues. Orbit takes it from its copy
/// on GitHub before reading, so an answer after the comment counts though
/// the comment doesn't change, and one from before it doesn't.
#[cfg(unix)]
#[tokio::test]
async fn comments_from_before_orbit_read_answers_hear_them() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    say(d.path(), "agent", "Declined I1: from before the comment");
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let lint = [("Lint", "FAILURE")];
    answer(d.path(), '1', &[("Rust", "FAILURE")], Some(('5', &lint)));
    watch(&e, &gh).await;
    drop(e);
    saved_before_answers(d.path());
    say(d.path(), "agent", "Taking I1");
    let e = connected(d.path()).await;
    let before = writes(d.path()).len();
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][1]["body"].to_string();
    assert!(body.contains("someone is on it"), "{body}");
    assert!(!body.contains("declined"), "{body}");
    assert_eq!(writes(d.path()).len(), before + 1, "only that edit");
    let issue = github(d.path())["issues"][0].clone();
    let s = e.snapshot().await;
    let t = s.ops.iter().find(|t| t.kind == ThreadKind::Issue).unwrap();
    assert_eq!(t.items[0].heard_from, issue["created_at"].as_str().unwrap());
}

/// A comment saved by a version of Orbit that didn't read answers may have
/// been posted for an earlier failure of the same check and edited since.
/// GitHub can't say when the failure now showed first, so answers count
/// from Orbit's last edit: one from the earlier failure doesn't carry over.
#[cfg(unix)]
#[tokio::test]
async fn an_old_answer_on_a_reused_comment_doesnt_carry_over() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    let ci = |head: char, rust: &str, lint: &str| {
        let checks = [("Rust", rust), ("Lint", lint)];
        answer(d.path(), '1', &[], Some((head, &checks)));
    };
    e.update(|s| s.prs = vec![pr('5')]).await;
    ci('5', "FAILURE", "SUCCESS");
    watch(&e, &gh).await;
    say(d.path(), "agent", "Declined I1: the runner was down");
    e.update(|s| s.prs = vec![pr('6')]).await;
    ci('6', "SUCCESS", "SUCCESS");
    watch(&e, &gh).await;
    e.update(|s| s.prs = vec![pr('7')]).await;
    ci('7', "FAILURE", "FAILURE");
    watch(&e, &gh).await;
    let body = || github(d.path())["comments"][0]["body"].to_string();
    assert!(
        body().contains("- **I2** · `CI / Lint` · failing at `7777777`"),
        "{}",
        body()
    );
    say(d.path(), "agent", "Taking I2");
    drop(e);
    saved_before_answers(d.path());
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    assert!(body().contains("someone is on it"), "{}", body());
    assert!(!body().contains("declined"), "{}", body());
}

/// Gives the checks GitHub reports these links, by name.
fn links(dir: &Path, links: &[(&str, &str)]) {
    let file = dir.join("answer.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    fn walk(v: &mut Value, links: &[(&str, &str)]) {
        match v {
            Value::Object(o) => {
                if let Some((_, url)) = links.iter().find(|(n, _)| o.get("name") == Some(&json!(n)))
                {
                    o.insert("detailsUrl".into(), json!(url));
                }
                o.values_mut().for_each(|v| walk(v, links));
            }
            Value::Array(a) => a.iter_mut().for_each(|v| walk(v, links)),
            _ => {}
        }
    }
    walk(&mut v, links);
    std::fs::write(file, v.to_string()).unwrap();
}
/// A GitHub Actions job whose step `step` failed, and its log.
fn job(dir: &Path, id: u32, step: &str, log: &[&str]) {
    let (before, at) = ("2026-10-29T11:59:59Z", "2026-10-29T12:00:00Z");
    let steps = json!({ "steps": [
        { "name": "Set up job", "conclusion": "success", "started_at": before, "completed_at": before },
        { "name": step, "conclusion": "failure", "started_at": at, "completed_at": at }
    ] });
    std::fs::write(dir.join(format!("job-{id}.json")), steps.to_string()).unwrap();
    let log: Vec<String> = log
        .iter()
        .map(|l| format!("2026-10-29T12:00:00.0000000Z {l}"))
        .collect();
    std::fs::write(dir.join(format!("log-{id}.txt")), log.join("\n")).unwrap();
}
fn job_url(id: u32) -> String {
    format!("https://github.com/a/b/actions/runs/1/job/{id}")
}

/// Orbit reads why each failing job failed, once, and shows the step and
/// the end of its log on its comment. A check that isn't a GitHub Actions
/// job, or whose log GitHub no longer has, says so on Operations; a log
/// that looks like it holds a secret is held back; one GitHub didn't
/// answer for is read again after a wait.
#[cfg(unix)]
#[tokio::test]
async fn the_comment_says_why_each_job_failed() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let failing: &[(&str, &str)] = &[
        ("Rust", "FAILURE"),
        ("Lint", "FAILURE"),
        ("Docs", "FAILURE"),
    ];
    answer(d.path(), '1', &[("Rust", "SUCCESS")], Some(('5', failing)));
    let (rust, lint) = (job_url(2), job_url(3));
    links(
        d.path(),
        &[
            ("Rust", &rust),
            ("Lint", &lint),
            ("Docs", "https://ci.example.com/build/1"),
        ],
    );
    job(
        d.path(),
        2,
        "Run cargo test",
        &[
            "##[group]Run cargo test",
            "cargo test --locked",
            "##[endgroup]",
            "test it_works ... FAILED",
            "##[error]Process completed with exit code 101.",
        ],
    );
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains(&format!(
        "`CI / Rust` · failing at `5555555` · [details]({rust})\n  Failed at step `Run cargo test`:\n  ```text\n  test it_works ... FAILED\n  Error: Process completed with exit code 101.\n  ```\n"
    )));
    assert!(body.contains(&format!(
        "`CI / Lint` · failing at `5555555` · [details]({lint})\n"
    )));
    let read = e.state.lock().await.failures.clone();
    assert_eq!(read.len(), 3);
    let of = |url: &str| read.iter().find(|f| f.url == url).unwrap().clone();
    assert_eq!(of(&rust).step, "Run cargo test");
    assert_eq!(
        of(&lint).error.as_deref(),
        Some("GitHub no longer has this job's log.")
    );
    assert_eq!(
        of("https://ci.example.com/build/1").error.as_deref(),
        Some("Orbit can only read the logs of GitHub Actions jobs.")
    );
    assert_eq!(calls(d.path(), "actions/jobs/2"), 2);
    assert_eq!(calls(d.path(), "actions/jobs/3"), 1);

    // Each job is read once.
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "actions/jobs/"), 3);

    // A new head: one log holds a secret, and one GitHub answers badly.
    e.update(|s| s.prs = vec![pr('6')]).await;
    let failing: &[(&str, &str)] = &[("Rust", "FAILURE"), ("Lint", "FAILURE")];
    answer(d.path(), '1', &[("Rust", "SUCCESS")], Some(('6', failing)));
    let (rust, lint) = (job_url(4), job_url(5));
    links(d.path(), &[("Rust", &rust), ("Lint", &lint)]);
    let token = format!("gh{}_{}", 'p', "A1".repeat(18));
    job(
        d.path(),
        4,
        "Run cargo test",
        // Far enough along its line that only part of it would be shown.
        &[&format!("{} {token}", "x".repeat(220)), "##[error]boom"],
    );
    job(d.path(), 5, "Run npm test", &["##[error]lint failed"]);
    std::fs::write(d.path().join("job-5.json"), "not JSON").unwrap();
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!body.contains(&token[..12]));
    assert!(body.contains(&format!(
        "[details]({rust})\n  Orbit held back this job's log: it looks like it holds a secret.\n"
    )));
    assert!(body.contains(&format!("[details]({lint})\n")));
    let read = e.state.lock().await.failures.clone();
    let urls: Vec<&str> = read.iter().map(|f| f.url.as_str()).collect();
    assert_eq!(urls, [&rust, &lint], "what no longer fails is forgotten");
    assert!(read[0].held && read[0].lines.is_empty());
    assert!(read[1]
        .error
        .as_deref()
        .unwrap()
        .starts_with("Orbit couldn't read its log yet: Invalid GitHub response"));
    assert!(read[1].retry_at.is_some());
    let saved = std::fs::read_to_string(d.path().join("state").join("state.json")).unwrap();
    assert!(!saved.contains(&token[..12]));

    // Not until its wait is over.
    job(d.path(), 5, "Run npm test", &["##[error]lint failed"]);
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "actions/jobs/5"), 1);
    waited(&e).await;
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains(&format!(
        "[details]({lint})\n  Failed at step `Run npm test`:\n  ```text\n  Error: lint failed\n  ```\n"
    )));
    assert_eq!(calls(d.path(), "actions/jobs/5"), 3);
}
/// As if every log Orbit is waiting to read again is due.
async fn waited(e: &Engine) {
    e.update(|s| {
        for f in &mut s.failures {
            if f.retry_at.is_some() {
                f.retry_at = Some("2026-01-01T00:00:00.000Z".into());
            }
        }
    })
    .await;
}

/// Logs GitHub keeps failing on wait while the next jobs are read; after
/// a rate limit Orbit stops for that pass, and reads them all once it is
/// over.
#[cfg(unix)]
#[tokio::test]
async fn a_log_github_wont_give_doesnt_hold_up_the_others() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    let names = ["A", "B", "C", "D"];
    let failing: Vec<(&str, &str)> = names.iter().map(|n| (*n, "FAILURE")).collect();
    answer(d.path(), '1', &[("A", "SUCCESS")], Some(('5', &failing)));
    let urls: Vec<String> = (2..=5).map(job_url).collect();
    let named: Vec<(&str, &str)> = names
        .iter()
        .zip(&urls)
        .map(|(n, u)| (*n, u.as_str()))
        .collect();
    links(d.path(), &named);
    for id in 2..=5 {
        job(
            d.path(),
            id,
            &format!("Run step {id}"),
            &[&format!("##[error]failed {id}")],
        );
    }
    for id in 2..=4 {
        std::fs::write(d.path().join(format!("job-{id}.json")), "not JSON").unwrap();
    }
    watch(&e, &gh).await;
    for id in 2..=4 {
        assert_eq!(calls(d.path(), &format!("actions/jobs/{id}")), 1);
    }
    assert_eq!(calls(d.path(), "actions/jobs/5"), 0);
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "actions/jobs/5"), 2);
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("Failed at step `Run step 5`:"));
    assert_eq!(calls(d.path(), "actions/jobs/"), 5);

    // GitHub's rate limit: Orbit stops at the first, and waits.
    std::fs::write(d.path().join("limit"), "").unwrap();
    waited(&e).await;
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "actions/jobs/"), 6);
    let read = e.state.lock().await.failures.clone();
    let first = read.iter().find(|f| f.url == urls[0]).unwrap();
    assert_eq!(
        first.error.as_deref(),
        Some("Orbit couldn't read its log yet: API rate limit exceeded for user ID 1. (HTTP 403). It tries again later.")
    );
    assert_eq!(first.tries, 2);
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "actions/jobs/"), 6);

    std::fs::remove_file(d.path().join("limit")).unwrap();
    for id in 2..=4 {
        job(
            d.path(),
            id,
            &format!("Run step {id}"),
            &[&format!("##[error]failed {id}")],
        );
    }
    waited(&e).await;
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    for id in 2..=5 {
        assert!(
            body.contains(&format!("Failed at step `Run step {id}`:")),
            "{id}"
        );
    }
    let read = e.state.lock().await.failures.clone();
    assert!(read
        .iter()
        .all(|f| f.retry_at.is_none() && f.error.is_none()));
}

/// A private key in the failed step, longer than the lines Orbit shows,
/// holds the step's log back, so none of it is saved or posted.
#[cfg(unix)]
#[tokio::test]
async fn a_long_private_key_is_never_kept() {
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
    links(d.path(), &[("Rust", &job_url(2))]);
    let mut log = vec![
        "##[group]Run ./deploy.sh".to_string(),
        "##[endgroup]".into(),
        format!("-----BEGIN {} PRIVATE KEY-----", "RSA"),
    ];
    log.extend((0..30).map(|n| format!("MIIEkeybody{n:02}{}", "Q".repeat(50))));
    log.push(format!("-----END {} PRIVATE KEY-----", "RSA"));
    log.push("##[error]Process completed with exit code 1.".into());
    let log: Vec<&str> = log.iter().map(String::as_str).collect();
    job(d.path(), 2, "Run ./deploy.sh", &log);
    watch(&e, &gh).await;
    let body = github(d.path())["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("Orbit held back this job's log: it looks like it holds a secret."));
    let saved = std::fs::read_to_string(d.path().join("state").join("state.json")).unwrap();
    for text in [&body, &saved] {
        assert!(!text.contains("keybody") && !text.contains("PRIVATE KEY"));
    }
}

/// Job `id` in run 1 at commit 5, try `attempt` of the check `name`, which
/// failed at `step` with this log.
fn actions_job(dir: &Path, id: u32, name: &str, attempt: u32, step: &str, log: &[&str]) {
    in_run(dir, (1, '5'), id, name, attempt, step, log);
}
/// The same in another run, at another commit.
fn in_run(
    dir: &Path,
    (run, at): (u32, char),
    id: u32,
    name: &str,
    attempt: u32,
    step: &str,
    log: &[&str],
) {
    job(dir, id, step, log);
    let file = dir.join(format!("job-{id}.json"));
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    for (key, value) in [
        ("id", json!(id)),
        ("run_id", json!(run)),
        ("head_sha", json!(sha(at))),
        ("name", json!(name)),
        ("run_attempt", json!(attempt)),
        ("status", json!("completed")),
        ("conclusion", json!("failure")),
    ] {
        v[key] = value;
    }
    std::fs::write(file, v.to_string()).unwrap();
}
/// As if GitHub finished rerunning a job as job `next`, try `attempt`,
/// which failed when the runner went away.
fn rerun_lost(dir: &Path, next: u32, attempt: u32) {
    actions_job(
        dir,
        next,
        "Rust",
        attempt,
        "Run cargo test",
        &["##[error]The runner has received a shutdown signal."],
    );
    links(dir, &[("Rust", &job_url(next))]);
}
fn reruns_sent(dir: &Path) -> Vec<String> {
    writes(dir)
        .into_iter()
        .filter(|w| w.ends_with("/rerun"))
        .collect()
}
fn comment(dir: &Path) -> String {
    github(dir)["comments"][0]["body"]
        .as_str()
        .unwrap()
        .to_string()
}
/// Whether Bit has a note that says `text`.
async fn noted(e: &Engine, text: &str) -> bool {
    let notes = e.snapshot().await.buddy_feed.notes;
    notes.iter().any(|n| n.text.contains(text))
}

/// A job that failed getting the code is rerun once, through the journal,
/// before the comment says so; when it fails again Bit asks before a second
/// rerun, and there is never a third at one commit. A new commit starts
/// again.
#[cfg(unix)]
#[tokio::test]
async fn a_flaky_job_is_rerun_once_and_never_twice_without_a_tap() {
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
    links(d.path(), &[("Rust", &job_url(2))]);
    actions_job(
        d.path(),
        2,
        "Rust",
        1,
        "Run actions/checkout@v4",
        &[
            "fatal: unable to access 'https://github.com/a/b/': Could not resolve host: github.com",
            "##[error]Process completed with exit code 128.",
        ],
    );
    watch(&e, &gh).await;
    assert_eq!(
        writes(d.path()),
        [
            "POST repos/a/b/actions/jobs/2/rerun",
            "POST repos/a/b/issues/12/comments"
        ],
        "rerun first, so the comment says so"
    );
    assert!(comment(d.path()).contains(&format!(
        "[details]({})\n  Orbit reran this job, as it looks like bad luck: It failed getting the code or tools ready, before any test ran.\n  Failed at step `Run actions/checkout@v4`:",
        job_url(2)
    )));
    let s = e.snapshot().await;
    let w = &s.journal[0];
    assert_eq!(w.action, crate::ops::Action::RerunJob);
    assert_eq!((w.state, w.url.as_str()), (WriteState::Done, &*job_url(2)));
    assert_eq!(w.number, Some(12));
    assert!(noted(&e, "Orbit reran I1 (CI / Rust) on a/b #12, as it looks like bad luck: It failed getting the code or tools ready, before any test ran.").await);
    // Once.
    watch(&e, &gh).await;
    assert_eq!(reruns_sent(d.path()).len(), 1);

    // It fails again, when the runner goes away: Bit asks first.
    rerun_lost(d.path(), 1002, 2);
    watch(&e, &gh).await;
    assert_eq!(reruns_sent(d.path()).len(), 1);
    assert!(comment(d.path()).contains(&format!(
        "[details]({})\n  It failed again after it was rerun, so it's likely real.\n",
        job_url(1002)
    )));
    assert!(noted(&e, "I1 (CI / Rust) failed again on a/b #12 after a rerun. It still looks like bad luck: The runner went away. Tap Rerun again on Operations to try once more.").await);
    // How the rerun turned out: a miss, which Bit noted just above, once.
    e.flush().await.unwrap();
    let s = e.snapshot().await;
    let reran: Vec<_> = s
        .outcomes
        .iter()
        .filter(|o| o.kind == crate::outcomes::Kind::Rerun)
        .collect();
    assert_eq!(reran.len(), 1);
    assert_eq!(
        (
            reran[0].what.as_str(),
            reran[0].verdict,
            reran[0].why.as_str()
        ),
        (
            "Orbit reran CI / Rust at 5555555 on a/b #12.",
            crate::outcomes::Verdict::Missed,
            "It failed again after the rerun."
        )
    );
    assert!(!s
        .buddy_feed
        .notes
        .iter()
        .any(|n| n.key.as_deref().is_some_and(|k| k.starts_with("outcome:"))));
    assert!(e.allow_rerun("pr:a/b#12", 9).await.is_err());
    e.allow_rerun("pr:a/b#12", 1).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(
        reruns_sent(d.path()),
        [
            "POST repos/a/b/actions/jobs/2/rerun",
            "POST repos/a/b/actions/jobs/1002/rerun"
        ]
    );
    assert!(comment(d.path()).contains(
        "Orbit reran this job a second time, as it still looks like bad luck: The runner went away.\n"
    ));

    // Never a third, even with a tap.
    rerun_lost(d.path(), 2002, 3);
    watch(&e, &gh).await;
    e.allow_rerun("pr:a/b#12", 1).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(reruns_sent(d.path()).len(), 2);
    assert!(comment(d.path()).contains("It failed again after two reruns, so it's likely real.\n"));
    assert!(
        noted(
            &e,
            "I1 (CI / Rust) failed again on a/b #12 after a rerun, so it's likely real."
        )
        .await
    );

    // A new commit starts again.
    e.update(|s| s.prs = vec![pr('6')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('6', &[("Rust", "FAILURE")])),
    );
    let url = "https://github.com/a/b/actions/runs/3/job/3".to_string();
    links(d.path(), &[("Rust", &url)]);
    in_run(
        d.path(),
        (3, '6'),
        3,
        "Rust",
        1,
        "Set up job",
        &["##[error]boom"],
    );
    // The job alone says it failed setting up, without its log.
    std::fs::remove_file(d.path().join("log-3.txt")).unwrap();
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()).last().unwrap(),
        "POST repos/a/b/actions/jobs/3/rerun"
    );
}

/// Orbit waits for a job's workflow to finish before rerunning it; a rerun
/// cut off by a restart is found on GitHub rather than sent again; a rerun
/// GitHub refuses isn't asked again, and the rules decide.
#[cfg(unix)]
#[tokio::test]
async fn a_rerun_is_never_sent_twice() {
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
    links(d.path(), &[("Rust", &job_url(2))]);
    actions_job(d.path(), 2, "Rust", 1, "Set up job", &["##[error]boom"]);
    let docs = json!({ "id": 9, "run_id": 1, "name": "Docs", "status": "in_progress" });
    std::fs::write(d.path().join("job-9.json"), docs.to_string()).unwrap();
    watch(&e, &gh).await;
    assert!(reruns_sent(d.path()).is_empty());
    let s = e.snapshot().await;
    let r = &s.ops[0].items[0].reruns[0];
    assert_eq!(
        r.error.as_deref(),
        Some("Its workflow is still running, so Orbit reruns it once that finishes.")
    );
    assert!(s
        .journal
        .iter()
        .all(|w| w.action != crate::ops::Action::RerunJob));

    // Once it has finished.
    let docs = json!({ "id": 9, "run_id": 1, "name": "Docs", "status": "completed" });
    std::fs::write(d.path().join("job-9.json"), docs.to_string()).unwrap();
    let due = |s: &mut Snapshot| {
        for r in s
            .ops
            .iter_mut()
            .flat_map(|t| &mut t.items)
            .flat_map(|i| &mut i.reruns)
        {
            r.retry_at = r
                .retry_at
                .as_ref()
                .map(|_| "2026-01-01T00:00:00.000Z".into());
        }
    };
    e.update(due).await;
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()),
        ["POST repos/a/b/actions/jobs/2/rerun"]
    );

    // As if Orbit stopped once the rerun reached GitHub, before it answered:
    // its item and journal are as they were saved before it was sent. Then
    // the new try fails too, before Orbit starts again.
    e.update(|s| {
        let r = &mut s.ops[0].items[0].reruns[0];
        (r.done, r.unsure, r.at) = (false, true, String::new());
        let w = s
            .journal
            .iter_mut()
            .find(|w| w.action == crate::ops::Action::RerunJob)
            .unwrap();
        assert_eq!(w.url, job_url(2), "the journal names the job");
        w.state = WriteState::Sending;
        w.done_at = None;
    })
    .await;
    e.flush().await.unwrap();
    drop(e);
    rerun_lost(d.path(), 1002, 2);
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('5')]).await;
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()).len(),
        1,
        "found, not sent again, and the new try not rerun without a tap"
    );
    let s = e.snapshot().await;
    let item = &s.ops[0].items[0];
    assert_eq!(item.url, job_url(1002));
    let r = &item.reruns[0];
    assert!(r.done && !r.already && !r.unsure);
    let cut = s
        .journal
        .iter()
        .find(|w| w.action == crate::ops::Action::RerunJob)
        .unwrap();
    assert_eq!(cut.state, WriteState::Failed);
    watch(&e, &gh).await;
    assert_eq!(reruns_sent(d.path()).len(), 1);
    assert!(
        comment(d.path()).contains("It failed again after it was rerun, so it's likely real.\n")
    );
    assert!(noted(&e, "Tap Rerun again on Operations").await);

    // A new commit whose rerun GitHub refuses.
    e.update(|s| s.prs = vec![pr('6')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('6', &[("Rust", "FAILURE")])),
    );
    let url = "https://github.com/a/b/actions/runs/3/job/3".to_string();
    links(d.path(), &[("Rust", &url)]);
    in_run(
        d.path(),
        (3, '6'),
        3,
        "Rust",
        1,
        "Set up job",
        &["##[error]boom"],
    );
    std::fs::write(
        d.path().join("rerun-error"),
        "gh: Resource not accessible by integration (HTTP 403)\n",
    )
    .unwrap();
    watch(&e, &gh).await;
    e.update(due).await;
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path())[1..],
        ["POST repos/a/b/actions/jobs/3/rerun"],
        "asked once"
    );
    let s = e.snapshot().await;
    let r = &s.ops[0].items[0].reruns[0];
    assert!(r.lasting && !r.done);
    assert_eq!(
        r.error.as_deref(),
        Some("GitHub won't rerun this job: Resource not accessible by integration (HTTP 403).")
    );
    assert!(
        !comment(d.path()).contains("rerun"),
        "nothing to say on GitHub"
    );

    // The rules decide.
    std::fs::remove_file(d.path().join("rerun-error")).unwrap();
    e.update(|s| s.prs = vec![pr('7')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('7', &[("Rust", "FAILURE")])),
    );
    let url = "https://github.com/a/b/actions/runs/4/job/4".to_string();
    links(d.path(), &[("Rust", &url)]);
    in_run(
        d.path(),
        (4, '7'),
        4,
        "Rust",
        1,
        "Set up job",
        &["##[error]boom"],
    );
    e.update(|s| s.settings.buddy.rules.rerun_flaky = "leave_it".into())
        .await;
    watch(&e, &gh).await;
    e.update(|s| s.settings.buddy.rules.rerun_flaky = "ask_first".into())
        .await;
    watch(&e, &gh).await;
    assert_eq!(reruns_sent(d.path()).len(), 2);
    e.allow_rerun("pr:a/b#12", 1).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(
        reruns_sent(d.path()).last().unwrap(),
        "POST repos/a/b/actions/jobs/4/rerun"
    );
}

/// A rerun cut off before it reached GitHub isn't sent on the strength of
/// the look alone: once Orbit finds no newer try, the rules decide again.
#[cfg(unix)]
#[tokio::test]
async fn a_rerun_github_never_got_waits_for_the_rules() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.rules.rerun_flaky = "ask_first".into();
    })
    .await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Rust", "FAILURE")])),
    );
    links(d.path(), &[("Rust", &job_url(2))]);
    actions_job(d.path(), 2, "Rust", 1, "Set up job", &["##[error]boom"]);
    watch(&e, &gh).await;
    assert!(reruns_sent(d.path()).is_empty());
    // As if a rerun you tapped was saved, then Orbit stopped before sending.
    e.update(|s| {
        s.ops[0].items[0].reruns = vec![crate::reruns::Rerun {
            url: job_url(2),
            unsure: true,
            ..Default::default()
        }];
    })
    .await;
    watch(&e, &gh).await;
    assert!(reruns_sent(d.path()).is_empty(), "Bit asks first now");
    let s = e.snapshot().await;
    assert!(!s.ops[0].items[0].reruns[0].unsure);
    e.update(|s| s.settings.buddy.rules.rerun_flaky = "do_it".into())
        .await;
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()),
        ["POST repos/a/b/actions/jobs/2/rerun"]
    );
}

/// A fake Ollama that answers every quick call `answer`, and the chats it
/// was sent.
async fn quick(answer: &str) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
    let fake = crate::ollama::tests::Fake {
        content: json!({ "answer": answer }).to_string(),
        ..Default::default()
    };
    let chats = fake.chats.clone();
    (crate::ollama::tests::serve(fake).await, chats)
}
/// The pull request at commit `at` failing `CI / Rust` by job `id` of run
/// `run`, which failed at `step` with this log.
fn quick_failure(dir: &Path, (run, at): (u32, char), id: u32, step: &str, log: &[&str]) {
    answer(
        dir,
        '1',
        &[("Rust", "SUCCESS")],
        Some((at, &[("Rust", "FAILURE")])),
    );
    links(
        dir,
        &[(
            "Rust",
            &format!("https://github.com/a/b/actions/runs/{run}/job/{id}"),
        )],
    );
    in_run(dir, (run, at), id, "Rust", 1, step, log);
}
const NO_HOST: [&str; 2] = [
    "fatal: unable to access 'https://github.com/a/b/': Could not resolve host: github.com",
    "##[error]Process completed with exit code 128.",
];

/// In Watch, Orbit asks Ollama once about each failed job, with its check,
/// step and log, and keeps the answer while the rules decide; once the
/// rerun fails again, it scores both.
#[cfg(unix)]
#[tokio::test]
async fn a_quick_call_in_watch_is_kept_while_the_rules_decide() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (url, chats) = quick("no").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = url.clone();
        s.settings.buddy.quick_model = "small".into();
    })
    .await;
    quick_failure(d.path(), (1, '5'), 2, "Run actions/checkout@v4", &NO_HOST);
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()),
        ["POST repos/a/b/actions/jobs/2/rerun"],
        "the rules decide"
    );
    let s = e.snapshot().await;
    assert_eq!(s.calls.len(), 1);
    let c = &s.calls[0];
    assert_eq!(
        (c.url.as_str(), c.number, c.mode.as_str(), c.model.as_str()),
        (&*job_url(2), Some(12), "watch", "small")
    );
    assert_eq!((c.rules, c.said), (true, Some(crate::calls::Said::No)));
    let sent = chats.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["model"], "small");
    assert_eq!(sent[0]["format"], crate::calls::schema());
    let asked = sent[0]["messages"][1]["content"].as_str().unwrap();
    assert!(
        asked.starts_with("Check: CI / Rust\nStep that failed: Run actions/checkout@v4\n")
            && asked.contains("Could not resolve host"),
        "{asked}"
    );
    watch(&e, &gh).await;
    assert_eq!(chats.lock().unwrap().len(), 1, "once");

    // The rerun fails again: the rules were wrong, and the model right. The
    // new job gets a call of its own.
    rerun_lost(d.path(), 1002, 2);
    watch(&e, &gh).await;
    e.flush().await.unwrap();
    let s = e.snapshot().await;
    let c = s.calls.iter().find(|c| c.url == job_url(2)).unwrap();
    assert_eq!(
        (c.truth, c.how.as_str()),
        (Some(false), "It failed again after a rerun.")
    );
    assert_eq!(s.calls.len(), 2);
    let score = crate::calls::score(&s.calls, crate::calls::LUCK, "small");
    assert_eq!(
        (
            score.scored,
            score.rules_right,
            score.model_right,
            score.on_right
        ),
        (1, 0, 1, 1)
    );
}

/// In On, a clear answer decides: a no keeps Orbit from rerunning what the
/// rules call bad luck, and a yes reruns what they don't, by Bit's rule.
/// With no answer, the rules decide.
#[cfg(unix)]
#[tokio::test]
async fn a_quick_call_that_is_on_decides_whether_a_job_is_rerun() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (no, _) = quick("no").await;
    let (yes, _) = quick("yes").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = no.clone();
        s.settings.buddy.quick_model = "small".into();
        s.settings.buddy.luck_call = "on".into();
    })
    .await;
    quick_failure(d.path(), (1, '5'), 2, "Run actions/checkout@v4", &NO_HOST);
    watch(&e, &gh).await;
    assert!(reruns_sent(d.path()).is_empty());
    assert!(!comment(d.path()).contains("Orbit reran"));
    let s = e.snapshot().await;
    assert_eq!(s.calls[0].mode, "on");
    // The rules' own answer still shows.
    assert!(s.failures[0].flaky.is_some());
    // Back in Watch the rules decide again at once, before the next pass,
    // and the call does again in On.
    let why = |s: &Snapshot| {
        let mut items = s.ops.iter().flat_map(|t| &t.items);
        let item = items.find(|i| i.url == job_url(2)).unwrap();
        item.why.as_ref().and_then(|w| w.flaky.clone())
    };
    assert_eq!(why(&s), None);
    let mode = |mode: &str| {
        serde_json::from_value::<SettingsPatch>(json!({"buddy": {"luckCall": mode}})).unwrap()
    };
    e.update_settings(mode("watch")).await.unwrap();
    assert!(why(&e.snapshot().await).is_some());
    e.update_settings(mode("on")).await.unwrap();
    assert_eq!(why(&e.snapshot().await), None);

    // A test fails at a new commit: the rules don't call it bad luck, the
    // model does.
    e.update(|s| {
        s.prs = vec![pr('6')];
        s.settings.buddy.ollama_url = yes.clone();
    })
    .await;
    quick_failure(
        d.path(),
        (3, '6'),
        3,
        "Run cargo test",
        &[
            "test parses ... FAILED",
            "##[error]Process completed with exit code 101.",
        ],
    );
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()),
        ["POST repos/a/b/actions/jobs/3/rerun"]
    );
    assert!(
        noted(
            &e,
            "as it looks like bad luck: Orbit's quick call on Ollama said so."
        )
        .await
    );

    // Nothing answers: the rules decide.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    e.update(|s| {
        s.prs = vec![pr('7')];
        s.settings.buddy.ollama_url = gone.clone();
    })
    .await;
    quick_failure(d.path(), (4, '7'), 4, "Run actions/checkout@v4", &NO_HOST);
    watch(&e, &gh).await;
    assert_eq!(
        reruns_sent(d.path()).last().unwrap(),
        "POST repos/a/b/actions/jobs/4/rerun"
    );
    let s = e.snapshot().await;
    let c = s.calls.iter().find(|c| c.url.ends_with("/job/4")).unwrap();
    assert!(c.said.is_none() && c.error.as_deref().unwrap().contains("Nothing answered"));
}

/// A pull request failing a check that fails on main too hears so in
/// Orbit's comment. In Watch, Orbit asks Ollama once whether it was failing
/// already, with both logs, and keeps the answer while the rule decides;
/// in On, a no takes the note away.
/// Main, at commit 1, and pull request 12, at commit 5, both fail Rust,
/// main by job 7 and the pull request by job 2, with the same log.
fn both_fail_rust(dir: &Path) {
    answer(
        dir,
        '1',
        &[("Rust", "FAILURE")],
        Some(('5', &[("Rust", "FAILURE")])),
    );
    let file = dir.join("answer.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let repo = &mut v["data"]["repository"];
    repo["defaultBranchRef"]["target"]["statusCheckRollup"]["contexts"]["nodes"][0]["detailsUrl"] =
        json!(job_url(7));
    repo["pullRequests"]["nodes"][0]["commits"]["nodes"][0]["commit"]["statusCheckRollup"]
        ["contexts"]["nodes"][0]["detailsUrl"] = json!(job_url(2));
    std::fs::write(&file, v.to_string()).unwrap();
    for (run, id) in [((1, '5'), 2), ((9, '1'), 7)] {
        in_run(
            dir,
            run,
            id,
            "Rust",
            1,
            "Run cargo test",
            &[
                "test parses ... FAILED",
                "##[error]Process completed with exit code 101.",
            ],
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_pull_request_failing_a_check_main_fails_too_hears_so() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (no, chats) = quick("no").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = no.clone();
        s.settings.buddy.quick_model = "small".into();
        s.settings.buddy.luck_call = "off".into();
    })
    .await;
    both_fail_rust(d.path());
    watch(&e, &gh).await;
    let note = "It fails on main too, at 1111111, so it may not come from this pull request.";
    let posted = github(d.path())["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["body"].as_str().unwrap().to_string())
        .find(|b| b.contains("`CI / Rust`"))
        .unwrap();
    assert!(posted.contains(note), "{posted}");
    let s = e.snapshot().await;
    let c = s.calls.iter().find(|c| c.question == "fault").unwrap();
    assert_eq!(
        (c.url.as_str(), c.number, c.rules, c.said),
        (&*job_url(2), Some(12), true, Some(crate::calls::Said::No))
    );
    let sent = chats.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["messages"][0]["content"],
        crate::calls::FAULT_INSTRUCTIONS
    );
    let asked = sent[0]["messages"][1]["content"].as_str().unwrap();
    assert!(
        asked.starts_with(
            "Check: CI / Rust\nOn the pull request, the step that failed: Run cargo test\n"
        ) && asked.contains("On the default branch, the step that failed: Run cargo test"),
        "{asked}"
    );
    watch(&e, &gh).await;
    assert_eq!(chats.lock().unwrap().len(), 1, "once");

    // In On, the model's no takes the note away.
    e.update(|s| s.settings.buddy.fault_call = "on".into())
        .await;
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let mut items = s.ops.iter().flat_map(|t| &t.items);
    let item = items.find(|i| i.url == job_url(2)).unwrap();
    assert_eq!(item.why.as_ref().unwrap().already, None);
    assert!(!crate::calls::told_already(&s, &job_url(2)));
    let edited = github(d.path())["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["body"].as_str().unwrap().to_string())
        .find(|b| b.contains("`CI / Rust`"))
        .unwrap();
    assert!(!edited.contains("fails on main"), "{edited}");
}

/// Says Orbit's sign-in may only `permission` in the repository, in the
/// CI read `answer` wrote.
fn may_only(dir: &Path, permission: &str) {
    let path = dir.join("answer.json");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["data"]["repository"]["viewerPermission"] = json!(permission);
    std::fs::write(path, v.to_string()).unwrap();
}
/// A fake Ollama that answers every second look `about`, and yes to the
/// other questions, and the chats it was sent.
async fn looks_at(about: &str) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
    let fake = crate::ollama::tests::Fake {
        models: vec!["big".into()],
        content: json!({
            "about": about,
            "matches": "yes",
            "nothingPrivate": "yes",
            "needed": "yes",
        })
        .to_string(),
        ..Default::default()
    };
    let chats = fake.chats.clone();
    (crate::ollama::tests::serve(fake).await, chats)
}
/// Orbit's comments on GitHub about `CI / Rust`.
fn rust_comments(dir: &Path) -> usize {
    github(dir)["comments"].as_array().map_or(0, |c| {
        c.iter()
            .filter(|c| c["body"].as_str().unwrap_or("").contains("`CI / Rust`"))
            .count()
    })
}

/// A new comment in a repository where Orbit's sign-in isn't an admin gets
/// a second look first: in Watch it is recorded and posted, and it shows
/// what happened next. One where it's an admin gets none.
#[cfg(unix)]
#[tokio::test]
async fn a_post_where_you_arent_an_admin_gets_a_second_look() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (url, chats) = looks_at("yes").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = url.clone();
        s.settings.buddy.quick_model = "small".into();
        s.settings.buddy.luck_call = "off".into();
        s.settings.buddy.fault_call = "off".into();
    })
    .await;
    quick_failure(
        d.path(),
        (1, '5'),
        2,
        "Run cargo test",
        &["##[error]exit code 1"],
    );
    may_only(d.path(), "WRITE");
    watch(&e, &gh).await;
    assert_eq!(rust_comments(d.path()), 1, "Watch posts it");
    let s = e.snapshot().await;
    assert_eq!(s.ci[0].permission, "WRITE");
    assert_eq!(s.looks.len(), 1);
    let l = &s.looks[0];
    assert_eq!(
        (l.repo.as_str(), l.number, l.mode.as_str(), l.model.as_str()),
        ("a/b", Some(12), "watch", "small")
    );
    assert_eq!(l.verdict, crate::looks::Verdict::Post);
    assert_eq!(l.permission, "WRITE");
    assert!(
        !l.posted_at.is_empty() && l.url.contains("/12#issuecomment-"),
        "{l:?}"
    );
    let sent = chats.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["messages"][0]["content"],
        crate::looks::INSTRUCTIONS
    );
    let asked = sent[0]["messages"][1]["content"].as_str().unwrap();
    assert!(
        asked.contains("Where it goes: a new comment on pull request #12")
            && asked.contains("Failing checks: CI / Rust")
            && asked.contains("`CI / Rust`")
            && !asked.contains("<!--"),
        "{asked}"
    );
    assert_eq!(sent[0]["format"], crate::looks::schema());
    // Edits to it get none.
    quick_failure(
        d.path(),
        (1, '6'),
        3,
        "Run cargo test",
        &["##[error]exit code 1"],
    );
    may_only(d.path(), "WRITE");
    watch(&e, &gh).await;
    assert_eq!(chats.lock().unwrap().len(), 1, "no look at an edit");

    // Where Orbit's sign-in is an admin, none.
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (url, chats) = looks_at("yes").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = url.clone();
        s.settings.buddy.quick_model = "small".into();
        s.settings.buddy.luck_call = "off".into();
        s.settings.buddy.fault_call = "off".into();
        s.settings.buddy.second_look = "on".into();
    })
    .await;
    quick_failure(
        d.path(),
        (1, '5'),
        2,
        "Run cargo test",
        &["##[error]exit code 1"],
    );
    watch(&e, &gh).await;
    assert_eq!(rust_comments(d.path()), 1);
    assert!(e.snapshot().await.looks.is_empty());
    assert!(chats.lock().unwrap().is_empty());
}

/// You can have whoever answers your questions take the second look: here
/// Ollama with Bit's own model, with no quick call model chosen.
#[cfg(unix)]
#[tokio::test]
async fn bits_answerer_can_take_the_second_look() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (url, chats) = looks_at("yes").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.answerer = "ollama".into();
        s.settings.buddy.ollama_url = url.clone();
        s.settings.buddy.ollama_model = "big".into();
        s.settings.buddy.second_look_by = "answerer".into();
        s.ollama = crate::types::CliStatus {
            ready: true,
            ..Default::default()
        };
        s.ollama_models = vec!["big".into()];
    })
    .await;
    quick_failure(
        d.path(),
        (1, '5'),
        2,
        "Run cargo test",
        &["##[error]exit code 1"],
    );
    may_only(d.path(), "TRIAGE");
    watch(&e, &gh).await;
    assert_eq!(rust_comments(d.path()), 1);
    let s = e.snapshot().await;
    let l = &s.looks[0];
    assert_eq!(
        (l.by.as_str(), l.model.as_str(), l.verdict),
        ("Ollama", "big", crate::looks::Verdict::Post)
    );
    let sent = chats.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["model"], "big");
    assert_eq!(
        sent[0]["messages"][0]["content"],
        crate::looks::INSTRUCTIONS
    );
}

/// In On, a post the second look says should wait stays off GitHub, Bit
/// says why, and your tap posts it.
#[cfg(unix)]
#[tokio::test]
async fn on_holds_a_post_the_second_look_says_should_wait() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (url, chats) = looks_at("no").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = url.clone();
        s.settings.buddy.quick_model = "small".into();
        s.settings.buddy.luck_call = "off".into();
        s.settings.buddy.fault_call = "off".into();
        s.settings.buddy.second_look = "on".into();
    })
    .await;
    quick_failure(
        d.path(),
        (1, '5'),
        2,
        "Run cargo test",
        &["##[error]exit code 1"],
    );
    may_only(d.path(), "MAINTAIN");
    watch(&e, &gh).await;
    assert_eq!(rust_comments(d.path()), 0, "it waits");
    let s = e.snapshot().await;
    let l = &s.looks[0];
    assert_eq!(l.verdict, crate::looks::Verdict::Wait);
    assert!(l.held);
    assert_eq!(
        l.why,
        "The model said it isn't about this repository's problem."
    );
    let told = s
        .buddy_feed
        .notes
        .iter()
        .find(|n| n.text.contains("second look held"))
        .map(|n| n.text.clone());
    assert!(
        told.as_deref()
            .is_some_and(|t| t.contains("a/b #12") && t.contains("Post comment")),
        "{told:?}"
    );
    watch(&e, &gh).await;
    assert_eq!(rust_comments(d.path()), 0, "still waiting");
    assert_eq!(chats.lock().unwrap().len(), 1, "one look");
    let key = s.ops[0].key.clone();
    e.allow_telling(&key).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(rust_comments(d.path()), 1, "your tap posts it");
    let s = e.snapshot().await;
    assert!(!s.looks[0].posted_at.is_empty());
    assert!(s.looks[0].held);
    assert_eq!(chats.lock().unwrap().len(), 1);
}

/// A fix the agent named counts against "failing already" as soon as
/// Orbit hears it: Orbit reads answers after it follows cases, and scores
/// calls by the next CI read before it follows them again.
#[cfg(unix)]
#[tokio::test]
async fn a_fix_the_agent_named_keeps_a_failing_already_call_unscored() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let (yes, _) = quick("yes").await;
    let e = connected(d.path()).await;
    e.update(|s| {
        s.prs = vec![pr('5')];
        s.settings.buddy.ollama_url = yes.clone();
        s.settings.buddy.quick_model = "small".into();
        s.settings.buddy.luck_call = "off".into();
    })
    .await;
    both_fail_rust(d.path());
    watch(&e, &gh).await;
    let call = |s: &Snapshot| {
        s.calls
            .iter()
            .find(|c| c.question == "fault")
            .cloned()
            .unwrap()
    };
    assert_eq!(
        call(&e.snapshot().await).said,
        Some(crate::calls::Said::Yes)
    );

    // The agent says it fixed Rust, and main passes it.
    say(d.path(), "agent", "Fixed I1 in 6666666");
    compared(d.path(), '6', '5', "behind");
    answer(
        d.path(),
        '3',
        &[("Rust", "SUCCESS")],
        Some(('5', &[("Rust", "FAILURE")])),
    );
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let c = call(&s);
    assert!(!c.base_passed_at.is_empty() && c.settled_at.is_none());
    let answered = s.ops.iter().flat_map(|t| &t.items).any(|i| {
        i.answer
            .as_ref()
            .is_some_and(|a| a.kind == crate::ops::AnswerKind::Fixed)
    });
    let case = s.cases.iter().find(|k| k.id == c.problem).unwrap();
    assert!(answered && !case.fixed, "the case hears of the fix later");

    // The pull request passes at its next commit: its own fix, maybe.
    e.update(|s| s.prs = vec![pr('6')]).await;
    answer(
        d.path(),
        '3',
        &[("Rust", "SUCCESS")],
        Some(('6', &[("Rust", "SUCCESS")])),
    );
    watch(&e, &gh).await;
    let c = call(&e.snapshot().await);
    assert_eq!(
        (c.truth, c.how.as_str(), c.settled_at.is_some()),
        (None, "Orbit couldn't tell how it turned out.", true)
    );
}

/// Quick calls go one at a time, and each reads the settings again first:
/// once Ollama moves, the answer from the old address is dropped and the
/// job is asked about again at the new one; once the calls or Watch CI are
/// off, or the repository is disconnected, no other log is sent and the
/// answer is dropped.
#[tokio::test]
async fn each_quick_call_reads_the_settings_again_before_it_sends_a_log() {
    use crate::ollama::tests::{serve, Fake};
    let slow = |answer: &str| Fake {
        content: json!({ "answer": answer }).to_string(),
        delay: std::time::Duration::from_millis(300),
        ..Default::default()
    };
    let job = |id: u32| format!("https://github.com/a/b/actions/runs/1/job/{id}");
    let failing = |s: &mut Snapshot| {
        s.settings.buddy.quick_model = "small".into();
        s.problems = vec![crate::problems::Problem {
            id: "p1".into(),
            key: "ci:a/b#12".into(),
            kind: crate::problems::ProblemKind::PullRequest,
            repo: "a/b".into(),
            pr_id: Some("a/b#12".into()),
            sha: "5".repeat(40),
            failing: [2, 3]
                .map(|id| crate::ci::Check {
                    name: format!("CI / {id}"),
                    url: job(id),
                })
                .to_vec(),
            opened_at: crate::problems::stamp(chrono::Utc::now()),
            ..Default::default()
        }];
        s.failures = [2, 3]
            .map(|id| crate::logs::Failure {
                url: job(id),
                check: format!("CI / {id}"),
                step: "Run cargo test".into(),
                lines: vec![format!("line of job {id}")],
                read_at: crate::problems::stamp(chrono::Utc::now()),
                ..Default::default()
            })
            .to_vec();
    };
    let sent = |chats: &Arc<std::sync::Mutex<Vec<Value>>>| -> Vec<String> {
        let chats = chats.lock().unwrap();
        let asked = chats
            .iter()
            .map(|c| c["messages"][1]["content"].to_string());
        asked.collect()
    };
    async fn first(chats: &Arc<std::sync::Mutex<Vec<Value>>>) {
        for _ in 0..500 {
            if !chats.lock().unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("nothing was asked");
    }

    // Ollama moves while the first call waits.
    let d = tempfile::tempdir().unwrap();
    let (old, new) = (slow("yes"), slow("no"));
    let (old_chats, new_chats) = (old.chats.clone(), new.chats.clone());
    let (old, new) = (serve(old).await, serve(new).await);
    let e = connected(d.path()).await;
    e.update(|s| {
        failing(s);
        s.settings.buddy.ollama_url = old.clone();
    })
    .await;
    let calls = tokio::spawn({
        let e = e.clone();
        async move { e.make_calls().await }
    });
    first(&old_chats).await;
    e.update(|s| s.settings.buddy.ollama_url = new.clone())
        .await;
    calls.await.unwrap();
    assert_eq!(sent(&old_chats).len(), 1);
    assert_eq!(sent(&new_chats), sent(&old_chats), "the same job again");
    let s = e.snapshot().await;
    assert_eq!(s.calls.len(), 1);
    assert_eq!(
        (s.calls[0].url.as_str(), s.calls[0].said),
        (&*job(2), Some(crate::calls::Said::No))
    );

    // The calls, or Watch CI, go off while the first waits, or the
    // repository is disconnected.
    for change in [
        (|s: &mut Snapshot| s.settings.buddy.luck_call = "off".into()) as fn(&mut Snapshot),
        |s| s.settings.watch_ci = false,
        |s| s.settings.repositories.clear(),
    ] {
        let d = tempfile::tempdir().unwrap();
        let fake = slow("yes");
        let chats = fake.chats.clone();
        let url = serve(fake).await;
        let e = connected(d.path()).await;
        e.update(|s| {
            failing(s);
            s.settings.buddy.ollama_url = url.clone();
        })
        .await;
        let calls = tokio::spawn({
            let e = e.clone();
            async move { e.make_calls().await }
        });
        first(&chats).await;
        e.update(change).await;
        calls.await.unwrap();
        assert_eq!(sent(&chats).len(), 1, "no other log");
        assert!(e.snapshot().await.calls.is_empty());
    }
}

/// A check whose link is a job at another commit, or another check's job,
/// isn't Orbit's to rerun.
#[cfg(unix)]
#[tokio::test]
async fn a_link_to_another_job_is_never_rerun() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.prs = vec![pr('8')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('8', &[("Rust", "FAILURE")])),
    );
    // A job that looks like bad luck, but at commit 5.
    links(d.path(), &[("Rust", &job_url(2))]);
    actions_job(d.path(), 2, "Rust", 1, "Set up job", &["##[error]boom"]);
    watch(&e, &gh).await;
    assert!(reruns_sent(d.path()).is_empty());
    let s = e.snapshot().await;
    let r = &s.ops[0].items[0].reruns[0];
    assert!(r.lasting && !r.done);
    assert_eq!(
        r.error.as_deref(),
        Some("Its link isn't this check's job at this commit, so Orbit won't rerun it.")
    );
    let read = calls(d.path(), "actions/jobs/2");
    watch(&e, &gh).await;
    assert_eq!(
        calls(d.path(), "actions/jobs/2"),
        read,
        "not looked at again"
    );
    assert_eq!(calls(d.path(), "actions/runs/1/jobs"), 0, "never listed");

    // At the pull request's new head, but another check's job.
    e.update(|s| s.prs = vec![pr('9')]).await;
    answer(
        d.path(),
        '1',
        &[("Rust", "SUCCESS")],
        Some(('9', &[("Rust", "FAILURE")])),
    );
    let url = "https://github.com/a/b/actions/runs/9/job/9".to_string();
    links(d.path(), &[("Rust", &url)]);
    in_run(
        d.path(),
        (9, '9'),
        9,
        "Lint",
        1,
        "Set up job",
        &["##[error]boom"],
    );
    watch(&e, &gh).await;
    assert!(reruns_sent(d.path()).is_empty());
    let s = e.snapshot().await;
    assert!(s.ops[0].items[0].reruns[0].lasting);
}

/// Orbit reads every page of a run's jobs before it reruns one: a newer
/// try, or a job still running, on a later page counts.
#[cfg(unix)]
#[tokio::test]
async fn every_page_of_a_runs_jobs_is_read() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    for id in 600..720 {
        let other =
            json!({ "id": id, "run_id": 7, "name": format!("Shard {id}"), "status": "completed" });
        std::fs::write(d.path().join(format!("job-{id}.json")), other.to_string()).unwrap();
    }
    // Listed after the others.
    in_run(
        d.path(),
        (7, '5'),
        900,
        "Rust",
        1,
        "Set up job",
        &["##[error]boom"],
    );
    let job = crate::reruns::Job {
        item: 1,
        url: "https://github.com/a/b/actions/runs/7/job/900".into(),
        check: "CI / Rust".into(),
        sha: sha('5'),
        reason: String::new(),
        unsure: false,
    };
    let look = || crate::github::GH.scope(gh.clone(), crate::reruns::look("a/b", &job));
    assert_eq!(look().await, Ok(crate::reruns::Look::Ready));
    assert_eq!(calls(d.path(), "actions/runs/7/jobs"), 2);
    std::fs::write(d.path().join("rerun-900"), "").unwrap();
    assert_eq!(look().await, Ok(crate::reruns::Look::Already));
    std::fs::remove_file(d.path().join("rerun-900")).unwrap();
    let busy = json!({ "id": 719, "run_id": 7, "name": "Shard 719", "status": "in_progress" });
    std::fs::write(d.path().join("job-719.json"), busy.to_string()).unwrap();
    assert_eq!(look().await, Ok(crate::reruns::Look::Running));
    // A list that ends before it has all it said it holds is no answer.
    std::fs::write(d.path().join("more-jobs"), "1").unwrap();
    let before = calls(d.path(), "actions/runs/7/jobs");
    assert_eq!(
        look().await,
        Err("GitHub's list of the run's jobs ended early.".into())
    );
    assert_eq!(calls(d.path(), "actions/runs/7/jobs") - before, 3);
}

/// What the fake GitHub says is at the top of `a/b` at `at`, with these
/// workflow files and `package.json`.
fn files(dir: &Path, at: char, workflows: &[(&str, &str)], top: &[&str], package: Option<&str>) {
    let entries: Vec<Value> = workflows
        .iter()
        .map(|(name, text)| json!({ "name": name, "type": "blob", "object": { "text": text, "isTruncated": false } }))
        .collect();
    let top: Vec<Value> = top
        .iter()
        .map(|n| json!({ "name": n, "type": "blob" }))
        .collect();
    let v = json!({ "data": { "repository": {
        "workflows": if entries.is_empty() { Value::Null } else { json!({ "entries": entries }) },
        "root": { "entries": top },
        "package": package.map(|text| json!({ "text": text, "isTruncated": false })),
    } } });
    std::fs::write(dir.join(format!("setup-{}.json", sha(at))), v.to_string()).unwrap();
}

/// Says the fake GitHub's files at `at` are in a private repository.
fn private_at(dir: &Path, at: char) {
    let file = dir.join(format!("setup-{}.json", sha(at)));
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    v["data"]["repository"]["isPrivate"] = json!(true);
    v["data"]["repository"]["visibility"] = json!("PRIVATE");
    std::fs::write(file, v.to_string()).unwrap();
}

/// The issue's example, through the fake GitHub: a private repository's
/// workflow with an unpinned action, a `pull_request_target` job that
/// checks out the pull request, and an issue title in a script gets an
/// issue with three items, once GitHub says just before that it's still
/// private; the commit that fixes them closes it once Orbit reads it.
#[cfg(unix)]
#[tokio::test]
async fn risky_workflow_settings_go_to_a_private_repositorys_agent_until_fixed() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.rules.open_issues = "do_it".into())
        .await;
    let risky = "on: [pull_request_target, issues]\npermissions: {}\njobs:\n  build:\n    runs-on: x\n    steps:\n      - uses: other/tool@v1\n      - uses: actions/checkout@v5\n        with:\n          ref: ${{ github.event.pull_request.head.sha }}\n      - run: echo \"${{ github.event.issue.title }}\"\n";
    answer(d.path(), '1', &[], None);
    files(d.path(), '1', &[("ci.yml", risky)], &[], None);
    private_at(d.path(), '1');
    // GitHub says it's public when Orbit asks just before writing.
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let p = s
        .problems
        .iter()
        .find(|p| p.kind == crate::problems::ProblemKind::WorkflowRisks)
        .unwrap();
    assert_eq!(p.risks.len(), 3);
    assert!(!p.private, "what GitHub said last counts");
    assert!(writes(d.path()).is_empty());
    assert!(std::fs::read_to_string(d.path().join("calls"))
        .unwrap()
        .contains("GET repos/a/b\n"));

    // Private, at the next commit: the issue goes.
    std::fs::write(
        d.path().join("repo.json"),
        json!({ "full_name": "a/b", "private": true, "visibility": "private" }).to_string(),
    )
    .unwrap();
    answer(d.path(), '2', &[], None);
    files(d.path(), '2', &[("ci.yml", risky)], &[], None);
    private_at(d.path(), '2');
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
    let g = github(d.path());
    let issue = &g["issues"][0];
    assert_eq!(issue["title"], "Fix the risky workflow settings");
    let body = issue["body"].as_str().unwrap();
    for item in [
        "- **I1** · checks out the pull request's code",
        "- **I2** · `github.event.issue.title` goes straight into a script",
        "- **I3** · `other/tool@v1` isn't pinned",
    ] {
        assert!(body.contains(item), "{body}");
    }
    let number = issue["number"].as_u64().unwrap();
    assert!(e
        .snapshot()
        .await
        .buddy_feed
        .notes
        .last()
        .unwrap()
        .text
        .starts_with(&format!(
        "Orbit opened issue #{number} to tell the agent on a/b about 3 risky workflow settings."
    )));

    // The commit that fixes them closes it, as completed.
    let fixed = "on: [pull_request, issues]\npermissions: {}\njobs:\n  build:\n    runs-on: x\n    steps:\n      - uses: other/tool@0123456789abcdef0123456789abcdef01234567 # v1\n      - uses: actions/checkout@v5\n      - run: echo \"$TITLE\"\n        env:\n          TITLE: ${{ github.event.issue.title }}\n";
    answer(d.path(), '3', &[], None);
    files(d.path(), '3', &[("ci.yml", fixed)], &[], None);
    private_at(d.path(), '3');
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"][0]["state"], "closed");
    assert_eq!(g["issues"][0]["state_reason"], "completed");
    let note = g["comments"].as_array().unwrap().last().unwrap()["body"].clone();
    assert!(note.as_str().unwrap().starts_with(
        "Orbit found no risky settings in the workflows on `main` at `3333333`, so it is closing this issue."
    ));
    let body = g["issues"][0]["body"].as_str().unwrap();
    assert!(
        body.contains("- **I3** · `other/tool@v1` isn't pinned, in ci.yml · fixed at `3333333`"),
        "{body}"
    );
    let s = e.snapshot().await;
    assert!(s.problems.iter().all(|p| !p.is_open()));
    let sent = writes(d.path()).len();
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), sent);
}

/// Gives the fake GitHub's files at `at` these version files, by their
/// names in `workflows::query`.
fn versions_at(dir: &Path, at: char, files: &[(&str, &str)]) {
    let file = dir.join(format!("setup-{}.json", sha(at)));
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    for (name, text) in files {
        v["data"]["repository"][*name] = json!({ "text": text, "isTruncated": false });
    }
    std::fs::write(file, v.to_string()).unwrap();
}

/// A Tauri app whose release was bumped everywhere but its Rust side gets
/// an issue for its agent, public or not, with what to change in each
/// file; the commit that makes them agree closes it.
#[cfg(unix)]
#[tokio::test]
async fn version_files_that_disagree_go_to_the_agent_until_they_agree() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.rules.open_issues = "do_it".into())
        .await;
    let ci = [("ci.yml", "on: pull_request\n")];
    let cargo = |v: &str| format!("[package]\nname = \"app\"\nversion = \"{v}\"\n");
    let lock = |v: &str| format!("version = 4\n\n[[package]]\nname = \"app\"\nversion = \"{v}\"\n");
    let tauri = |v: &str| format!(r#"{{ "version": "{v}" }}"#);
    let package = |v: &str| format!(r#"{{ "name": "app", "version": "{v}" }}"#);
    answer(d.path(), '1', &[], None);
    files(d.path(), '1', &ci, &[], None);
    versions_at(
        d.path(),
        '1',
        &[
            ("package", &package("0.3.0")),
            ("tauri", &tauri("0.3.0")),
            ("tauriCargo", &cargo("0.2.0")),
            ("tauriLock", &lock("0.2.0")),
        ],
    );
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    assert_eq!(s.setups[0].versions.len(), 4);
    let p = s
        .problems
        .iter()
        .find(|p| p.kind == crate::problems::ProblemKind::Versions)
        .unwrap();
    assert_eq!(p.mismatches.len(), 2);
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
    let g = github(d.path());
    let issue = &g["issues"][0];
    assert_eq!(issue["title"], "Make the version files agree");
    let body = issue["body"].as_str().unwrap();
    for line in [
        "## Orbit · Versions\n\nOn `main` at `1111111`, 2 version files say another version than the files they should agree with.",
        &format!("- **I1** · `src-tauri/Cargo.toml` says `0.2.0`, but `package.json` and `src-tauri/tauri.conf.json` say `0.3.0` · [file](https://github.com/a/b/blob/{}/src-tauri/Cargo.toml) · change it to `0.3.0`", sha('1')),
        "- **I2** · `src-tauri/Cargo.lock` records `0.2.0` for `src-tauri/Cargo.toml`, but `package.json` and `src-tauri/tauri.conf.json` say `0.3.0`",
        "· once `src-tauri/Cargo.toml` says `0.3.0`, run `cargo update --workspace` in `src-tauri` to update it",
        r#""kind":"versions""#,
        r#""kind":"version","mismatch":1"#,
        r#""mismatch":2,"of":"src-tauri/Cargo.toml","path":"src-tauri/Cargo.lock","says":"0.2.0""#,
    ] {
        assert!(body.contains(line), "{line}\n\n{body}");
    }
    let number = issue["number"].as_u64().unwrap();
    let notes = e.snapshot().await.buddy_feed.notes;
    assert!(notes.last().unwrap().text.starts_with(&format!(
        "Orbit opened issue #{number} to tell the agent on a/b that 2 version files disagree."
    )));

    // The commit that makes them agree closes it, as completed.
    answer(d.path(), '2', &[], None);
    files(d.path(), '2', &ci, &[], None);
    versions_at(
        d.path(),
        '2',
        &[
            ("package", &package("0.3.0")),
            ("tauri", &tauri("0.3.0")),
            ("tauriCargo", &cargo("0.3.0")),
            ("tauriLock", &lock("0.3.0")),
        ],
    );
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"][0]["state"], "closed");
    assert_eq!(g["issues"][0]["state_reason"], "completed");
    let note = g["comments"].as_array().unwrap().last().unwrap()["body"].clone();
    assert!(note.as_str().unwrap().starts_with(
        "The version files on `main` agree at `2222222`, so Orbit is closing this issue."
    ));
    let body = g["issues"][0]["body"].as_str().unwrap();
    for line in [
        "The version files on `main` agree at `2222222`. Nothing is left to do here.",
        "- **I1** · src-tauri/Cargo.toml says 0.2.0 · agrees at `2222222`",
    ] {
        assert!(body.contains(line), "{line}\n\n{body}");
    }
    let s = e.snapshot().await;
    assert!(s.problems.iter().all(|p| !p.is_open()));
    assert!(s
        .buddy_feed
        .notes
        .last()
        .unwrap()
        .text
        .starts_with(&format!(
            "Orbit closed issue #{number}: a/b's version files agree now."
        )));
}

/// With actionlint and zizmor installed, what they find beyond Orbit's own
/// checks joins the same problem and issue, from a folder Orbit removes.
#[cfg(unix)]
#[tokio::test]
async fn what_actionlint_and_zizmor_find_joins_orbits_own() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.use_lint_tools(crate::lint::tests::fake("ok", &d.path().join("lint-log")));
    e.update(|s| s.settings.buddy.rules.open_issues = "do_it".into())
        .await;
    std::fs::write(
        d.path().join("repo.json"),
        json!({ "full_name": "a/b", "private": true, "visibility": "private" }).to_string(),
    )
    .unwrap();
    let ci = "on: [issues]\npermissions: {}\njobs:\n  build:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v5\n      - run: echo \"${{ github.event.issue.title }}\"\n";
    answer(d.path(), '1', &[], None);
    files(d.path(), '1', &[("ci.yml", ci)], &[], None);
    private_at(d.path(), '1');
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    let tools: Vec<_> = s.setups[0].tools.iter().map(|t| t.tool).collect();
    use crate::risks::RiskKind;
    assert_eq!(tools, [RiskKind::Actionlint, RiskKind::Zizmor]);
    let p = s
        .problems
        .iter()
        .find(|p| p.kind == crate::problems::ProblemKind::WorkflowRisks)
        .unwrap();
    let kinds: Vec<_> = p.risks.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [RiskKind::Injection, RiskKind::Zizmor]);
    let g = github(d.path());
    let body = g["issues"][0]["body"].as_str().unwrap();
    assert!(
        body.contains("- **I2** · zizmor found: credential persistence through GitHub Actions artifacts (`artipacked`) (medium) · `.github/workflows/ci.yml` line 7, job `build`"),
        "{body}"
    );
    assert!(body.contains("https://docs.zizmor.sh/audits/#artipacked"));
    assert_eq!(std::fs::read_dir(e.root.join("lint")).unwrap().count(), 0);
}

/// A repository with no CI on pull requests: once Orbit is sure, Bit
/// offers an issue with a workflow to start from, opens it when you say
/// yes, and closes it as done once a workflow there runs on pull
/// requests. Each commit's files are read once.
#[cfg(unix)]
#[tokio::test]
async fn bit_proposes_ci_for_a_repository_with_none_on_pull_requests() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    answer(d.path(), '1', &[], None);
    let package = r#"{"scripts":{"lint":"eslint .","test":"vitest run"}}"#;
    files(
        d.path(),
        '1',
        &[("release.yml", "on:\n  push:\n    tags: ['v*']\n")],
        &["package.json", "package-lock.json", "README.md"],
        Some(package),
    );
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    assert_eq!(s.setups.len(), 1);
    assert_eq!(s.setups[0].sha, sha('1'));
    assert!(s.problems.is_empty(), "not sure yet");
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "GET setup/"), 1, "read once a commit");

    // Long enough after reading for checks from elsewhere to show up.
    e.update(|s| {
        let ago = chrono::Utc::now() - crate::workflows::QUIET;
        s.setups[0].read_at = crate::problems::stamp(ago);
    })
    .await;
    watch(&e, &gh).await;
    let s = e.snapshot().await;
    assert_eq!(s.problems.len(), 1);
    assert_eq!(s.problems[0].kind, crate::problems::ProblemKind::NoCi);
    assert!(writes(d.path()).is_empty(), "Bit asks first");
    let offered: Vec<&str> = s
        .buddy_feed
        .notes
        .iter()
        .map(|n| n.text.as_str())
        .filter(|t| t.contains("Propose CI"))
        .collect();
    assert_eq!(offered.len(), 1);
    assert!(
        offered[0].starts_with("No CI runs on a/b's pull requests. Tap Propose CI on Operations")
    );
    assert!(
        e.shown_snapshot().await.attention.is_empty(),
        "not a Needs you item"
    );
    watch(&e, &gh).await;
    let notes = e.snapshot().await.buddy_feed.notes;
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.text.contains("Propose CI"))
            .count(),
        1,
        "once"
    );

    // You tap Propose CI.
    let key = e.snapshot().await.ops[0].key.clone();
    e.allow_telling(&key).await.unwrap();
    crate::github::GH.scope(gh.clone(), e.tell_agents()).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
    let g = github(d.path());
    let issue = &g["issues"][0];
    assert_eq!(issue["title"], "Add CI that runs on pull requests");
    assert_eq!(issue["labels"][0]["name"], "orbit");
    let body = issue["body"].as_str().unwrap();
    assert!(body.contains(
        "None of the workflows on `main` runs on pull requests (`.github/workflows/release.yml`), so nothing checks a change before it merges."
    ), "{body}");
    assert!(
        body.contains("Orbit built it from `package.json` and `package-lock.json` at `1111111`")
    );
    assert!(
        body.contains("      - run: npm ci\n      - run: npm run lint\n      - run: npm test\n```"),
        "{body}"
    );
    assert!(body.contains("- **I1** · add a workflow that runs on pull requests"));
    let number = issue["number"].as_u64().unwrap();
    let notes = e.snapshot().await.buddy_feed.notes;
    assert!(notes.last().unwrap().text.starts_with(&format!(
        "Orbit opened issue #{number} for the agent on a/b, with a workflow to start from"
    )));

    // The agent adds a workflow that runs on pull requests.
    answer(d.path(), '2', &[], None);
    files(
        d.path(),
        '2',
        &[
            ("release.yml", "on: release"),
            ("ci.yml", "on: [pull_request]"),
        ],
        &["package.json", "package-lock.json"],
        Some(package),
    );
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"][0]["state"], "closed");
    assert_eq!(g["issues"][0]["state_reason"], "completed");
    let note = g["comments"].as_array().unwrap().last().unwrap()["body"].clone();
    assert!(note.as_str().unwrap().starts_with(
        "A workflow on `main` runs on pull requests at `2222222`, so Orbit is closing this issue."
    ));
    let s = e.snapshot().await;
    assert!(s.problems[0].passed());
    assert!(s
        .buddy_feed
        .notes
        .last()
        .unwrap()
        .text
        .starts_with(&format!(
            "Orbit closed issue #{number}: a workflow runs on a/b's pull requests now."
        )));
    assert_eq!(calls(d.path(), "GET setup/"), 2);
    // Nothing more once it's done.
    let sent = writes(d.path()).len();
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), sent);
}

/// Checks from another service, and workflows Orbit can't read, mean CI
/// runs: nothing is proposed.
#[cfg(unix)]
#[tokio::test]
async fn nothing_is_proposed_where_something_runs_checks() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = connected(d.path()).await;
    e.update(|s| s.settings.buddy.rules.propose_ci = "do_it".into())
        .await;
    let quiet = || async {
        e.update(|s| {
            let ago = chrono::Utc::now() - crate::workflows::QUIET;
            for setup in &mut s.setups {
                setup.read_at = crate::problems::stamp(ago);
            }
        })
        .await;
    };
    // A workflow Orbit can't read.
    answer(d.path(), '1', &[], None);
    files(
        d.path(),
        '1',
        &[("odd.yml", "on: {pull_request: {}}")],
        &[],
        None,
    );
    watch(&e, &gh).await;
    quiet().await;
    watch(&e, &gh).await;
    assert!(e.snapshot().await.problems.is_empty());
    // No workflows, but a status from elsewhere on the default branch.
    answer(d.path(), '2', &[("ci/jenkins", "SUCCESS")], None);
    files(d.path(), '2', &[], &["Cargo.toml"], None);
    watch(&e, &gh).await;
    quiet().await;
    watch(&e, &gh).await;
    assert!(e.snapshot().await.problems.is_empty());
    // Orbit remembers it saw checks, so a newer commit without them
    // doesn't open one either.
    answer(d.path(), '3', &[], None);
    files(d.path(), '3', &[], &["Cargo.toml"], None);
    watch(&e, &gh).await;
    quiet().await;
    watch(&e, &gh).await;
    assert!(e.snapshot().await.problems.is_empty());
    assert!(writes(d.path()).is_empty());
    // Without either, as after Watch CI went off, the issue is opened at
    // once under Do it.
    e.update(|s| s.settings.watch_ci = false).await;
    watch(&e, &gh).await;
    assert!(e.snapshot().await.setups.is_empty());
    e.update(|s| s.settings.watch_ci = true).await;
    watch(&e, &gh).await;
    quiet().await;
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
    let body = github(d.path())["issues"][0]["body"].clone();
    assert!(body
        .as_str()
        .unwrap()
        .contains("      - run: cargo test\n```"));
}

/// `a/b` with its only version tag, v0.3.0, at commit 3, and these runs
/// for it there: an ID, a workflow and how it stands.
fn released(dir: &Path, runs: &[(u64, &str, &str, Option<&str>)]) {
    let tags = json!({ "data": { "repository": {
        "viewerPermission": "ADMIN",
        "defaultBranchRef": { "name": "main", "target": { "oid": sha('1') } },
        "refs": { "totalCount": 1, "nodes": [
            { "name": "v0.3.0", "target": { "__typename": "Commit", "oid": sha('3') } }
        ] },
    } } });
    std::fs::write(dir.join("tags.json"), tags.to_string()).unwrap();
    let runs: Vec<Value> = runs
        .iter()
        .map(|(id, name, status, conclusion)| {
            json!({
                "id": id,
                "workflow_id": id,
                "name": name,
                "head_branch": "v0.3.0",
                "head_sha": sha('3'),
                "event": "push",
                "status": status,
                "conclusion": conclusion,
                "created_at": "2026-11-02T12:00:00Z",
                "html_url": format!("https://github.com/a/b/actions/runs/{id}"),
            })
        })
        .collect();
    std::fs::write(dir.join("queued.json"), json!(runs).to_string()).unwrap();
}
/// Makes each release build due to be read again.
async fn builds_read_long_ago(e: &Engine) {
    e.update(|s| {
        for b in &mut s.builds {
            b.read_at = "2020-01-01T00:00:00.000Z".into();
        }
    })
    .await;
}

/// The issue's example for release builds, through the fake GitHub: a
/// failing build on a version tag needs you once, with why it failed from
/// its log; the agent hears of it in an issue, a job that failed by bad
/// luck is rerun once as the rules say, and once each run passes the
/// problem and the issue close.
#[cfg(unix)]
#[tokio::test]
async fn a_failing_release_build_needs_you_once_and_bad_luck_is_rerun() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "SUCCESS")], None);
    released(
        d.path(),
        &[
            (7, "Release", "completed", Some("failure")),
            (8, "Docs", "completed", Some("success")),
        ],
    );
    in_run(
        d.path(),
        (7, '3'),
        70,
        "build (macos)",
        1,
        "Run actions/checkout@v4",
        &[
            "fatal: unable to access 'https://github.com/a/b/': Could not resolve host: github.com",
            "##[error]Process completed with exit code 128.",
        ],
    );
    let url = "https://github.com/a/b/actions/runs/7/job/70";
    let file = d.path().join("job-70.json");
    let mut job: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    job["html_url"] = json!(url);
    std::fs::write(&file, job.to_string()).unwrap();
    let e = connected(d.path()).await;
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    let failing: Vec<_> = shown
        .attention
        .iter()
        .filter(|a| a.kind == AttentionKind::CiFailing)
        .collect();
    assert_eq!(failing.len(), 1);
    assert_eq!(
        failing[0].reason,
        "The release build for v0.3.0 in a/b is failing at 3333333: Release / build (macos)."
    );
    let b = &shown.builds[0];
    assert_eq!(
        (b.tag.as_str(), b.ci.passing, b.runs.len()),
        ("v0.3.0", 1, 2)
    );
    assert!(writes(d.path()).is_empty());

    // The next pass reads why it failed, reruns it as bad luck, and tells
    // the agent, once.
    watch(&e, &gh).await;
    assert_eq!(
        writes(d.path()),
        [
            "POST repos/a/b/actions/jobs/70/rerun",
            "POST repos/a/b/issues"
        ]
    );
    let issue = &github(d.path())["issues"][0];
    assert_eq!(issue["title"], "The release build for v0.3.0 is failing");
    let body = issue["body"].as_str().unwrap();
    assert!(body.starts_with(
        "## Orbit · Release build\n\n1 check fails in the release build for `v0.3.0` at `3333333`."
    ));
    assert!(body.contains(&format!(
        "- **I1** · `Release / build (macos)` · failing at `3333333` · [details]({url})\n  Orbit reran this job, as it looks like bad luck"
    )));
    assert!(body.contains("\"kind\":\"release_build\""));
    watch(&e, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    assert_eq!(
        e.shown_snapshot()
            .await
            .attention
            .iter()
            .filter(|a| a.kind == AttentionKind::CiFailing)
            .count(),
        1
    );

    // The rerun passes: the problem closes, and so does the issue.
    released(
        d.path(),
        &[
            (7, "Release", "completed", Some("success")),
            (8, "Docs", "completed", Some("success")),
        ],
    );
    builds_read_long_ago(&e).await;
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    assert!(shown
        .attention
        .iter()
        .all(|a| a.kind != AttentionKind::CiFailing));
    assert!(shown.problems.iter().all(|p| !p.is_open()));
    watch(&e, &gh).await;
    let g = github(d.path());
    assert_eq!(g["issues"][0]["state"], "closed");
    let notes = g["comments"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0]["body"].as_str().unwrap().starts_with(
        "Every run of the release build for `v0.3.0` passed at `3333333`, so Orbit is closing this issue."
    ));
    // Once it passed, it is read again only every half hour.
    let asked = calls(d.path(), "GET repos/a/b/actions/runs");
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "GET repos/a/b/actions/runs"), asked);
    // Then a run the published release started fails at the same tag: it
    // is failing again.
    released(
        d.path(),
        &[
            (7, "Release", "completed", Some("success")),
            (8, "Docs", "completed", Some("success")),
            (9, "Publish", "completed", Some("failure")),
        ],
    );
    builds_read_long_ago(&e).await;
    watch(&e, &gh).await;
    assert!(calls(d.path(), "GET repos/a/b/actions/runs") > asked);
    let shown = e.shown_snapshot().await;
    let open: Vec<_> = shown.problems.iter().filter(|p| p.is_open()).collect();
    assert_eq!(open.len(), 1);
    assert_eq!(
        crate::problems::build_text(open[0]),
        "The release build for v0.3.0 in a/b is failing at 3333333: Publish."
    );
}

/// A failing release build, read once, with a job that looks like bad
/// luck: what Orbit knows after its first pass.
#[cfg(unix)]
async fn failed_by_bad_luck(dir: &Path) -> std::sync::Arc<Engine> {
    answer(dir, '1', &[("Rust", "SUCCESS")], None);
    released(dir, &[(7, "Release", "completed", Some("failure"))]);
    in_run(
        dir,
        (7, '3'),
        70,
        "build (macos)",
        1,
        "Run actions/checkout@v4",
        &[
            "fatal: unable to access 'https://github.com/a/b/': Could not resolve host: github.com",
            "##[error]Process completed with exit code 128.",
        ],
    );
    let file = dir.join("job-70.json");
    let mut job: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    job["html_url"] = json!("https://github.com/a/b/actions/runs/7/job/70");
    std::fs::write(&file, job.to_string()).unwrap();
    connected(dir).await
}

/// A job of a tag's build is rerun only while that tag is still the one
/// Orbit watches at that commit: once Orbit knows of a newer tag, such as
/// one it just pushed, or the tag moved, the old build's jobs are left.
#[cfg(unix)]
#[tokio::test]
async fn a_release_job_isnt_rerun_once_a_newer_tag_or_a_move_is_known() {
    for change in ["newer", "moved"] {
        let d = tempfile::tempdir().unwrap();
        let gh = fake_gh(d.path());
        let e = failed_by_bad_luck(d.path()).await;
        watch(&e, &gh).await;
        assert_eq!(e.shown_snapshot().await.builds[0].tag, "v0.3.0");
        e.update(|s| {
            let r = &mut s.releases[0];
            match change {
                // As pushing v0.4.0 keeps it.
                "newer" => crate::tagging::seen(r, "v0.4.0", &sha('4')),
                _ => r.tags[0].sha = sha('4'),
            }
        })
        .await;
        watch(&e, &gh).await;
        watch(&e, &gh).await;
        assert_eq!(calls(d.path(), "/rerun"), 0, "{change}");
    }
    // Unchanged, it is rerun.
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = failed_by_bad_luck(d.path()).await;
    watch(&e, &gh).await;
    watch(&e, &gh).await;
    assert_eq!(calls(d.path(), "/rerun"), 1);
}

/// A version tag deleted after its build failed: once a full read of the
/// tags shows it gone, its problem closes and Orbit forgets the build, so
/// nothing says it still fails.
#[cfg(unix)]
#[tokio::test]
async fn a_deleted_tag_closes_its_build_and_forgets_it() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let e = failed_by_bad_luck(d.path()).await;
    watch(&e, &gh).await;
    assert_eq!(e.shown_snapshot().await.builds.len(), 1);
    let tags = json!({ "data": { "repository": {
        "viewerPermission": "ADMIN",
        "defaultBranchRef": { "name": "main", "target": { "oid": sha('1') } },
        "refs": { "totalCount": 0, "nodes": [] },
    } } });
    std::fs::write(d.path().join("tags.json"), tags.to_string()).unwrap();
    e.update(|s| s.releases[0].read_at = "2020-01-01T00:00:00.000Z".into())
        .await;
    watch(&e, &gh).await;
    let shown = e.shown_snapshot().await;
    assert!(shown.builds.is_empty());
    let p = shown
        .problems
        .iter()
        .find(|p| p.kind == crate::problems::ProblemKind::ReleaseBuild)
        .unwrap();
    assert_eq!(p.closed_reason.as_deref(), Some("The tag v0.3.0 is gone."));
    assert!(shown
        .attention
        .iter()
        .all(|a| a.kind != AttentionKind::CiFailing));
}
