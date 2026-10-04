//! Serves Orbit to a browser on another device, such as a phone reaching the
//! desktop over Tailscale. The server listens on `127.0.0.1` only; `tailscale
//! serve` publishes it to the tailnet. It shares the desktop app's engine, so
//! every browser sees the same live workspace as the desktop window.
//!
//! - `POST /api/<command>` runs one of [`COMMANDS`] with a JSON object of the
//!   same camelCase arguments the desktop window sends to Tauri.
//! - `GET /api/events` streams snapshots as JSON lines: the current one first,
//!   then each change. Empty lines keep the connection alive.
//! - Any other `GET` serves the built frontend.
//!
//! Every `/api` request needs `Authorization: Bearer <token>`. The token comes
//! from the access link's URL fragment, so it is never sent in a URL. There are
//! no cookies, so other websites cannot send commands, and a header that is not
//! a CORS-safelisted one makes browsers refuse cross-origin requests.
use crate::{engine::Engine, types::Snapshot};
use axum::{
    body::{Body, Bytes},
    extract::{Path as Route, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::watch};

/// Commands a browser can run, named as the desktop app names them.
/// `open_github` is left out: the server never starts programs on the host for
/// a remote caller, so the browser opens GitHub links itself. `export_report`
/// returns the report's file name and text for the browser to download.
pub const COMMANDS: &[&str] = &[
    "get_snapshot",
    "update_settings",
    "save_repository",
    "set_repository_automatic",
    "disconnect_repository",
    "get_storage_usage",
    "remove_worktrees",
    "export_report",
    "poll_now",
    "connect_services",
    "start_review",
    "cancel_review",
    "stop_loop",
    "continue_loop",
    "publish_review",
    "publish_plan",
    "send_message",
    "set_legacy_chat_model",
    "answer_approval",
    "get_files",
    "list_repositories",
];
/// How often an idle event stream sends an empty line, so the browser can
/// tell a quiet workspace from a dropped connection.
const KEEPALIVE: Duration = Duration::from_secs(15);
const TOKEN_FILE: &str = "remote-token";
const HTML_CSP: &str = "default-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self'; object-src 'none'; base-uri 'self'; form-action 'none'; frame-ancestors 'none'";

/// A file of the built frontend.
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}
/// Looks up a built frontend file by its path, such as `index.html`.
pub type Assets = Arc<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

/// Passes each emitted snapshot to the connected browsers. Snapshots are only
/// serialized while a browser is connected, and a slow browser skips to the
/// latest one rather than queueing every change.
#[derive(Clone)]
pub struct Hub(Arc<watch::Sender<Option<Arc<str>>>>);
impl Default for Hub {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(None)))
    }
}
impl Hub {
    pub fn publish(&self, snapshot: &Snapshot) {
        if self.0.receiver_count() == 0 {
            return;
        }
        if let Ok(json) = serde_json::to_string(snapshot) {
            self.0.send_replace(Some(json.into()));
        }
    }
}

/// The access token kept in Orbit's data folder, created on first use.
pub fn token(root: &Path) -> Result<String, String> {
    let path = root.join(TOKEN_FILE);
    if let Ok(saved) = std::fs::read_to_string(&path) {
        let saved = saved.trim();
        if valid_token(saved) {
            return Ok(saved.into());
        }
    }
    // Version 4 UUIDs come from the operating system's secure random source.
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    // `mode` only applies to a new file, so also narrow one left from before.
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, token.as_bytes()).map_err(|e| e.to_string())?;
    Ok(token)
}
fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone)]
struct Remote {
    engine: Arc<Engine>,
    hub: Hub,
    token: Arc<str>,
    assets: Assets,
}
pub fn router(engine: Arc<Engine>, hub: Hub, token: String, assets: Assets) -> Router {
    Router::new()
        .route("/api/events", get(events))
        .route("/api/{command}", post(command))
        .fallback(get(asset))
        .with_state(Remote {
            engine,
            hub,
            token: token.into(),
            assets,
        })
}
pub async fn serve(
    listener: TcpListener,
    engine: Arc<Engine>,
    hub: Hub,
    token: String,
    assets: Assets,
) -> std::io::Result<()> {
    axum::serve(listener, router(engine, hub, token, assets)).await
}

fn failure(status: StatusCode, error: impl Into<String>) -> Response {
    let mut response = (status, Json(json!({ "error": error.into() }))).into_response();
    no_store(&mut response);
    response
}
fn no_store(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}
/// The response refusing a request without the access token, if it has none.
fn denied(remote: &Remote, headers: &HeaderMap) -> Option<Response> {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    // Compare in constant time, so response times reveal nothing about the token.
    let expected = remote.token.as_bytes();
    let same = given.len() == expected.len()
        && given
            .bytes()
            .zip(expected)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0;
    (!same).then(|| {
        failure(
            StatusCode::UNAUTHORIZED,
            "This access link no longer works. Open Orbit's current link to reconnect.",
        )
    })
}

