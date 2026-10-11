use super::*;
use serde_json::json;

#[test]
fn a_thread_turns_off_every_tool_server_and_extra_feature() {
    let config = off(
        &json!({"mcp_servers": {"notes": {"command": "x"}, "mail": {"url": "https://example.com"}}}),
    );
    assert_eq!(
        config["mcp_servers"],
        json!({"notes": {"enabled": false}, "mail": {"enabled": false}})
    );
    for feature in FEATURES_OFF {
        assert_eq!(config["features"][feature], false, "{feature}");
    }
    assert_eq!(config["features"].as_object().unwrap().len(), 7);
    // Only extras: Codex's standard tools and your other settings stay.
    assert_eq!(config.len(), 2);
    assert_eq!(off(&json!({}))["mcp_servers"], json!({}));
}

#[test]
fn a_scan_keeps_only_its_plugin_and_its_server() {
    let codex_config = json!({
        "mcp_servers": {"notes": {"command": "x"}, "codex-security": {"command": "y"}},
        "plugins": {"codex-security@market": {"enabled": true}, "figma@market": {"enabled": true}},
    });
    let list = json!({"marketplaces": [
        {"name": "market", "plugins": [
            {"id": "codex-security@market", "installed": true},
            {"id": "figma@market", "installed": true},
            {"id": "offered@market", "installed": false},
        ]},
        {"name": "account", "plugins": [{"id": "linear@account", "installed": true}]},
    ]});
    let plugins = installed_plugins(&codex_config, &list);
    assert_eq!(
        plugins,
        ["codex-security@market", "figma@market", "linear@account"]
    );
    let servers = ["notes", "figma", "linear", "codex-security"].map(String::from);
    let config = off_except_plugin(
        &codex_config,
        &plugins,
        &servers,
        "codex-security@market",
        "codex-security",
    );
    assert_eq!(config["mcp_servers"], json!({"notes": {"enabled": false}}));
    assert_eq!(config["features"].get("plugins"), None);
    assert_eq!(config["features"]["apps"], false);
    // Every other plugin is off, and so are its servers, which Codex still
    // honours for a plugin installed from your account.
    let others = json!({"enabled": false, "mcp_servers": {
        "notes": {"enabled": false}, "figma": {"enabled": false}, "linear": {"enabled": false},
    }});
    assert_eq!(
        config["plugins"],
        json!({"figma@market": others, "linear@account": others})
    );
}

#[test]
fn a_server_counts_as_on_unless_codex_says_it_is_off() {
    let page = json!({"data": [
        {"name": "notes", "runtimeStatus": "disabled"},
        {"name": "mail", "runtimeStatus": "starting"},
        {"name": "apps", "runtimeStatus": null},
    ]});
    assert_eq!(servers_on(&page), ["mail", "apps"]);
    assert_eq!(server_names(&page), ["notes", "mail", "apps"]);
    assert!(servers_on(&json!({})).is_empty());
}
