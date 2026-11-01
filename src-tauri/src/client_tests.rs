use super::*;
use crate::{
    engine::Engine,
    remote::{self, Asset, Assets, Hub},
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{mpsc, watch};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

#[test]
fn reads_access_links() {
    let link = |text: &str| Link::parse(text);
    let ok = link(&format!("https://box.example.ts.net/#token={TOKEN}")).unwrap();
    assert_eq!(ok.base, "https://box.example.ts.net/");
    assert_eq!(ok.token, TOKEN);
    assert_eq!(ok.host(), "box.example.ts.net");
    // Pasted with spaces, a port, a path and capitals: the same Orbit.
    let ok = link(&format!(
        "  https://box.example.ts.net:8443/live?x=1#token={}\n",
        TOKEN.to_uppercase()
    ))
    .unwrap();
    assert_eq!(ok.base, "https://box.example.ts.net:8443/");
    assert_eq!(ok.token, TOKEN);
    assert_eq!(ok.host(), "box.example.ts.net:8443");
    // Plain http only on this computer.
    for here in [
        "http://127.0.0.1:4317/",
        "http://localhost:4317/",
        "http://LOCALHOST/",
        "http://[::1]:4317/",
        "http://127.0.0.2/",
    ] {
        let ok = link(&format!("{here}#token={TOKEN}")).unwrap_or_else(|e| panic!("{here}: {e}"));
        assert!(ok.base.starts_with("http://"), "{here}");
    }
    assert_eq!(
        link(&format!("http://[::1]:4317/#token={TOKEN}"))
            .unwrap()
            .host(),
        "[::1]:4317"
    );
    for elsewhere in [
        "http://box.example.ts.net/",
        "http://192.168.1.20:4317/",
        "http://100.64.0.1/",
        "http://localhost.example.com/",
        "http://127.0.0.1.example.com/",
    ] {
        assert_eq!(
            link(&format!("{elsewhere}#token={TOKEN}")),
            Err(NOT_PRIVATE.to_string()),
            "{elsewhere}"
        );
    }
    for wrong in [
        "https://box.example.ts.net/".to_string(),
        "https://box.example.ts.net/#token=".into(),
        format!("https://box.example.ts.net/#token={}", &TOKEN[1..]),
        format!("https://box.example.ts.net/#token={TOKEN}0"),
        format!("https://box.example.ts.net/#token={}", "g".repeat(64)),
        format!("https://box.example.ts.net/#key={TOKEN}"),
        format!("https://me:secret@box.example.ts.net/#token={TOKEN}"),
        format!("https://me@box.example.ts.net/#token={TOKEN}"),
        format!("ftp://box.example.ts.net/#token={TOKEN}"),
        format!("file:///home/me/#token={TOKEN}"),
        format!("box.example.ts.net/#token={TOKEN}"),
        format!("#token={TOKEN}"),
        String::new(),
    ] {
        assert_eq!(link(&wrong), Err(NOT_A_LINK.to_string()), "{wrong}");
    }
}

#[test]
fn keeps_the_link_privately_until_you_stop() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("data");
    assert_eq!(load(&root), None);
    // Nothing to stop is fine.
    forget(&root).unwrap();
    let link = Link::parse(&format!("https://box.example.ts.net/#token={TOKEN}")).unwrap();
    save(&root, &link).unwrap();
    assert_eq!(load(&root), Some(link.clone()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(root.join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert_eq!(seen(&root, &link), None);
    remember(&root, &link, vec!["a".into()]).unwrap();
    assert_eq!(seen(&root, &link), Some(vec!["a".to_string()]));
    assert_eq!(load(&root), Some(link.clone()));
    // A new link starts again: what the other Orbit sent before isn't shown.
    let other = Link::parse(&format!("https://other.example.ts.net/#token={OTHER}")).unwrap();
    save(&root, &other).unwrap();
    assert_eq!(seen(&root, &other), None);
    // Nor with a new link to the same Orbit.
    let renewed = Link::parse(&format!("https://box.example.ts.net/#token={OTHER}")).unwrap();
    assert_eq!(seen(&root, &renewed), None);
    forget(&root).unwrap();
    assert_eq!(load(&root), None);
    assert!(!root.join(FILE).exists());
    // A damaged file is no link, so the app runs its own Orbit.
    std::fs::write(root.join(FILE), "{not json").unwrap();
    assert_eq!(load(&root), None);
}

/// What was shown is noted while the app follows an Orbit, which can be
/// just as you stop using it or give it a new link. A note written after
/// that never brings the old link back.
#[test]
fn a_late_note_of_what_was_shown_never_brings_a_link_back() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    let old = Link::parse(&format!("https://box.example.ts.net/#token={TOKEN}")).unwrap();
    let new = Link::parse(&format!("https://other.example.ts.net/#token={OTHER}")).unwrap();
    save(root, &old).unwrap();
    remember(root, &old, vec!["a".into()]).unwrap();
    // You tap Stop using it, then the note is written.
    forget(root).unwrap();
    remember(root, &old, vec!["a".into(), "b".into()]).unwrap();
    assert_eq!(load(root), None);
    // You give it a new link, then the old follower's note is written.
    save(root, &old).unwrap();
    save(root, &new).unwrap();
    remember(root, &old, vec!["c".into()]).unwrap();
    assert_eq!(load(root), Some(new.clone()));
    // And it counts for nothing with the new link.
    assert_eq!(seen(root, &new), None);
    remember(root, &new, vec!["d".into()]).unwrap();
    assert_eq!(seen(root, &new), Some(vec!["d".to_string()]));
    assert_eq!(seen(root, &old), None);
    assert_eq!(load(root), Some(new));
}

fn notice(id: &str, key: &str) -> Value {
    json!({
        "id": id,
        "title": format!("Title {id}"),
        "body": format!("Body {id}"),
        "item": {
            "key": key,
            "kind": "brief",
            "prId": null,
            "reviewId": null,
            "approvalId": null,
            "repo": null,
            "reason": "Something"
        }
    })
}
fn snapshot(notices: &[Value], open: &[&str], notify: bool) -> Value {
    json!({
        "settings": { "notify": notify },
        "notices": notices,
        "attention": open.iter().map(|key| json!({ "key": key })).collect::<Vec<_>>(),
    })
}
fn ids(sent: &[SentNotice]) -> Vec<&str> {
    sent.iter().map(|n| n.id.as_str()).collect()
}

#[test]
fn shows_each_notification_once_while_its_item_is_open() {
    let a = notice("a", "brief:1");
    let b = notice("b", "brief:2");
    let c = notice("c", "brief:3");
    let mut relay = Relay::new(None);
    // The first snapshot ever only marks what's there, sent before this
    // app followed that Orbit.
    let (shown, kept) = relay.take(&snapshot(std::slice::from_ref(&a), &["brief:1"], true));
    assert!(shown.is_empty());
    assert_eq!(kept, Some(vec!["a".to_string()]));
    let (shown, kept) = relay.take(&snapshot(
        &[a.clone(), b.clone()],
        &["brief:1", "brief:2"],
        true,
    ));
    assert_eq!(ids(&shown), ["b"]);
    assert_eq!(shown[0].title, "Title b");
    assert_eq!(shown[0].item.key, "brief:2");
    assert_eq!(kept, Some(vec!["a".to_string(), "b".to_string()]));
    // The same again, as after a reconnect: nothing, and nothing to save.
    let (shown, kept) = relay.take(&snapshot(
        &[a.clone(), b.clone()],
        &["brief:1", "brief:2"],
        true,
    ));
    assert!(shown.is_empty());
    assert_eq!(kept, None);
    // An item that closed before the app heard of it isn't shown.
    let (shown, kept) = relay.take(&snapshot(
        &[a.clone(), b.clone(), c.clone()],
        &["brief:1"],
        true,
    ));
    assert!(shown.is_empty());
    assert_eq!(kept.unwrap().len(), 3);
    // Nor anything while notifications are off there.
    let d = notice("d", "brief:4");
    let (shown, _) = relay.take(&snapshot(std::slice::from_ref(&d), &["brief:4"], false));
    assert!(shown.is_empty());
    // An older notification dropped there is forgotten here too.
    let e = notice("e", "brief:5");
    let (shown, kept) = relay.take(&snapshot(&[d, e], &["brief:4", "brief:5"], true));
    assert_eq!(ids(&shown), ["e"]);
    assert_eq!(kept, Some(vec!["d".to_string(), "e".to_string()]));
}

#[test]
fn an_orbit_with_no_notifications_yet_shows_its_first() {
    let mut relay = Relay::new(None);
    // An Orbit from before notifications were kept sends none.
    let (shown, kept) = relay.take(&json!({ "settings": {} }));
    assert!(shown.is_empty());
    assert_eq!(kept, Some(vec![]));
    let a = notice("a", "brief:1");
    let (shown, _) = relay.take(&snapshot(&[a], &["brief:1"], true));
    assert_eq!(ids(&shown), ["a"]);
}

#[test]
fn remembers_what_was_shown_across_a_restart() {
    let a = notice("a", "brief:1");
    let b = notice("b", "brief:2");
    let mut relay = Relay::new(Some(vec!["a".into()]));
    let (shown, kept) = relay.take(&snapshot(&[a, b], &["brief:1", "brief:2"], true));
    assert_eq!(ids(&shown), ["b"]);
    assert!(kept.is_some());
    // A notification it can't read is skipped, not fatal.
    let mut relay = Relay::new(Some(vec![]));
    let (shown, _) = relay.take(&snapshot(
        &[json!({ "id": "x" }), notice("c", "brief:3")],
        &["brief:3"],
        true,
    ));
    assert_eq!(ids(&shown), ["c"]);
}

/// An always-on Orbit on a port of this computer, as `orbit-server` runs
/// it. Its engine's snapshots go to its event streams.
struct Remote {
    _d: tempfile::TempDir,
    engine: Arc<Engine>,
    hub: Hub,
    port: u16,
    stop: watch::Sender<bool>,
}
fn assets() -> Assets {
    Arc::new(|_: &str| -> Option<Asset> { None })
}
async fn remote_orbit() -> Remote {
    remote_orbit_on(
        tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap(),
    )
}
fn remote_orbit_on(listener: tokio::net::TcpListener) -> Remote {
    let d = tempfile::tempdir().unwrap();
    let hub = Hub::default();
    let browsers = hub.clone();
    let engine = Engine::new(d.path().into(), move |s: &crate::types::Snapshot| {
        browsers.publish(s)
    })
    .unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = serve(listener, &engine, &hub);
    Remote {
        _d: d,
        engine,
        hub,
        port,
        stop,
    }
}
/// Serves `engine` on `listener` until the returned sender says stop.
fn serve(
    listener: tokio::net::TcpListener,
    engine: &Arc<Engine>,
    hub: &Hub,
) -> watch::Sender<bool> {
    let (stop, stopped) = watch::channel(false);
    let app = remote::router(
        engine.clone(),
        hub.clone(),
        TOKEN.into(),
        assets(),
        stopped.clone(),
        remote::SERVER_MARK,
    );
    let mut until = stopped;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = until.wait_for(|stop| *stop).await;
            })
            .await;
    });
    stop
}
fn client(port: u16, token: &str) -> Client {
    Client::new(Link::parse(&format!("http://127.0.0.1:{port}/#token={token}")).unwrap()).unwrap()
}
/// A client that waits a fraction of a second for anything.
fn impatient(port: u16) -> Client {
    let link = Link::parse(&format!("http://127.0.0.1:{port}/#token={TOKEN}")).unwrap();
    let short = std::time::Duration::from_millis(300);
    let limits = Limits {
        silence: short,
        snapshot: short,
        action: short * 2,
        moving: short * 2,
    };
    Client::with_limits(link, limits).unwrap()
}
/// Binds `port` again, once whatever had it lets it go.
async fn rebind(port: u16) -> tokio::net::TcpListener {
    loop {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => break listener,
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    }
}
/// A port that takes connections and never answers on them, as a stuck
/// server or proxy would.
async fn silent() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let mut held = vec![];
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    (port, task)
}

