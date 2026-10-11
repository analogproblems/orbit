//! The extra tools you can add to Codex yourself: tool servers, plugins,
//! connected apps, the browser, the computer, tool suggestions, memories and
//! image generation. Codex reviews, follow-up chats, finding extraction and
//! scans run with them off, on top of your Codex settings, which stay as
//! they are. A scan keeps only the Codex Security plugin it runs.
use serde_json::{json, Map, Value};

/// Codex's features that add extras to a thread.
pub const FEATURES_OFF: [&str; 7] = [
    "plugins",
    "apps",
    "browser_use",
    "computer_use",
    "tool_suggest",
    "memories",
    "image_generation",
];

/// The thread settings that switch the extras off: each tool server your
/// Codex settings name (`codex_config`, as Codex reads them for the thread's
/// folder) and the features in `FEATURES_OFF`.
pub fn off(codex_config: &Value) -> Map<String, Value> {
    off_except(codex_config, None)
}

/// Like `off`, but keeps the plugin `keep` (its id) and its tool server
/// `keep_server` on. Plugins stay on, each other plugin in `plugins` is off,
/// and so is each tool server in `servers` but `keep_server` within them:
/// Codex ignores a thread's switch for a plugin installed from your
/// account, but not for that plugin's servers.
pub fn off_except_plugin(
    codex_config: &Value,
    plugins: &[String],
    servers: &[String],
    keep: &str,
    keep_server: &str,
) -> Map<String, Value> {
    let plugin_servers: Map<String, Value> = servers
        .iter()
        .filter(|name| *name != keep_server)
        .map(|name| (name.clone(), json!({"enabled": false})))
        .collect();
    let plugins: Map<String, Value> = plugins
        .iter()
        .filter(|id| *id != keep)
        .map(|id| {
            (
                id.clone(),
                json!({"enabled": false, "mcp_servers": plugin_servers}),
            )
        })
        .collect();
    let mut config = off_except(codex_config, Some(("plugins", keep_server)));
    config.insert("plugins".into(), plugins.into());
    config
}

/// `off`, keeping a feature and a tool server on.
fn off_except(codex_config: &Value, kept: Option<(&str, &str)>) -> Map<String, Value> {
    let (kept_feature, kept_server) = kept.unzip();
    let servers: Map<String, Value> = codex_config["mcp_servers"]
        .as_object()
        .into_iter()
        .flat_map(|servers| servers.keys())
        .filter(|name| Some(name.as_str()) != kept_server)
        .map(|name| (name.clone(), json!({"enabled": false})))
        .collect();
    let features: Map<String, Value> = FEATURES_OFF
        .iter()
        .filter(|f| Some(**f) != kept_feature)
        .map(|f| (f.to_string(), json!(false)))
        .collect();
    let mut config = Map::new();
    config.insert("mcp_servers".into(), servers.into());
    config.insert("features".into(), features.into());
    config
}

/// The plugins installed in Codex: those your Codex settings name and those
/// a `plugin/list` response shows as installed.
pub fn installed_plugins(codex_config: &Value, list: &Value) -> Vec<String> {
    let mut ids: Vec<String> = codex_config["plugins"]
        .as_object()
        .into_iter()
        .flat_map(|plugins| plugins.keys().cloned())
        .collect();
    let listed = list["marketplaces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|market| market["plugins"].as_array().into_iter().flatten())
        .filter(|plugin| plugin["installed"] == true)
        .filter_map(|plugin| plugin["id"].as_str());
    for id in listed {
        if !ids.iter().any(|known| known == id) {
            ids.push(id.into());
        }
    }
    ids
}

/// The names of the tool servers in a page of a server list.
pub fn server_names(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|server| server["name"].as_str().unwrap_or("unnamed").to_string())
        .collect()
}

/// The tool servers in a page of a thread's server list that aren't
/// switched off for it, from plugins and apps as well as your settings.
pub fn servers_on(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|server| server["runtimeStatus"] != "disabled")
        .map(|server| server["name"].as_str().unwrap_or("unnamed").to_string())
        .collect()
}

#[cfg(test)]
#[path = "extras_tests.rs"]
mod tests;
