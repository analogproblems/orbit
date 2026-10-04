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
    let app = router(engine.clone(), hub.clone(), TOKEN.into(), assets());
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
fn serves_every_desktop_command_but_opening_links() {
    let lib = include_str!("lib.rs");
    let start = lib.find("generate_handler![").unwrap() + "generate_handler![".len();
    let end = start + lib[start..].find(']').unwrap();
    let mut desktop: Vec<&str> = lib[start..end]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "open_github")
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