#[tokio::test]
async fn runs_commands_on_the_always_on_orbit() {
    let r = remote_orbit().await;
    let c = client(r.port, TOKEN);
    let snapshot = c.call("get_snapshot", &json!({})).await.unwrap();
    assert!(snapshot["settings"].is_object());
    let settings = c
        .call("update_settings", &json!({ "patch": { "concurrency": 3 } }))
        .await
        .unwrap();
    assert_eq!(settings["concurrency"], 3);
    assert_eq!(r.engine.snapshot().await.settings.concurrency, 3);
    // Its own errors come back as it words them.
    assert_eq!(
        c.call("export_report", &json!({ "reviewId": "x" })).await,
        Err("Unknown review.".into())
    );
    // What a browser can't do there, this app can't either.
    for refused in [
        "open_github",
        "set_remote_access",
        "choose_checkout",
        "nope",
    ] {
        assert_eq!(
            c.call(refused, &json!({ "url": "https://github.com" }))
                .await,
            Err("This action is not available here.".into()),
            "{refused}"
        );
    }
    // A link that no longer works says so.
    assert_eq!(
        client(r.port, OTHER).call("get_snapshot", &json!({})).await,
        Err(EXPIRED.into())
    );
}

#[tokio::test]
async fn says_when_the_always_on_orbit_cannot_be_reached() {
    let port = {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    let error = client(port, TOKEN)
        .call("get_snapshot", &json!({}))
        .await
        .unwrap_err();
    assert!(error.contains("can't reach the always-on Orbit"), "{error}");
    assert!(error.contains(&format!("127.0.0.1:{port}")), "{error}");
}

#[tokio::test]
async fn gives_up_on_an_orbit_that_takes_the_connection_but_never_answers() {
    let (port, _silent) = silent().await;
    let c = impatient(port);
    let started = std::time::Instant::now();
    // Bounded here too, so a regression fails rather than hangs.
    let soon = std::time::Duration::from_secs(5);
    for command in ["get_snapshot", "poll_now"] {
        let error = tokio::time::timeout(soon, c.call(command, &json!({})))
            .await
            .expect("it should give up")
            .unwrap_err();
        assert!(
            error.contains("didn't answer in time"),
            "{command}: {error}"
        );
        assert!(error.contains(&format!("127.0.0.1:{port}")), "{error}");
    }
    let checked = tokio::time::timeout(soon, c.check()).await;
    assert!(matches!(checked, Ok(Err(_))), "{checked:?}");
    assert!(started.elapsed() < soon);
}

/// This app's Orbit moves through the link to an always-on Orbit with
/// nothing of its own yet, signed in as the same account, and only once.
#[tokio::test]
async fn moves_into_an_always_on_orbit_with_nothing_of_its_own() {
    let r = remote_orbit().await;
    r.engine.update(|s| s.github_user = Some("me".into())).await;
    let c = client(r.port, TOKEN);
    let before = c.snapshot().await.unwrap();
    assert_eq!(can_take_in(&before, Some("Me")), Ok(()));
    assert_eq!(can_take_in(&before, None), Ok(()));
    let error = can_take_in(&before, Some("you")).unwrap_err();
    assert!(error.contains("signed in to GitHub as me"), "{error}");
    let signed_out = json!({ "settings": { "repositories": [] }, "githubUser": null });
    assert_eq!(can_take_in(&signed_out, None), Err(NOT_SIGNED_IN.into()));

    let d = tempfile::tempdir().unwrap();
    let here = Engine::new(d.path().into(), |_| {}).unwrap();
    here.update(|s| {
        s.settings.concurrency = 3;
        s.scans = vec![crate::types::SecurityScan {
            id: "scan-1".into(),
            repo: "a/b".into(),
            status: "completed".into(),
            ..Default::default()
        }];
    })
    .await;
    let name = here.snapshot().await.instance.name;
    let moved = here.pack().await.unwrap();
    // A link that no longer works says so, and moves nothing.
    assert_eq!(
        client(r.port, OTHER).move_in(&moved).await,
        Err(EXPIRED.into())
    );
    c.move_in(&moved).await.unwrap();
    let after = c.snapshot().await.unwrap();
    assert_eq!(after["settings"]["concurrency"], 3);
    assert!(took_in(&before, &after, &name));
    assert!(!took_in(&after, &after, &name));
    assert!(!took_in(&before, &before, &name));
    assert_eq!(can_take_in(&after, Some("me")), Err(NOT_EMPTY.into()));
    let error = c.move_in(&moved).await.unwrap_err();
    assert!(error.contains("already has repositories"), "{error}");
}

/// An Orbit, empty and signed in, to move into, served at `port` by a
/// stand-in that takes the move in and then goes away without answering,
/// as when the connection drops just as the move went through.
async fn lost_on_the_way(target: Arc<Engine>) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        drop(listener);
        let mut request = vec![];
        let mut chunk = [0; 64 * 1024];
        let (head, length) = loop {
            let n = stream.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map(|n| n.trim().parse::<usize>().unwrap())
                    .unwrap();
                break (end + 4, length);
            }
        };
        while request.len() < head + length {
            let n = stream.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
        }
        let moved = serde_json::from_slice(&request[head..head + length]).unwrap();
        target.take_in(moved).await.unwrap();
        let _ = stream.shutdown().await;
    });
    port
}

