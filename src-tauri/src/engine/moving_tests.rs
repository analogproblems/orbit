//! Moving an Orbit into an always-on Orbit, end to end with the fake
//! GitHubs of `ops_tests` and `setup_tests` and real Git folders.
use super::{
    ops_tests::{answer, connected, fake_gh, github, pr, signed_by, watch, writes},
    worktrees::git,
    Engine,
};
use crate::{moving::Moved, types::*};
use std::{path::Path, sync::Arc, time::Duration};

/// A clone of `repo` at `folder`: a Git working tree whose origin is it.
async fn clone_of(folder: &Path, repo: &str) -> String {
    std::fs::create_dir_all(folder).unwrap();
    git(folder, &["init", "-q"]).await.unwrap();
    let origin = format!("https://github.com/{repo}.git");
    git(folder, &["remote", "add", "origin", &origin])
        .await
        .unwrap();
    std::fs::canonicalize(folder)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}
/// An always-on Orbit with nothing of its own yet, signed in to GitHub.
async fn always_on(dir: &Path) -> Arc<Engine> {
    let e = Engine::new(dir.join("state"), |_| {}).unwrap();
    e.update(|s| s.github_user = Some("me".into())).await;
    e
}
/// `e` packed to move, as the desktop app does once it stopped.
async fn packed(e: &Engine) -> Moved {
    e.shutdown().await;
    e.fence().await.unwrap();
    e.pack().await.unwrap()
}
/// The Orbit `e` started again, as the app does after it moved.
fn restarted(e: Arc<Engine>) -> Arc<Engine> {
    let root = e.root.clone();
    assert_eq!(Arc::strong_count(&e), 1);
    drop(e);
    Engine::new(root, |_| {}).unwrap()
}
/// Waits until the fake GitHub holds a request, as `hold` asks.
async fn held(dir: &Path) {
    for _ in 0..500 {
        if dir.join("held").exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("GitHub was never asked");
}
fn another_orbit(s: &Snapshot) -> bool {
    s.attention
        .iter()
        .any(|a| a.kind == AttentionKind::AnotherOrbit)
}

/// The desktop app's Orbit posted on a pull request, then moved to an
/// always-on Orbit. That one keeps the comment up to date as its own
/// rather than step back from it or post another, and the first, if you
/// ran it again, steps back.
#[cfg(unix)]
#[tokio::test]
async fn a_moved_orbit_picks_up_where_the_first_left_off() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    let first = connected(&d.path().join("first")).await;
    let path = clone_of(&d.path().join("clone"), "a/b").await;
    first
        .update(|s| {
            s.settings.automatic = true;
            s.settings.repositories[0].path = path.clone();
            s.prs = vec![pr('5')];
        })
        .await;
    let failing = |checks: &[(&str, &str)]| {
        answer(d.path(), '1', &[("Rust", "SUCCESS")], Some(('5', checks)))
    };
    failing(&[("Rust", "FAILURE")]);
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues/12/comments"]);
    let first_name = first.snapshot().await.instance.name;
    assert_eq!(signed_by(d.path()), first_name);

    let second = always_on(&d.path().join("second")).await;
    let second_name = second.snapshot().await.instance.name;
    second.take_in(packed(&first).await).await.unwrap();
    let (s, was) = (second.snapshot().await, first.snapshot().await);
    assert_eq!(s.settings.repositories[0].path, path);
    assert!(s.settings.repositories[0].automatic);
    assert!(s.settings.automatic);
    let ids = |prs: &[PullRequest]| prs.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&s.prs), ids(&was.prs));
    assert_eq!(s.ops, was.ops);
    assert_eq!(s.journal, was.journal);
    assert_eq!(s.instance.name, second_name);
    assert!(s.instance.is(&first_name, &now()));
    assert!(s.posting.is_empty());

    failing(&[("Rust", "FAILURE"), ("Lint", "FAILURE")]);
    watch(&second, &gh).await;
    let sent = writes(d.path());
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(sent[1].starts_with("PATCH repos/a/b/issues/comments/"));
    assert_eq!(signed_by(d.path()), second_name);
    assert_eq!(github(d.path())["comments"].as_array().unwrap().len(), 1);
    assert!(!another_orbit(&second.shown_snapshot().await));

    failing(&[("Rust", "SUCCESS"), ("Lint", "FAILURE")]);
    let first = restarted(first);
    watch(&first, &gh).await;
    assert_eq!(writes(d.path()).len(), 2);
    assert!(another_orbit(&first.shown_snapshot().await));
}

