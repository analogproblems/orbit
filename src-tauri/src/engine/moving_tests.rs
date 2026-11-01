//! Moving an Orbit into an always-on Orbit, end to end with the fake
//! GitHubs of `ops_tests` and `setup_tests` and real Git folders.
use super::{
    ops_tests::{answer, connected, fake_gh, github, pr, signed_by, watch, writes},
    worktrees::git,
    Engine,
};
use crate::{moving::Moved, types::*};
use serde_json::json;
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
/// Serves `e` as an always-on Orbit does, returning its link.
async fn serve(e: &Arc<Engine>) -> crate::client::Link {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = "ab".repeat(32);
    let (_, stop) = tokio::sync::watch::channel(false);
    let app = crate::remote::router(
        e.clone(),
        crate::remote::Hub::default(),
        token.clone(),
        Arc::new(|_: &str| None),
        stop,
        crate::remote::SERVER_MARK,
    );
    tokio::spawn(async move { axum::serve(listener, app).await });
    crate::client::Link::parse(&format!("http://127.0.0.1:{port}/#token={token}")).unwrap()
}
/// A client for `link` that waits a fraction of a second for anything, as
/// one behind a proxy that gives up early would.
fn impatient(link: crate::client::Link) -> crate::client::Client {
    let short = Duration::from_millis(300);
    let limits = crate::client::Limits {
        silence: short,
        snapshot: short,
        action: short * 2,
        moving: short * 2,
    };
    crate::client::Client::with_limits(link, limits).unwrap()
}
/// Waits until `done` holds for `e`'s state.
async fn until(e: &Engine, done: impl Fn(&Snapshot) -> bool) {
    for _ in 0..250 {
        if done(&*e.state.lock().await) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("it never happened");
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
            s.notices_sent = 5;
            s.push_devices = vec![crate::push::PushDevice {
                id: "woken-by-first".into(),
                ..Default::default()
            }];
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
            s.notices_sent = 7;
            s.push_devices = vec![crate::push::PushDevice {
                id: "woken-by-second".into(),
                read: 1,
                told: 1,
                ..Default::default()
            }];
        })
        .await;
    let before = second.snapshot().await;
    // One folder from a Windows computer, which isn't a path here.
    let mut moved = packed(&first).await;
    assert_eq!(moved.state["pushDevices"].as_array().map(Vec::len), Some(0));
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
    // Devices are woken by the Orbit they signed up with, for what it sends
    // from now on, counted on from the higher count of the two.
    let devices: Vec<_> = s
        .push_devices
        .iter()
        .map(|d| (d.id.as_str(), d.read, d.told))
        .collect();
    assert_eq!(devices, [("woken-by-second", 7, 7)]);
    assert_eq!(s.notices_sent, 7);
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

/// A move still on its way in when the app stops waiting for the answer,
/// as when a proxy gives up on a long download, isn't taken for one that
/// failed, though the always-on Orbit still looks as it did: the app's
/// Orbit stays paused, and the move shows as made once it is.
#[tokio::test]
async fn a_move_still_on_its_way_in_is_never_taken_for_one_that_failed() {
    use crate::client::{move_to, pending_move, settle, Failed, Outcome};
    let d = tempfile::tempdir().unwrap();
    let there = always_on(&d.path().join("there")).await;
    let link = serve(&there).await;
    let here = Engine::new(d.path().join("here"), |_| {}).unwrap();
    here.update(|s| {
        s.settings.automatic = true;
        s.settings.concurrency = 3;
    })
    .await;
    // The always-on Orbit is busy with a download, so the move waits.
    let downloading = there.cloning.lock().await;
    here.shutdown().await;
    let root = d.path().join("here");
    let always_on = impatient(link);
    let error = move_to(&here, &root, &always_on).await;
    assert!(matches!(error, Err(Failed::Unsure(_))), "{error:?}");
    let seen = always_on.snapshot().await.unwrap();
    assert_eq!(seen["settings"]["concurrency"], 2, "looks as it did");
    let pending = pending_move(&root).unwrap();
    assert!(pending.automatic);
    assert!(!here.snapshot().await.settings.automatic);
    assert_eq!(settle(&pending).await, Outcome::Unsure);

    drop(downloading);
    until(&there, |s| s.settings.concurrency == 3).await;
    assert_eq!(settle(&pending).await, Outcome::Moved);
}

/// A repository being connected as the Orbit moves is connected first and
/// moves with it. Once moving, the Orbit takes no new command from a
/// browser, and once packed, no settings change at all.
#[cfg(unix)]
#[tokio::test]
async fn a_repository_being_connected_as_the_orbit_moves_goes_with_it() {
    let d = tempfile::tempdir().unwrap();
    let gh = super::setup_tests::fake_gh(d.path());
    std::fs::write(d.path().join("gate"), "").unwrap();
    let first = Engine::new(d.path().join("first"), |_| {}).unwrap();
    let link = serve(&first).await;
    let connecting = tokio::spawn(crate::github::GH.scope(gh, {
        let e = first.clone();
        async move {
            e.save_repository(Repository::new("acme/api", ""), None)
                .await
        }
    }));
    for _ in 0..500 {
        if d.path().join("cloning").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(d.path().join("cloning").exists(), "downloads acme/api");
    first.shutdown().await;
    let fencing = tokio::spawn({
        let e = first.clone();
        async move { e.fence().await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!fencing.is_finished(), "waits for the download");
    let browser = crate::client::Client::new(link).unwrap();
    let patch = json!({ "patch": { "concurrency": 3 } });
    let refused = browser.call("update_settings", &patch).await;
    assert_eq!(refused, Err(super::MOVING.into()));
    assert!(browser.snapshot().await.is_ok(), "still shows itself");

    std::fs::write(d.path().join("release"), "").unwrap();
    let saved = connecting.await.unwrap().unwrap();
    assert_eq!(saved.repositories[0].name, "acme/api");
    fencing.await.unwrap().unwrap();
    let moved = first.pack().await.unwrap();
    assert_eq!(
        moved.state["settings"]["repositories"][0]["name"],
        "acme/api"
    );
    let patch = SettingsPatch {
        concurrency: Some(3),
        ..Default::default()
    };
    assert_eq!(
        first.update_settings(patch).await.unwrap_err(),
        super::MOVING
    );
    assert_ne!(first.snapshot().await.settings.concurrency, 3);
}