/// A move whose answer is lost, with the always-on Orbit then out of
/// reach, leaves this app's Orbit paused and the move noted, until the
/// always-on Orbit answers again and shows it moved.
#[tokio::test]
async fn a_move_the_app_cannot_confirm_leaves_its_orbit_paused_until_it_can() {
    let d = tempfile::tempdir().unwrap();
    let target = Engine::new(d.path().join("there"), |_| {}).unwrap();
    target.update(|s| s.github_user = Some("me".into())).await;
    let here = Engine::new(d.path().join("here"), |_| {}).unwrap();
    here.update(|s| {
        s.settings.automatic = true;
        s.settings.concurrency = 3;
    })
    .await;
    let name = here.snapshot().await.instance.name;
    let port = lost_on_the_way(target.clone()).await;
    let before = json!({ "instance": { "former": [] } });
    here.shutdown().await;
    let root = d.path().join("here");
    let error = move_to(&here, &root, &impatient(port), &before).await;
    assert!(matches!(error, Err(Failed::Unsure(_))), "{error:?}");
    let pending = pending_move(&root).unwrap();
    assert_eq!(pending.name, name);
    assert!(pending.automatic);
    assert_eq!(pending.link.base, format!("http://127.0.0.1:{port}/"));
    // Autopilot is off, and stays off after a restart.
    let saved: crate::types::Snapshot =
        serde_json::from_slice(&std::fs::read(root.join("state.json")).unwrap()).unwrap();
    assert!(!saved.settings.automatic);
    assert_eq!(target.snapshot().await.settings.concurrency, 3);
    // Still out of reach as the app starts again: it still can't tell.
    assert_eq!(settle(&pending).await, Outcome::Unsure);
    // Back, it says the move went through.
    let stop = serve(rebind(port).await, &target, &Hub::default());
    assert_eq!(settle(&pending).await, Outcome::Moved);
    let _ = stop.send(true);
    end_move(&root).unwrap();
    assert_eq!(pending_move(&root), None);
}