async fn command(
    State(remote): State<Remote>,
    Route(name): Route<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = denied(&remote, &headers) {
        return denied;
    }
    if !COMMANDS.contains(&name.as_str()) {
        return failure(StatusCode::NOT_FOUND, "This action is not available here.");
    }
    let args: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(args @ Value::Object(_)) => args,
            _ => return failure(StatusCode::BAD_REQUEST, "Arguments must be a JSON object."),
        }
    };
    match dispatch(&remote.engine, &name, &args).await {
        Ok(value) => {
            let mut response = Json(value).into_response();
            no_store(&mut response);
            response
        }
        Err(error) => failure(StatusCode::UNPROCESSABLE_ENTITY, error),
    }
}

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("Invalid {key}: {e}"))
}
fn out(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}
/// Runs a command as the desktop app's Tauri command of the same name does.
async fn dispatch(e: &Arc<Engine>, name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "get_snapshot" => out(e.shown_snapshot().await),
        "update_settings" => out(e.update_settings(arg(a, "patch")?).await?),
        "save_repository" => out(e
            .save_repository(arg(a, "repository")?, arg(a, "original")?)
            .await?),
        "set_repository_automatic" => out(e
            .set_repository_automatic(&arg::<String>(a, "name")?, arg(a, "automatic")?)
            .await?),
        "disconnect_repository" => out(e.disconnect_repository(&arg::<String>(a, "name")?).await?),
        "get_storage_usage" => out(e.storage_usage().await?),
        "remove_worktrees" => out(e.remove_worktrees(arg(a, "reviewIds")?).await),
        "export_report" => {
            let (file_name, text) = e.export_report(&arg::<String>(a, "reviewId")?).await?;
            Ok(json!({ "fileName": file_name, "text": text }))
        }
        "poll_now" => out(e.poll().await?),
        "connect_services" => {
            e.connect().await?;
            out(e.poll().await?)
        }
        "start_review" => out(e.enqueue(&arg::<String>(a, "prId")?, false).await?),
        "cancel_review" => out(e.cancel(&arg::<String>(a, "reviewId")?).await?),
        "stop_loop" => out(e.stop_loop(&arg::<String>(a, "prId")?).await?),
        "continue_loop" => out(e
            .continue_loop(&arg::<String>(a, "prId")?, arg(a, "rounds")?)
            .await?),
        "publish_review" => out(e
            .publish(
                &arg::<String>(a, "reviewId")?,
                false,
                arg(a, "allowSensitive")?,
            )
            .await?),
        "publish_plan" => out(e.publish_plan(&arg::<String>(a, "reviewId")?).await?),
        "send_message" => out(e
            .message(&arg::<String>(a, "reviewId")?, &arg::<String>(a, "text")?)
            .await?),
        "set_legacy_chat_model" => out(e
            .set_legacy_chat_model(
                &arg::<String>(a, "reviewId")?,
                arg(a, "model")?,
                arg(a, "effort")?,
            )
            .await?),
        "answer_approval" => out(e.decide(arg(a, "id")?, arg(a, "accept")?).await?),
        "get_files" => out(e.files(&arg::<String>(a, "prId")?).await?),
        "list_repositories" => out(crate::github::repositories().await?),
        _ => Err("This action is not available here.".into()),
    }
}

async fn events(State(remote): State<Remote>, headers: HeaderMap) -> Response {
    if let Some(denied) = denied(&remote, &headers) {
        return denied;
    }
    // Subscribe before reading the snapshot, so no change can fall between them.
    let mut changes = remote.hub.0.subscribe();
    changes.mark_unchanged();
    let first = match serde_json::to_string(&remote.engine.shown_snapshot().await) {
        Ok(json) => json,
        Err(e) => return failure(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let lines = futures_util::stream::unfold(
        (Some(first), changes),
        |(pending, mut changes)| async move {
            if let Some(line) = pending {
                return Some((
                    Ok::<_, std::convert::Infallible>(line + "\n"),
                    (None, changes),
                ));
            }
            tokio::select! {
                changed = changes.changed() => {
                    changed.ok()?;
                    let latest = changes.borrow_and_update().clone();
                    let line = latest.map_or_else(String::new, |json| json.to_string());
                    Some((Ok(line + "\n"), (None, changes)))
                }
                _ = tokio::time::sleep(KEEPALIVE) => Some((Ok("\n".into()), (None, changes))),
            }
        },
    );
    let mut response = Response::new(Body::from_stream(lines));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // Ask proxies to pass each line on as it is written.
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

async fn asset(State(remote): State<Remote>, uri: Uri) -> Response {
    let path = uri.path();
    // Built files never need escapes, parent segments or backslashes, so refuse
    // them rather than trust the lookup to stay inside the frontend folder.
    if path.starts_with("/api/")
        || path.contains(['%', '\\'])
        || path.split('/').any(|segment| segment == "..")
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let key = match path.trim_start_matches('/') {
        "" => "index.html",
        key => key,
    };
    let Some(file) = (remote.assets)(key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let html = file.mime_type.starts_with("text/html");
    let mut response = Response::new(Body::from(file.bytes));
    let headers = response.headers_mut();
    if let Ok(mime) = HeaderValue::from_str(&file.mime_type) {
        headers.insert(header::CONTENT_TYPE, mime);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if html {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(HTML_CSP),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    response
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
