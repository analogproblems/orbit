//! Checks with Codex that a thread runs without the extra tools you've
//! added to Codex (see `crate::extras`).
use crate::{extras, rpc::Rpc, scans};
use serde::Serialize;
use serde_json::{json, Map, Value};

/// Why a thread's extras couldn't be confirmed off.
pub(super) enum Left {
    /// Codex couldn't say which tool servers are on.
    Unsure(String),
    /// These tool servers are on.
    On(Vec<String>),
}
impl Left {
    /// What went wrong, for work that `stopped`, such as "the review didn't
    /// start".
    pub(super) fn message(&self, stopped: &str) -> String {
        match self {
            Left::Unsure(e) => format!(
                "Orbit couldn't check that Codex's extra tools are off, so {stopped}: {}",
                sentence(e)
            ),
            Left::On(names) => format!(
                "Codex kept these extra tools on, so {stopped}: {}.",
                names.join(", ")
            ),
        }
    }
}

/// Your Codex settings for a thread in `cwd`, as Codex reads them.
pub(super) async fn codex_config(rpc: &Rpc, cwd: impl Serialize) -> Result<Value, String> {
    let read = rpc
        .request("config/read", json!({"cwd": cwd}))
        .await
        .map_err(|e| format!("Codex couldn't read its settings: {e}"))?;
    Ok(read["config"].clone())
}

/// The thread settings that switch the extras off for a thread in `cwd`.
pub(super) async fn off(rpc: &Rpc, cwd: impl Serialize) -> Result<Map<String, Value>, String> {
    Ok(extras::off(&codex_config(rpc, cwd).await?))
}

/// The thread settings that switch the extras off for a scan in `cwd`, all
/// but the Codex Security plugin.
pub(super) async fn off_but_scans(
    rpc: &Rpc,
    cwd: impl Serialize,
) -> Result<Map<String, Value>, String> {
    let codex_config = codex_config(rpc, cwd).await?;
    let list = rpc.request("plugin/list", json!({})).await?;
    let keep = scans::find_plugin(&list)
        .filter(|p| p.status.installed)
        .ok_or("Install the Codex Security plugin first.")?
        .id;
    // The plugins' own servers, which only Codex knows by name.
    let servers: Vec<String> = list_servers(rpc, None)
        .await
        .map_err(|e| format!("Codex couldn't list its tool servers: {}", sentence(&e)))?
        .iter()
        .flat_map(extras::server_names)
        .collect();
    Ok(extras::off_except_plugin(
        &codex_config,
        &extras::installed_plugins(&codex_config, &list),
        &servers,
        &keep,
        scans::PLUGIN,
    ))
}

/// Checks with Codex that every tool server is off for `thread`, but `keep`.
pub(super) async fn check(rpc: &Rpc, thread: &str, keep: Option<&str>) -> Result<(), Left> {
    let on: Vec<String> = list_servers(rpc, Some(thread))
        .await
        .map_err(Left::Unsure)?
        .iter()
        .flat_map(extras::servers_on)
        .filter(|name| Some(name.as_str()) != keep)
        .collect();
    if on.is_empty() {
        Ok(())
    } else {
        Err(Left::On(on))
    }
}

/// Each page of the tool servers Codex lists for `thread`, or for Codex
/// itself.
async fn list_servers(rpc: &Rpc, thread: Option<&str>) -> Result<Vec<Value>, String> {
    let mut pages = vec![];
    let mut cursor = Value::Null;
    for _ in 0..20 {
        let page = rpc
            .request(
                "mcpServerStatus/list",
                json!({"threadId": thread, "detail": "toolsAndAuthOnly", "cursor": cursor}),
            )
            .await?;
        if !page["data"].is_array() {
            return Err("Codex didn't list them.".into());
        }
        cursor = page["nextCursor"].clone();
        pages.push(page);
        if cursor.is_null() {
            return Ok(pages);
        }
    }
    Err("Codex listed too many.".into())
}

/// Text from elsewhere as a sentence, ending in a full stop if it has no
/// ending of its own.
pub(super) fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?']) {
        text.into()
    } else {
        format!("{text}.")
    }
}