/// A move the always-on Orbit refuses starts this app's Orbit again as it
/// was, autopilot and all, with nothing noted.
#[tokio::test]
async fn a_move_the_always_on_orbit_refuses_leaves_this_orbit_as_it_was() {
    let r = remote_orbit().await;
    r.engine.update(|s| s.github_user = Some("me".into())).await;
    let c = client(r.port, TOKEN);
    let before = c.snapshot().await.unwrap();
    // Another Orbit moved in first.
    let d = tempfile::tempdir().unwrap();
    let other = Engine::new(d.path().join("other"), |_| {}).unwrap();
    other
        .update(|s| {
            s.scans = vec![crate::types::SecurityScan {
                id: "scan-1".into(),
                status: "completed".into(),
                ..Default::default()
            }]
        })
        .await;
    c.move_in(&other.pack().await.unwrap()).await.unwrap();

    let here = Engine::new(d.path().join("here"), |_| {}).unwrap();
    here.update(|s| s.settings.automatic = true).await;
    let pending = Moving {
        link: c.link().clone(),
        name: here.snapshot().await.instance.name,
        automatic: true,
    };
    here.shutdown().await;
    let root = d.path().join("here");
    match move_to(&here, &root, &c, &before).await {
        Err(Failed::Stayed(error)) => {
            assert!(error.contains("already has repositories"), "{error}")
        }
        other => panic!("{other:?}"),
    }
    assert!(here.snapshot().await.settings.automatic);
    assert_eq!(pending_move(&root), None);
    // As the app starts, a move noted then counts as not made.
    assert_eq!(settle(&pending).await, Outcome::Stayed);
}

