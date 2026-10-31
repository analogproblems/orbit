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
    remember(&root, vec!["a".into()]).unwrap();
    assert_eq!(seen(&root), Some(vec!["a".to_string()]));
    assert_eq!(load(&root), Some(link.clone()));
    // A new link starts again: what the other Orbit sent before isn't shown.
    let other = Link::parse(&format!("https://other.example.ts.net/#token={OTHER}")).unwrap();
    save(&root, &other).unwrap();
    assert_eq!(seen(&root), None);
    forget(&root).unwrap();
    assert_eq!(load(&root), None);
    assert!(!root.join(FILE).exists());
    // A damaged file is no link, so the app runs its own Orbit.
    std::fs::write(root.join(FILE), "{not json").unwrap();
    assert_eq!(load(&root), None);
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
    let d = tempfile::tempdir().unwrap();
    let hub = Hub::default();
    let browsers = hub.clone();
    let engine = Engine::new(d.path().into(), move |s: &crate::types::Snapshot| {
        browsers.publish(s)
    })
    .unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
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

#[derive(Debug)]
enum Heard {
    Shown(Value),
    Notified(SentNotice),
    Problem(Option<String>),
}
/// Follows the Orbit at `port` in the background, passing on what it hears.
fn follow(port: u16, token: &str, root: PathBuf) -> mpsc::UnboundedReceiver<Heard> {
    let (tx, rx) = mpsc::unbounded_channel();
    let c = client(port, token);
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
    let listener = loop {
        match tokio::net::TcpListener::bind(("127.0.0.1", r.port)).await {
            Ok(listener) => break listener,
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    };
    let _again = serve(listener, &r.engine, &r.hub);
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
