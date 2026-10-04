use super::*;
use axum::http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn assets() -> Assets {
    Arc::new(|path: &str| {
        (path == "index.html").then(|| Asset {
            bytes: b"<!doctype html>".to_vec(),
            mime_type: "text/html".into(),
        })
    })
}
fn server() -> (tempfile::TempDir, Arc<Engine>, Hub, Router) {
    let d = tempfile::tempdir().unwrap();
    let engine = Engine::new(d.path().into(), |_| {}).unwrap();
    let hub = Hub::default();
    let app = router(
        engine.clone(),
        hub.clone(),
        TOKEN.into(),
        assets(),
        watch::channel(false).1,
    );
    (d, engine, hub, app)
}
fn call(command: &str, token: Option<&str>, body: &str) -> Request<Body> {
    let mut request = Request::post(format!("/api/{command}"));
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request.body(Body::from(body.to_string())).unwrap()
}
async fn json(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn api_needs_the_access_token() {
    let (_d, _e, _hub, app) = server();
    for token in [
        None,
        Some("wrong"),
        Some(&TOKEN[1..]),
        Some(&TOKEN.to_uppercase()[..]),
    ] {
        let response = app
            .clone()
            .oneshot(call("get_snapshot", token, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        let events = Request::get("/api/events");
        let events = match token {
            Some(t) => events.header("authorization", format!("Bearer {t}")),
            None => events,
        };
        let response = app
            .clone()
            .oneshot(events.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{token:?}");
    }
}

#[tokio::test]
async fn runs_commands_with_the_access_token() {
    let (_d, _e, _hub, app) = server();
    let response = app
        .clone()
        .oneshot(call("get_snapshot", Some(TOKEN), ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(json(response).await["settings"].is_object());

    let response = app
        .clone()
        .oneshot(call(
            "update_settings",
            Some(TOKEN),
            r#"{"patch":{"concurrency":3}}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json(response).await["concurrency"], 3);
}

#[tokio::test]
async fn reports_command_errors_and_refuses_other_commands() {
    let (_d, _e, _hub, app) = server();
    let response = app
        .clone()
        .oneshot(call("export_report", Some(TOKEN), r#"{"reviewId":"x"}"#))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json(response).await["error"], "Unknown review.");

    let response = app
        .clone()
        .oneshot(call("update_settings", Some(TOKEN), r#"{"patch":3}"#))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json(response).await["error"]
        .as_str()
        .unwrap()
        .starts_with("Invalid patch"));

    let response = app
        .clone()
        .oneshot(call("get_snapshot", Some(TOKEN), "[]"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    for name in ["open_github", "reset_demo"] {
        let response = app
            .clone()
            .oneshot(call(
                name,
                Some(TOKEN),
                r#"{"url":"https://github.com/a/b"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{name}");
    }
}

#[tokio::test]
async fn streams_the_snapshot_then_each_change() {
    let (_d, engine, hub, app) = server();
    let response = app
        .oneshot(
            Request::get("/api/events")
                .header("authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut next = async || {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let line = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        assert!(line.ends_with('\n'));
        serde_json::from_str::<Value>(&line).unwrap()
    };
    assert_eq!(next().await["settings"]["concurrency"], 2);
    engine.update(|s| s.settings.concurrency = 4).await;
    hub.publish(&engine.shown_snapshot().await);
    assert_eq!(next().await["settings"]["concurrency"], 4);
}

#[tokio::test]
async fn publishes_nothing_without_a_browser() {
    let hub = Hub::default();
    hub.publish(&Snapshot::default());
    assert!(hub.0.borrow().is_none());
}

#[tokio::test]
async fn serves_the_frontend_without_escaping_it() {
    let (_d, _e, _hub, app) = server();
    let response = app
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
    assert_eq!(
        response.headers()[header::CONTENT_SECURITY_POLICY],
        HTML_CSP
    );
    for path in [
        "/../index.html",
        "/a/../index.html",
        "/%2e%2e/index.html",
        "/a\\b",
        "/api/get_snapshot/index.html",
        "/missing.js",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[test]
fn serves_every_desktop_command_but_the_desktop_only_ones() {
    let lib = include_str!("lib.rs");
    let start = lib.find("generate_handler![").unwrap() + "generate_handler![".len();
    let end = start + lib[start..].find(']').unwrap();
    let mut desktop: Vec<&str> = lib[start..end]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty() && !DESKTOP_ONLY.contains(name))
        .collect();
    let mut remote = COMMANDS.to_vec();
    desktop.sort_unstable();
    remote.sort_unstable();
    assert_eq!(desktop, remote);
}

#[test]
fn keeps_one_access_token() {
    let d = tempfile::tempdir().unwrap();
    let first = token(d.path()).unwrap();
    assert!(valid_token(&first));
    assert_eq!(token(d.path()).unwrap(), first);
    std::fs::write(d.path().join(TOKEN_FILE), "short").unwrap();
    let replaced = token(d.path()).unwrap();
    assert!(valid_token(&replaced));
    assert_ne!(replaced, first);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.path().join(TOKEN_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

/// Sends one HTTP/1.1 request and returns the status code and body.
async fn request(port: u16, method: &str, path: &str, token: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let status = response[9..12].parse().unwrap();
    (status, response)
}
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn link_token(status: &Status) -> String {
    status
        .link
        .as_deref()
        .unwrap()
        .split_once("#token=")
        .unwrap()
        .1
        .to_string()
}
fn access() -> (tempfile::TempDir, Arc<Access>) {
    let d = tempfile::tempdir().unwrap();
    let engine = Engine::new(d.path().into(), |_| {}).unwrap();
    let access = Access::new(engine, Hub::default(), d.path().into(), assets());
    (d, access)
}

#[tokio::test]
async fn turns_on_and_off_from_settings() {
    let (d, access) = access();
    access.start().await;
    let status = access.status().await;
    assert!(!status.enabled && !status.running && status.link.is_none());
    assert_eq!(status.port, DEFAULT_PORT);

    let port = free_port();
    let status = access.set(true, port).await.unwrap();
    assert!(status.running, "{status:?}");
    assert_eq!(status.error, None);
    let link = status.link.clone().unwrap();
    assert!(link.starts_with(&format!("http://127.0.0.1:{port}/#token=")));
    let token = link_token(&status);
    assert_eq!(
        token,
        std::fs::read_to_string(d.path().join(TOKEN_FILE)).unwrap()
    );
    assert_eq!(
        request(port, "POST", "/api/get_snapshot", &token).await.0,
        200
    );

    // The choice is kept for the next launch.
    let saved: Config =
        serde_json::from_slice(&std::fs::read(d.path().join(CONFIG_FILE)).unwrap()).unwrap();
    assert_eq!(
        saved,
        Config {
            enabled: true,
            port
        }
    );

    let status = access.set(false, port).await.unwrap();
    assert!(!status.running && status.link.is_none());
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
}

#[tokio::test]
async fn reset_link_disconnects_the_old_link() {
    let (_d, access) = access();
    let port = free_port();
    let old = link_token(&access.set(true, port).await.unwrap());
    let new = link_token(&access.reset_link().await.unwrap());
    assert_ne!(old, new);
    assert_eq!(
        request(port, "POST", "/api/get_snapshot", &old).await.0,
        401
    );
    assert_eq!(
        request(port, "POST", "/api/get_snapshot", &new).await.0,
        200
    );
}

#[tokio::test]
async fn ends_open_event_streams_when_stopped() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (_d, access) = access();
    let port = free_port();
    let token = link_token(&access.set(true, port).await.unwrap());
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!("GET /api/events HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut first = [0u8; 12];
    stream.read_exact(&mut first).await.unwrap();
    assert_eq!(&first, b"HTTP/1.1 200");
    tokio::time::timeout(Duration::from_secs(5), access.set(false, port))
        .await
        .unwrap()
        .unwrap();
    let mut rest = vec![];
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest))
        .await
        .expect("the stream should close")
        .unwrap();
}

#[tokio::test]
async fn reports_a_port_in_use_and_refuses_low_ports() {
    let (_d, access) = access();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let status = access.set(true, port).await.unwrap();
    assert!(status.enabled && !status.running);
    assert!(status
        .error
        .unwrap()
        .starts_with(&format!("Port {port} is not available")));
    drop(taken);
    // Saving the same choice again retries it.
    assert!(access.set(true, port).await.unwrap().running);
    assert!(access.set(true, 80).await.is_err());
}

#[tokio::test]
async fn starts_again_on_the_next_launch() {
    let d = tempfile::tempdir().unwrap();
    let port = free_port();
    let config = Config {
        enabled: true,
        port,
    };
    std::fs::write(
        d.path().join(CONFIG_FILE),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let engine = Engine::new(d.path().into(), |_| {}).unwrap();
    let access = Access::new(engine, Hub::default(), d.path().into(), assets());
    access.start().await;
    assert!(access.status().await.running);
}

#[test]
fn reads_the_tailnet_name() {
    assert_eq!(
        tailnet_name(r#"{"Self":{"DNSName":"Desk.example.ts.net."}}"#).as_deref(),
        Some("desk.example.ts.net")
    );
    for status in [
        r#"{"Self":{"DNSName":""}}"#,
        r#"{"Self":{"DNSName":"a/b.ts.net"}}"#,
        r#"{"Self":{}}"#,
        "not json",
    ] {
        assert_eq!(tailnet_name(status), None, "{status}");
    }
}