/// An always-on Orbit from before moving was possible says it needs
/// updating.
#[tokio::test]
async fn an_always_on_orbit_too_old_to_take_one_in_says_so() {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    // As one did, it knows no move-in and answers as for a command it
    // doesn't run.
    let old = axum::Router::new().fallback(|| async {
        (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(json!({ "error": "This action is not available here." })),
        )
    });
    tokio::spawn(async move { axum::serve(listener, old).await });
    let moved = crate::moving::Moved::default();
    assert_eq!(
        client(port, TOKEN).move_in(&moved).await,
        Err(TOO_OLD.into())
    );
}

#[derive(Debug)]
enum Heard {
    Shown(Value),
    Notified(SentNotice),
    Problem(Option<String>),
}
/// Follows the Orbit at `port` in the background, passing on what it hears.
fn follow(port: u16, token: &str, root: PathBuf) -> mpsc::UnboundedReceiver<Heard> {
    follow_with(client(port, token), root)
}
fn follow_with(c: Client, root: PathBuf) -> mpsc::UnboundedReceiver<Heard> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let (shown, notified, problem) = (tx.clone(), tx.clone(), tx);
        c.follow(
            &root,
            |s| {
                let _ = shown.send(Heard::Shown(s.clone()));
            },
            |n| {
                let _ = notified.send(Heard::Notified(n.clone()));
            },
            |p| {
                let _ = problem.send(Heard::Problem(p.map(String::from)));
            },
        )
        .await;
    });
    rx
}
async fn next(rx: &mut mpsc::UnboundedReceiver<Heard>) -> Heard {
    tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("heard nothing")
        .expect("stopped following")
}
async fn shown(rx: &mut mpsc::UnboundedReceiver<Heard>) -> Value {
    match next(rx).await {
        Heard::Shown(s) => {
            assert!(matches!(next(rx).await, Heard::Problem(None)));
            s
        }
        other => panic!("expected a snapshot, heard {other:?}"),
    }
}