/// What an Orbit is doing on GitHub as it moves finishes before it's
/// packed, and nothing new starts after: here, opening an issue for a
/// failing main branch, while GitHub takes its time to say whether there
/// is one. The always-on Orbit then doesn't open another.
#[cfg(unix)]
#[tokio::test]
async fn an_orbit_finishes_what_it_was_doing_on_github_before_it_moves() {
    let d = tempfile::tempdir().unwrap();
    let gh = fake_gh(d.path());
    answer(d.path(), '1', &[("Rust", "FAILURE")], None);
    let first = connected(&d.path().join("first")).await;
    let path = clone_of(&d.path().join("clone"), "a/b").await;
    first
        .update(|s| s.settings.repositories[0].path = path.clone())
        .await;
    std::fs::write(d.path().join("hold"), "repos/a/b/issues").unwrap();
    let watching = tokio::spawn(crate::github::GH.scope(gh.clone(), {
        let e = first.clone();
        async move { e.watch_ci().await }
    }));
    held(d.path()).await;
    first.shutdown().await;
    let fencing = tokio::spawn({
        let e = first.clone();
        async move { e.fence().await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!fencing.is_finished(), "waits for GitHub");
    std::fs::remove_file(d.path().join("hold")).unwrap();
    watching.await.unwrap();
    fencing.await.unwrap().unwrap();
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
    let moved = first.pack().await.unwrap();
    let calls = || std::fs::read_to_string(d.path().join("calls")).unwrap();
    let asked = calls();
    watch(&first, &gh).await;
    assert_eq!(calls(), asked, "nothing new starts");

    let second = always_on(&d.path().join("second")).await;
    second.take_in(moved).await.unwrap();
    watch(&second, &gh).await;
    assert_eq!(writes(d.path()), ["POST repos/a/b/issues"]);
    assert_eq!(github(d.path())["issues"].as_array().unwrap().len(), 1);
}

/// Two Orbits moving into one always-on Orbit at once: one moves, and the
/// other is refused before any of its files are written.
#[cfg(unix)]
#[tokio::test]
async fn one_orbit_moves_in_when_two_try_at_once() {
    let d = tempfile::tempdir().unwrap();
    let path = clone_of(&d.path().join("clone"), "a/b").await;
    let first = Engine::new(d.path().join("first"), |_| {}).unwrap();
    first
        .update(|s| s.settings.repositories = vec![Repository::new("a/b", &path)])
        .await;
    let stored = "ledger/a/b/12.json";
    let with = |mut moved: Moved, text: &str| {
        moved.files = vec![crate::moving::MovedFile {
            path: stored.into(),
            text: text.into(),
        }];
        moved
    };
    let one = with(packed(&first).await, "one");
    let other = with(first.pack().await.unwrap(), "other");

    let second = always_on(&d.path().join("second")).await;
    let (a, b) = tokio::join!(second.take_in(one), second.take_in(other));
    assert_eq!(a, Ok(()));
    let error = b.unwrap_err();
    assert!(error.contains("already has repositories"), "{error}");
    let text = std::fs::read_to_string(d.path().join("second/state").join(stored)).unwrap();
    assert_eq!(text, "one");
}

/// What belongs to a computer stays with it: the always-on Orbit keeps its
/// own sign-ins, API keys and reviewers, uses its own copy of a
/// repository it has no clone of, and forgets the moved Orbit's
/// worktrees. A review running when it moved ends as after a restart. The
/// history, write-ups and scan reports come along.
#[cfg(unix)]
#[tokio::test]
async fn what_belongs_to_each_computer_stays_there() {
    let d = tempfile::tempdir().unwrap();
    let gh = super::setup_tests::fake_gh(d.path());
    let first = Engine::new(d.path().join("first"), |_| {}).unwrap();
    let here = clone_of(&d.path().join("here"), "a/b").await;
    let pr = pr('5');
    first
        .update(|s| {
            s.github_user = Some("me".into());
            s.api_keys.openai.saved = true;
            s.settings.repositories = vec![
                Repository::new("a/b", &here),
                Repository::new("acme/api", "/nowhere/api"),
                Repository::new("acme/web", "/nowhere/web"),
            ];
            let mut running = Review::new(&pr, &s.settings, true);
            running.status = "reviewing".into();
            running.worktree = Some("/first/worktrees/1".into());
            let mut done = Review::new(&pr, &s.settings, true);
            done.status = "published".into();
            done.worktree = Some("/first/worktrees/2".into());
            s.reviews = vec![running, done];
            s.prs = vec![pr.clone()];
            s.scans = vec![SecurityScan {
                id: "scan-1".into(),
                repo: "a/b".into(),
                status: "completed".into(),
                has_report: true,
                scan_dir: Some("/first/scan-output/scan-1".into()),
                worktree: Some("/first/scan-worktrees/scan-1".into()),
                ..Default::default()
            }];
        })
        .await;
    let root = d.path().join("first");
    for (path, text) in [
        ("ledger/a/b/12.json", r#"{"stored":12}"#),
        ("ledger/a/b/writeups/12.json", r#"{"writeup":12}"#),
        ("scans/scan-1/report.md", "# What the scan found"),
    ] {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::write(root.join(path), text).unwrap();
    }

    let second = always_on(&d.path().join("second")).await;
    second
        .update(|s| {
            s.api_keys.anthropic.saved = true;
            s.antigravity.ready = true;
        })
        .await;
    let before = second.snapshot().await;
    // One folder from a Windows computer, which isn't a path here.
    let mut moved = packed(&first).await;
    moved.state["settings"]["repositories"][2]["path"] = r"C:\Users\me\web".into();
    crate::github::GH
        .scope(gh, second.take_in(moved))
        .await
        .unwrap();
    let s = second.snapshot().await;
    let repos = &s.settings.repositories;
    assert_eq!(repos[0].path, here);
    assert!(!repos[0].keep_updated);
    let copy = std::fs::canonicalize(d.path().join("second/state/clones/acme/api")).unwrap();
    assert_eq!(Path::new(&repos[1].path), copy);
    assert!(repos[1].keep_updated);
    let copy = std::fs::canonicalize(d.path().join("second/state/clones/acme/web")).unwrap();
    assert_eq!(Path::new(&repos[2].path), copy);
    assert_eq!(s.api_keys, before.api_keys);
    assert_eq!(s.antigravity, before.antigravity);
    assert_eq!(s.github_user.as_deref(), Some("me"));
    assert!(s.reviews.iter().all(|r| r.worktree.is_none()));
    assert_eq!(s.reviews[0].status, "cancelled");
    assert_eq!(s.reviews[1].status, "published");
    assert_eq!(s.scans[0].scan_dir, None);
    assert_eq!(s.scans[0].worktree, None);
    assert_eq!(
        second.scan_report("scan-1").await.unwrap(),
        "# What the scan found"
    );
    let moved = d.path().join("second/state");
    for path in ["ledger/a/b/12.json", "ledger/a/b/writeups/12.json"] {
        assert!(moved.join(path).is_file(), "{path}");
    }
    assert!(s.ledger.version > before.ledger.version);
    // Saved, so it is there after a restart.
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(moved.join("state.json")).unwrap()).unwrap();
    assert_eq!(saved.settings.repositories.len(), 3);
    assert_eq!(saved.instance.name, before.instance.name);
}

/// An always-on Orbit with repositories or reviews of its own takes in
/// nothing, and neither does one whose download of a repository fails.
#[cfg(unix)]
#[tokio::test]
async fn an_orbit_takes_in_another_only_when_it_has_nothing_of_its_own() {
    let d = tempfile::tempdir().unwrap();
    let gh = super::setup_tests::fake_gh(d.path());
    let first = Engine::new(d.path().join("first"), |_| {}).unwrap();
    first
        .update(|s| {
            s.settings.repositories = vec![Repository::new("acme/api", "/nowhere/api")];
        })
        .await;
    let moved = packed(&first).await;

    let busy = always_on(&d.path().join("busy")).await;
    let own = clone_of(&d.path().join("own"), "a/b").await;
    busy.update(|s| s.settings.repositories = vec![Repository::new("a/b", &own)])
        .await;
    let error = busy.take_in(moved).await.unwrap_err();
    assert!(error.contains("already has repositories"), "{error}");
    assert_eq!(busy.snapshot().await.settings.repositories[0].name, "a/b");

    let second = always_on(&d.path().join("second")).await;
    std::fs::write(d.path().join("clone-fails"), "").unwrap();
    let error = crate::github::GH
        .scope(gh.clone(), second.take_in(first.pack().await.unwrap()))
        .await
        .unwrap_err();
    assert!(error.contains("couldn't download acme/api"), "{error}");
    assert!(second.snapshot().await.settings.repositories.is_empty());
    // Tried again once it can download: it moves.
    std::fs::remove_file(d.path().join("clone-fails")).unwrap();
    crate::github::GH
        .scope(gh, second.take_in(first.pack().await.unwrap()))
        .await
        .unwrap();
    assert_eq!(second.snapshot().await.settings.repositories.len(), 1);
}

/// A scan can only finish where it started, so an Orbit running one
/// doesn't move.
#[tokio::test]
async fn an_orbit_running_a_scan_does_not_move() {
    let d = tempfile::tempdir().unwrap();
    let first = Engine::new(d.path().join("first"), |_| {}).unwrap();
    assert_eq!(first.cannot_move().await, None);
    first
        .update(|s| {
            s.scans = vec![SecurityScan {
                id: "scan-1".into(),
                status: "running".into(),
                ..Default::default()
            }];
        })
        .await;
    let why = first.cannot_move().await.unwrap();
    assert!(why.contains("scan is running"), "{why}");
    assert_eq!(first.pack().await.unwrap_err(), why);
}