#[tokio::test]
async fn follows_the_always_on_orbit_and_reconnects() {
    let r = remote_orbit().await;
    let here = tempfile::tempdir().unwrap();
    let mut rx = follow(r.port, TOKEN, here.path().into());
    assert_eq!(shown(&mut rx).await["settings"]["concurrency"], 2);
    r.engine.update(|s| s.settings.concurrency = 4).await;
    r.engine.flush().await.unwrap();
    assert_eq!(shown(&mut rx).await["settings"]["concurrency"], 4);
    // It goes away, and comes back on the same port.
    r.stop.send(true).unwrap();
    match next(&mut rx).await {
        Heard::Problem(Some(p)) => assert_eq!(p, LOST),
        other => panic!("expected the connection lost, heard {other:?}"),
    }
    let _again = serve(rebind(r.port).await, &r.engine, &r.hub);
    loop {
        match next(&mut rx).await {
            // It may try more than once while the port comes back.
            Heard::Problem(Some(p)) => assert_eq!(p, LOST),
            Heard::Shown(s) => {
                assert_eq!(s["settings"]["concurrency"], 4);
                break;
            }
            other => panic!("expected a snapshot, heard {other:?}"),
        }
    }
    assert!(matches!(next(&mut rx).await, Heard::Problem(None)));
}

/// A stuck server or proxy takes the connection and never answers. The app
/// says so and tries again, rather than waiting for ever, so it shows the
/// Orbit once it's back.
#[tokio::test]
async fn follows_again_after_an_orbit_that_never_answered() {
    let (port, stuck) = silent().await;
    let here = tempfile::tempdir().unwrap();
    let mut rx = follow_with(impatient(port), here.path().into());
    match next(&mut rx).await {
        Heard::Problem(Some(p)) => assert_eq!(p, LOST),
        other => panic!("expected the connection lost, heard {other:?}"),
    }
    stuck.abort();
    let _ = stuck.await;
    let _r = remote_orbit_on(rebind(port).await);
    loop {
        match next(&mut rx).await {
            Heard::Problem(Some(p)) => assert_eq!(p, LOST),
            Heard::Shown(s) => {
                assert!(s["settings"].is_object());
                break;
            }
            other => panic!("expected a snapshot, heard {other:?}"),
        }
    }
}

#[tokio::test]
async fn stops_following_when_the_link_stops_working() {
    let r = remote_orbit().await;
    let here = tempfile::tempdir().unwrap();
    let mut rx = follow(r.port, OTHER, here.path().into());
    match next(&mut rx).await {
        Heard::Problem(Some(p)) => assert_eq!(p, EXPIRED),
        other => panic!("expected the link refused, heard {other:?}"),
    }
    // And it gives up rather than trying again.
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;
    assert!(matches!(end, Ok(None)), "{end:?}");
}

/// Opens a morning brief on `engine`, which Orbit notifies.
async fn open_brief(engine: &Engine, day: &str) {
    engine
        .update(|s| {
            s.brief.latest = Some(crate::brief::Brief {
                day: day.into(),
                text: format!("Brief for {day}"),
                open: true,
                ..Default::default()
            })
        })
        .await;
    engine.flush().await.unwrap();
}

#[tokio::test]
async fn shows_the_notifications_the_always_on_orbit_sends() {
    let r = remote_orbit().await;
    let told = Arc::new(std::sync::Mutex::new(vec![]));
    let there = told.clone();
    r.engine
        .on_notice(move |n| there.lock().unwrap().push(n.title.clone()));
    // One sent before this app followed it isn't shown here.
    open_brief(&r.engine, "2026-10-30").await;
    let before = r.engine.snapshot().await.notices;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].item.key, "brief:2026-10-30");
    let here = tempfile::tempdir().unwrap();
    let mut rx = follow(r.port, TOKEN, here.path().into());
    shown(&mut rx).await;
    open_brief(&r.engine, "2026-10-31").await;
    // The snapshot first, then the notification for it.
    let s = match next(&mut rx).await {
        Heard::Shown(s) => s,
        other => panic!("expected a snapshot, heard {other:?}"),
    };
    assert_eq!(s["notices"].as_array().unwrap().len(), 2);
    match next(&mut rx).await {
        Heard::Notified(n) => {
            assert_eq!(n.title, "Your morning brief");
            assert_eq!(n.item.key, "brief:2026-10-31");
        }
        other => panic!("expected a notification, heard {other:?}"),
    }
    assert!(matches!(next(&mut rx).await, Heard::Problem(None)));
    assert_eq!(told.lock().unwrap().len(), 2);
    // After a restart, nothing already shown is shown again.
    drop(rx);
    let mut rx = follow(r.port, TOKEN, here.path().into());
    shown(&mut rx).await;
    r.engine.update(|s| s.settings.concurrency = 5).await;
    r.engine.flush().await.unwrap();
    assert_eq!(shown(&mut rx).await["settings"]["concurrency"], 5);
}

#[tokio::test]
async fn an_orbit_keeps_only_the_notifications_it_sends() {
    let d = tempfile::tempdir().unwrap();
    let engine = Engine::new(d.path().into(), |_| {}).unwrap();
    // Notifications off: none sent, none kept.
    engine.update(|s| s.settings.notify = false).await;
    open_brief(&engine, "2026-10-01").await;
    assert!(engine.snapshot().await.notices.is_empty());
    engine.update(|s| s.settings.notify = true).await;
    for day in 2..=30 {
        open_brief(&engine, &format!("2026-10-{day:02}")).await;
    }
    let kept = engine.snapshot().await.notices;
    assert_eq!(kept.len(), crate::attention::SENT_KEPT);
    assert_eq!(kept.last().unwrap().item.key, "brief:2026-10-30");
    assert_eq!(kept[0].item.key, "brief:2026-10-11");
    let unique: HashSet<&str> = kept.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(unique.len(), kept.len());
    // Saved with the state, so a restart shows none of them again.
    drop(engine);
    let engine = Engine::new(d.path().into(), |_| {}).unwrap();
    assert_eq!(engine.snapshot().await.notices, kept);
}
