use super::{scans::apply_scan_notification, test_support::git_repo, Engine};
use crate::{models::tests::catalog, rpc::Rpc, types::*};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc;

const FINDINGS: &str = include_str!("../scan_fixture/findings.json");

fn plugin_list(installed: bool) -> Value {
    json!({"marketplaces": [{"name": "openai-curated", "path": "/m/curated.json", "plugins": [
        {"id": "codex-security@openai-curated", "name": "codex-security", "installed": installed, "version": "0.1.24"}
    ]}]})
}
/// Shaped like Codex's `skills/list`: the plugin's skills carry its id, and
/// a skill of the same name from elsewhere does not.
fn skills_list() -> Value {
    json!({"data": [{"cwd": "/", "errors": [], "skills": [
        {"name": "security-scan", "path": "/elsewhere/security-scan/SKILL.md", "scope": "user", "enabled": true},
        {"name": "codex-security:security-scan", "path": "/p/skills/security-scan/SKILL.md",
         "scope": "user", "enabled": true, "pluginId": "codex-security@openai-curated"},
        {"name": "codex-security:deep-security-scan", "path": "/p/skills/deep-security-scan/SKILL.md",
         "scope": "user", "enabled": true, "pluginId": "codex-security@openai-curated"},
    ]}]})
}
/// Fixture options for a Codex with extras: a tool server of your own and
/// another plugin with a server of its own.
fn extras() -> Value {
    let mut list = plugin_list(true);
    list["marketplaces"][0]["plugins"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "figma@openai-curated", "name": "figma", "installed": true}));
    json!({
        "responses": {"plugin/list": list},
        "mcpServers": {"notes": {"command": "notes"}},
        "mcpGlobal": {"data": [
            {"name": "notes"}, {"name": "codex-security"}, {"name": "figma"},
        ], "nextCursor": null},
        "mcpStatus": {"data": [
            {"name": "notes", "runtimeStatus": "disabled"},
            {"name": "codex-security", "runtimeStatus": "connected"},
            {"name": "figma", "runtimeStatus": "disabled"},
        ], "nextCursor": null},
    })
}
/// Connects an engine to the fixture Codex.
async fn attach(e: &Arc<Engine>) -> Arc<Rpc> {
    let (tx, rx) = mpsc::channel(64);
    let mut cmd = tokio::process::Command::new("node");
    cmd.args(["-e", include_str!("../model_fixture.cjs")]);
    let rpc = Rpc::from_command(cmd, tx).await.unwrap();
    *e.rpc.lock().await = Some(rpc.clone());
    tokio::spawn(e.clone().pump(rpc.clone(), rx));
    rpc
}
/// A real worktree of a fresh repository at a scan's copy.
async fn scan_copy(e: &Engine, id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let (repo_dir, repo) = git_repo().await;
    let worktree = e.scan_worktree(id);
    std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    super::worktrees::git(
        &repo,
        &["worktree", "add", "--detach", worktree.to_str().unwrap()],
    )
    .await
    .unwrap();
    (repo_dir, worktree)
}
async fn fixture() -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>) {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(dir.path().join("state"), |_| {}).unwrap();
    let rpc = attach(&e).await;
    let checkout = dir.path().to_string_lossy().into_owned();
    e.update(|s| {
        s.settings.model = "model-a".into();
        s.settings.effort = "high".into();
        s.codex_ready = true;
        s.model_catalog_ready = true;
        s.models = catalog();
        s.settings.repositories.push(Repository {
            name: "a/b".into(),
            path: checkout,
            automatic: false,
            setup_command: String::new(),
            network_access: true,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Default::default(),
        });
    })
    .await;
    configure(&rpc, json!({})).await;
    (dir, e, rpc)
}
async fn configure(rpc: &Rpc, options: Value) {
    rpc.request("fixture/configure", options).await.unwrap();
}
async fn requests(rpc: &Rpc) -> Vec<Value> {
    rpc.request("fixture/requests", json!({}))
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}
async fn wait_for(e: &Engine, done: impl Fn(&Snapshot) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done(&e.snapshot().await) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed out");
}
/// A scan whose thread is running, as `start_scan_thread` leaves it.
fn running(id: &str, worktree: Option<&Path>) -> SecurityScan {
    SecurityScan {
        id: id.into(),
        repo: "a/b".into(),
        mode: "standard".into(),
        status: "running".into(),
        created_at: now(),
        commit: Some("a".repeat(40)),
        model: "model-a".into(),
        effort: "high".into(),
        thread_id: Some("scan-thread".into()),
        turn_id: Some("scan-turn".into()),
        worktree: worktree.map(|w| w.to_string_lossy().into()),
        ..SecurityScan::default()
    }
}
fn notification(method: &str, params: Value) -> Value {
    json!({"method": method, "params": params})
}
fn tool_item(tool: &str, arguments: Value, result: Value) -> Value {
    json!({"id": "item", "type": "mcpToolCall", "server": "codex-security", "tool": tool,
        "status": "completed", "arguments": arguments, "result": {"content": [], "structuredContent": result}})
}

#[tokio::test]
async fn checks_and_installs_the_plugin_only_when_asked() {
    let (_d, e, rpc) = fixture().await;
    configure(
        &rpc,
        json!({"responses": {"plugin/list": plugin_list(false)}}),
    )
    .await;
    let status = e.scan_plugin().await.unwrap();
    assert!(status.available && !status.installed);
    assert!(!requests(&rpc)
        .await
        .iter()
        .any(|r| r["method"] == "plugin/install"));
    // Starting a scan never installs it.
    let error = e.start_scan("a/b", "standard").await.unwrap_err();
    assert!(
        error.contains("Install the Codex Security plugin"),
        "{error}"
    );
    assert!(e.snapshot().await.scans.is_empty());
    e.install_scan_plugin().await.unwrap();
    let install = requests(&rpc)
        .await
        .into_iter()
        .find(|r| r["method"] == "plugin/install")
        .unwrap();
    assert_eq!(
        install["params"],
        json!({"marketplacePath": "/m/curated.json", "pluginName": "codex-security"})
    );
    configure(
        &rpc,
        json!({"responses": {"plugin/list": {"marketplaces": []}}}),
    )
    .await;
    assert!(!e.scan_plugin().await.unwrap().available);
    assert!(e.install_scan_plugin().await.is_err());
}

#[tokio::test]
async fn uses_a_plugin_from_a_remote_marketplace_by_its_remote_id() {
    let (_d, e, rpc) = fixture().await;
    let remote = |installed: bool| {
        json!({"marketplaces": [{"name": "openai-curated", "path": null, "plugins": [
            {"id": "codex-security@openai-curated", "remotePluginId": "plugin_6f1c2a", "name": "codex-security",
             "installed": installed, "version": "0.1.24"}
        ]}]})
    };
    configure(&rpc, json!({"responses": {"plugin/list": remote(false)}})).await;
    e.install_scan_plugin().await.unwrap();
    let install = requests(&rpc)
        .await
        .into_iter()
        .find(|r| r["method"] == "plugin/install")
        .unwrap();
    assert_eq!(
        install["params"],
        json!({"remoteMarketplaceName": "openai-curated", "pluginName": "plugin_6f1c2a"})
    );
    // Its skills have no path in plugin/read, so they come from skills/list,
    // where Codex loads them from its local copy.
    configure(
        &rpc,
        json!({"responses": {"plugin/list": remote(true), "skills/list": skills_list(),
            "plugin/read": {"plugin": {"skills": [{"name": "codex-security:security-scan", "path": null}]}}}}),
    )
    .await;
    let id = e.start_scan("a/b", "standard").await.unwrap();
    assert!(id.starts_with("scan-"));
    let list = requests(&rpc)
        .await
        .into_iter()
        .find(|r| r["method"] == "skills/list")
        .unwrap();
    assert_eq!(list["params"], json!({"cwds": [], "forceReload": true}));
    wait_for(&e, |s| !scan_active(&s.scans[0].status)).await;
    // Without the plugin's own skill, no scan starts.
    e.update(|s| s.scans.clear()).await;
    let mut other = skills_list();
    other["data"][0]["skills"] = json!([other["data"][0]["skills"][0]]);
    configure(
        &rpc,
        json!({"responses": {"plugin/list": remote(true), "skills/list": other}}),
    )
    .await;
    let error = e.start_scan("a/b", "standard").await.unwrap_err();
    assert!(error.contains("scan skill"), "{error}");
    assert!(e.snapshot().await.scans.is_empty());
}

#[tokio::test]
async fn runs_one_scan_at_a_time_and_reports_a_checkout_problem() {
    let (_d, e, rpc) = fixture().await;
    configure(
        &rpc,
        json!({"responses": {"plugin/list": plugin_list(true), "skills/list": skills_list()}}),
    )
    .await;
    assert!(e.start_scan("a/b", "diff").await.is_err());
    assert!(e.start_scan("c/d", "standard").await.is_err());
    e.update(|s| s.scans.push(running("scan-0", None))).await;
    let error = e.start_scan("a/b", "standard").await.unwrap_err();
    assert!(error.contains("already running"), "{error}");
    e.update(|s| s.scans.clear()).await;
    // The fixture's checkout is not a clone of a/b, so preparation fails.
    let id = e.start_scan("A/B", "deep").await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(
        (s.scans[0].repo.as_str(), s.scans[0].mode.as_str()),
        ("a/b", "deep")
    );
    wait_for(&e, |s| s.scans[0].status == "failed").await;
    let s = e.snapshot().await;
    let scan = s.scans.iter().find(|x| x.id == id).unwrap();
    assert!(scan.error.is_some() && scan.finished_at.is_some());
    assert!(!requests(&rpc)
        .await
        .iter()
        .any(|r| r["method"] == "thread/start"));
}

#[tokio::test]
async fn starts_the_skill_on_the_scan_copy_with_orbits_model() {
    let (d, e, rpc) = fixture().await;
    let worktree = d.path().join("copy");
    std::fs::create_dir_all(&worktree).unwrap();
    let mut scan = running("scan-1", Some(&worktree));
    scan.status = "preparing".into();
    scan.thread_id = None;
    scan.turn_id = None;
    e.update(|s| s.scans.push(scan)).await;
    configure(&rpc, extras()).await;
    e.start_scan_thread(
        "scan-1",
        &worktree,
        "codex-security:security-scan",
        "/p/SKILL.md",
    )
    .await
    .unwrap();
    let requests = requests(&rpc).await;
    let thread = requests
        .iter()
        .find(|r| r["method"] == "thread/start")
        .unwrap();
    // Of your extras, only Codex Security stays on, which Codex confirms.
    let read = requests
        .iter()
        .find(|r| r["method"] == "config/read")
        .unwrap();
    assert_eq!(read["params"]["cwd"], json!(worktree));
    let config = &thread["params"]["config"];
    assert_eq!(config["mcp_servers"], json!({"notes": {"enabled": false}}));
    assert_eq!(config["features"]["apps"], false);
    assert_eq!(config["features"].get("plugins"), None);
    assert_eq!(
        config["plugins"],
        json!({"figma@openai-curated": {"enabled": false, "mcp_servers": {
            "notes": {"enabled": false}, "figma": {"enabled": false},
        }}})
    );
    let checked = requests
        .iter()
        .filter(|r| r["method"] == "mcpServerStatus/list")
        .collect::<Vec<_>>();
    assert_eq!(checked[0]["params"]["threadId"], Value::Null);
    assert_eq!(checked[1]["params"]["threadId"], "fixture-thread");
    assert_eq!(thread["params"]["approvalPolicy"], "never");
    assert_eq!(thread["params"]["model"], "model-a");
    assert_eq!(thread["params"]["config"]["model_reasoning_effort"], "high");
    assert_eq!(
        thread["params"]["config"]["sandbox_workspace_write"]["network_access"],
        false
    );
    assert_eq!(
        thread["params"]["config"]["sandbox_workspace_write"]["writable_roots"][0],
        json!(e.scan_root())
    );
    let turn = requests
        .iter()
        .find(|r| r["method"] == "turn/start")
        .unwrap();
    assert_eq!(
        turn["params"]["input"][1],
        json!({"type": "skill", "name": "codex-security:security-scan", "path": "/p/SKILL.md"})
    );
    let text = turn["params"]["input"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Run a Codex Security scan on this repository."));
    assert!(text.contains(&"a".repeat(40)));
    let s = e.snapshot().await;
    assert_eq!(s.scans[0].status, "running");
    assert_eq!(s.scans[0].thread_id.as_deref(), Some("fixture-thread"));
    assert_eq!(s.scans[0].turn_id.as_deref(), Some("fixture-turn"));
}

#[tokio::test]
async fn a_scan_does_not_start_while_another_extra_is_on() {
    let (d, e, rpc) = fixture().await;
    let worktree = d.path().join("copy");
    std::fs::create_dir_all(&worktree).unwrap();
    let mut scan = running("scan-1", Some(&worktree));
    scan.status = "preparing".into();
    scan.thread_id = None;
    scan.turn_id = None;
    e.update(|s| s.scans.push(scan)).await;
    let mut options = extras();
    options["mcpStatus"] = json!({"data": [
        {"name": "codex-security", "runtimeStatus": "connected"},
        {"name": "figma", "runtimeStatus": "connected"},
    ], "nextCursor": null});
    configure(&rpc, options).await;
    let error = e
        .start_scan_thread(
            "scan-1",
            &worktree,
            "codex-security:security-scan",
            "/p/SKILL.md",
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "Codex kept these extra tools on, so the scan didn't start: figma."
    );
    let methods: Vec<Value> = requests(&rpc)
        .await
        .iter()
        .map(|r| r["method"].clone())
        .collect();
    assert!(methods.contains(&json!("thread/archive")));
    assert!(!methods.contains(&json!("turn/start")));
    assert_eq!(e.snapshot().await.scans[0].status, "preparing");
}

#[tokio::test]
async fn follows_progress_and_imports_the_finished_scan() {
    let (d, e, _rpc) = fixture().await;
    let (_repo_dir, repo) = git_repo().await;
    let worktree = e.scan_worktree("scan-1");
    std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    super::worktrees::git(
        &repo,
        &["worktree", "add", "--detach", worktree.to_str().unwrap()],
    )
    .await
    .unwrap();
    e.update(|s| s.scans.push(running("scan-1", Some(&worktree))))
        .await;
    let plugin_dir = e.scan_root().join("plugin-scan");
    std::fs::create_dir_all(plugin_dir.join("artifacts/02_discovery")).unwrap();
    std::fs::write(
        plugin_dir.join("artifacts/02_discovery/big.json"),
        "x".repeat(4096),
    )
    .unwrap();
    std::fs::write(plugin_dir.join("findings.json"), FINDINGS).unwrap();
    std::fs::write(plugin_dir.join("report.md"), "# Scan report").unwrap();
    crate::scans::seal(&plugin_dir, "plugin-id", &"a".repeat(40), "completed");
    let thread = json!({"threadId": "scan-thread", "turnId": "scan-turn"});
    let item = |item: Value| {
        let mut p = thread.clone();
        p["item"] = item;
        notification("item/completed", p)
    };
    e.event(item(tool_item(
        "start_codex_security_standard_scan",
        json!({"targetPath": "."}),
        json!({"scanId": "plugin-id", "scanDir": plugin_dir}),
    )))
    .await;
    e.event(item(tool_item(
        "update_codex_security_scan_progress",
        json!({"phase": "discovery", "reviewItemsCompleted": 2, "reviewItemsTotal": 5}),
        json!({}),
    )))
    .await;
    e.event(item(
        json!({"id": "m", "type": "agentMessage", "text": "Reviewing the upload handler."}),
    ))
    .await;
    // A thread that is not the scan's changes nothing.
    e.event(notification(
        "item/completed",
        json!({"threadId": "other", "item": {"type": "agentMessage", "text": "x"}}),
    ))
    .await;
    let s = e.snapshot().await;
    let scan = &s.scans[0];
    assert_eq!(scan.plugin_scan_id.as_deref(), Some("plugin-id"));
    assert_eq!(scan.scan_dir.as_deref(), Some(plugin_dir.to_str().unwrap()));
    let progress = scan.progress.as_ref().unwrap();
    assert_eq!((progress.completed, progress.total), (Some(2), Some(5)));
    assert_eq!(
        scan.last_message.as_deref(),
        Some("Reviewing the upload handler.")
    );

    e.event(notification(
        "turn/completed",
        json!({"threadId": "scan-thread", "turn": {"id": "scan-turn", "status": "completed"}}),
    ))
    .await;
    wait_for(&e, |s| s.scans[0].status == "completed").await;
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.findings.len(), 1);
    assert_eq!(scan.findings[0].id, "scan-1:csf_orbit_fixture_0001");
    assert!(scan.has_report && scan.finished_at.is_some());
    assert!(scan.disk_bytes.unwrap() > 0);
    assert!(!plugin_dir.join("artifacts").exists());
    assert_eq!(e.scan_report("scan-1").await.unwrap(), "# Scan report");
    assert!(scan.worktree.is_none() && !worktree.exists());

    // Deleting removes the findings, the report and the plugin's folder.
    e.delete_scan("scan-1").await.unwrap();
    assert!(e.snapshot().await.scans.is_empty());
    assert!(!plugin_dir.exists());
    assert!(!e.scan_results("scan-1").exists());
    assert!(e.scan_report("scan-1").await.is_err());
    drop(d);
}

#[tokio::test]
async fn a_scan_without_results_or_with_a_failed_turn_fails() {
    let (_d, e, _rpc) = fixture().await;
    e.update(|s| s.scans.push(running("scan-1", None))).await;
    e.event(notification(
        "turn/completed",
        json!({"threadId": "scan-thread", "turn": {"id": "scan-turn", "status": "completed"}}),
    ))
    .await;
    wait_for(&e, |s| s.scans[0].status == "failed").await;
    assert!(e.snapshot().await.scans[0]
        .error
        .as_deref()
        .unwrap()
        .contains("did not record this scan as finished: it wrote no scan-manifest.json"));

    let mut s = Snapshot::default();
    s.scans.push(running("scan-2", None));
    let limit = json!({"turn": {"id": "scan-turn", "status": "failed",
        "error": {"message": "limit", "codexErrorInfo": "usageLimitExceeded"}}});
    // A failed turn first stops the plugin's scan, then fails.
    assert_eq!(
        apply_scan_notification(&mut s, "scan-2", "turn/completed", &limit).as_deref(),
        Some("stop")
    );
    assert_eq!(s.scans[0].status, "cancelling");
    assert_eq!(s.scans[0].stop_outcome.as_deref(), Some("failed"));
    assert!(s.scans[0]
        .stop_reason
        .as_deref()
        .unwrap()
        .contains("usage limit"));
    // Another turn on the thread does not end the scan.
    s.scans.push(running("scan-3", None));
    let other = json!({"turn": {"id": "cancel-turn", "status": "completed"}});
    assert_eq!(
        apply_scan_notification(&mut s, "scan-3", "turn/completed", &other),
        None
    );
    assert_eq!(s.scans[1].status, "running");
}

/// A `list_codex_security_scans` reply with one plugin scan on scan-1's copy.
fn plugin_scan(status: &str) -> Value {
    json!({"content": [], "structuredContent": {"scans": [
        {"scanId": "deep-1", "targetPath": "/o/scan-worktrees/scan-1", "progress": {"status": status}},
        {"scanId": "other", "targetPath": "/o/scan-worktrees/scan-2", "progress": {"status": "running"}},
    ]}})
}
fn tool_calls(requests: &[Value]) -> Vec<Value> {
    requests
        .iter()
        .filter(|r| r["method"] == "mcpServer/tool/call")
        .map(|r| r["params"].clone())
        .collect()
}
async fn wait_releasing(e: &Engine, rpc: &Rpc, done: impl Fn(&Snapshot) -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done(&e.snapshot().await) {
            rpc.request("fixture/release", json!({})).await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed out");
}

#[tokio::test]
async fn cancelling_waits_until_the_plugin_confirms_its_scan_stopped() {
    let (_d, e, rpc) = fixture().await;
    configure(
        &rpc,
        json!({
            "holdMethod": "mcpServer/tool/call",
            "responses": {"plugin/list": plugin_list(true), "skills/list": skills_list()},
            "toolResponses": {"list_codex_security_scans": [plugin_scan("running"), plugin_scan("canceled")]},
        }),
    )
    .await;
    e.update(|s| s.scans.push(running("scan-1", None))).await;
    e.cancel_scan("scan-1").await.unwrap();
    assert_eq!(e.snapshot().await.scans[0].status, "cancelling");
    // While the plugin has not confirmed, the scan keeps its slot and files.
    let error = e.start_scan("a/b", "standard").await.unwrap_err();
    assert!(error.contains("already running"), "{error}");
    let error = e.delete_scan("scan-1").await.unwrap_err();
    assert!(error.contains("still stopping"), "{error}");
    // A second Cancel waits for the first.
    e.cancel_scan("scan-1").await.unwrap();
    // The interrupted turn ending does not end the scan, and its progress
    // is still followed.
    e.event(notification(
        "turn/completed",
        json!({"threadId": "scan-thread", "turn": {"id": "scan-turn", "status": "interrupted"}}),
    ))
    .await;
    e.event(notification(
        "item/completed",
        json!({"threadId": "scan-thread", "turnId": "scan-turn", "item": tool_item(
            "update_codex_security_scan_progress",
            json!({"phase": "validation", "reviewItemsCompleted": 4, "reviewItemsTotal": 9}),
            json!({}),
        )}),
    ))
    .await;
    let s = e.snapshot().await;
    assert_eq!(s.scans[0].status, "cancelling");
    assert_eq!(s.scans[0].progress.as_ref().unwrap().completed, Some(4));

    wait_releasing(&e, &rpc, |s| s.scans[0].status == "cancelled").await;
    let requests = requests(&rpc).await;
    let interrupts: Vec<_> = requests
        .iter()
        .filter(|r| r["method"] == "turn/interrupt")
        .collect();
    assert_eq!(interrupts.len(), 1);
    assert_eq!(
        interrupts[0]["params"],
        json!({"threadId": "scan-thread", "turnId": "scan-turn"})
    );
    // No message asks the model to cancel; Orbit calls the plugin itself on
    // the scan's thread, cancels only this scan's plugin scan, and checks.
    assert!(!requests.iter().any(|r| r["method"] == "turn/start"));
    let calls = tool_calls(&requests);
    let tools: Vec<_> = calls.iter().map(|c| c["tool"].as_str().unwrap()).collect();
    assert_eq!(
        tools,
        [
            "list_codex_security_scans",
            "cancel_codex_security_scan",
            "list_codex_security_scans"
        ]
    );
    assert_eq!(
        calls[0],
        json!({"threadId": "scan-thread", "server": "codex-security",
            "tool": "list_codex_security_scans", "arguments": {"query": "scan-1"}})
    );
    assert_eq!(calls[1]["arguments"], json!({"scanId": "deep-1"}));
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.plugin_scan_id.as_deref(), Some("deep-1"));
    assert!(scan.finished_at.is_some() && scan.error.is_none());
    assert!(e.cancel_scan("scan-1").await.is_err());
    e.delete_scan("scan-1").await.unwrap();
}

#[tokio::test]
async fn a_scan_counts_as_completed_only_with_the_plugins_sealed_record() {
    let (_d, e, rpc) = fixture().await;
    configure(
        &rpc,
        json!({"toolResponses": {"list_codex_security_scans": plugin_scan("complete")}}),
    )
    .await;
    let plugin_dir = e.scan_root().join("deep");
    std::fs::create_dir_all(plugin_dir.join("artifacts")).unwrap();
    let mut scan = running("scan-1", None);
    scan.mode = "deep".into();
    scan.scan_dir = Some(plugin_dir.to_string_lossy().into());
    e.update(|s| s.scans.push(scan)).await;
    let done = notification(
        "turn/completed",
        json!({"threadId": "scan-thread", "turn": {"id": "scan-turn", "status": "completed"}}),
    );

    // A turn that ended after writing only an empty draft is not a
    // completed scan with no findings.
    std::fs::write(plugin_dir.join("findings.json"), r#"{"findings": []}"#).unwrap();
    e.event(done.clone()).await;
    wait_for(&e, |s| !scan_active(&s.scans[0].status)).await;
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "failed");
    assert!(scan
        .error
        .unwrap()
        .contains("did not record this scan as finished"));
    assert!(plugin_dir.join("artifacts").exists());

    // A deep scan's id comes from the plugin's own list of scans on the copy,
    // and only its sealed manifest counts.
    std::fs::write(plugin_dir.join("findings.json"), FINDINGS).unwrap();
    crate::scans::seal(&plugin_dir, "deep-1", &"a".repeat(40), "completed");
    e.update(|s| {
        s.scans[0].status = "running".into();
        s.scans[0].error = None;
    })
    .await;
    e.event(done).await;
    wait_for(&e, |s| s.scans[0].status == "completed").await;
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.plugin_scan_id.as_deref(), Some("deep-1"));
    assert_eq!(scan.findings.len(), 1);
    assert!(!plugin_dir.join("artifacts").exists());
    let list = tool_calls(&requests(&rpc).await);
    assert_eq!(
        list.last().unwrap()["arguments"],
        json!({"query": "scan-1"})
    );
}

#[tokio::test]
async fn a_cancel_the_plugin_does_not_confirm_stays_pending_and_can_be_retried() {
    let (_d, e, rpc) = fixture().await;
    e.update(|s| s.scans.push(running("scan-1", None))).await;
    let error_after_cancel = |e: Arc<Engine>| async move {
        e.cancel_scan("scan-1").await.unwrap();
        wait_for(&e, |s| s.scans[0].error.is_some()).await;
        let scan = e.snapshot().await.scans[0].clone();
        assert_eq!(scan.status, "cancelling");
        assert!(scan.finished_at.is_none());
        scan.error.unwrap()
    };

    // Codex does not stop the turn.
    configure(&rpc, json!({"failMethod": "turn/interrupt"})).await;
    let error = error_after_cancel(e.clone()).await;
    assert!(
        error.contains("could not confirm") && error.contains("fixture failure"),
        "{error}"
    );
    assert!(tool_calls(&requests(&rpc).await).is_empty());
    assert!(e.delete_scan("scan-1").await.is_err());

    // The plugin refuses to cancel.
    configure(
        &rpc,
        json!({
            "errors": {"turn/interrupt": "no active turn to interrupt"},
            "toolResponses": {
                "list_codex_security_scans": plugin_scan("running"),
                "cancel_codex_security_scan": {"isError": true, "content": [{"type": "text", "text": "Scan is owned by another thread."}]},
            },
        }),
    )
    .await;
    let error = error_after_cancel(e.clone()).await;
    assert!(error.contains("owned by another thread"), "{error}");

    // The plugin accepts, but still reports its scan running.
    configure(
        &rpc,
        json!({
            "errors": {"turn/interrupt": "no active turn to interrupt"},
            "toolResponses": {"list_codex_security_scans": plugin_scan("running")},
        }),
    )
    .await;
    let error = error_after_cancel(e.clone()).await;
    assert!(error.contains("still reports its scan deep-1"), "{error}");

    // Listing fails.
    configure(
        &rpc,
        json!({
            "errors": {"turn/interrupt": "no active turn to interrupt"},
            "toolResponses": {"list_codex_security_scans": {"error": "tool server stopped"}},
        }),
    )
    .await;
    let error = error_after_cancel(e.clone()).await;
    assert!(error.contains("tool server stopped"), "{error}");

    // Once the plugin reports its scan stopped, the cancel completes.
    configure(
        &rpc,
        json!({
            "errors": {"turn/interrupt": "no active turn to interrupt"},
            "toolResponses": {"list_codex_security_scans": plugin_scan("complete")},
        }),
    )
    .await;
    e.cancel_scan("scan-1").await.unwrap();
    wait_for(&e, |s| s.scans[0].status == "cancelled").await;
    assert!(e.snapshot().await.scans[0].error.is_none());
    let tools: Vec<_> = tool_calls(&requests(&rpc).await)
        .iter()
        .map(|c| c["tool"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        tools,
        ["list_codex_security_scans", "list_codex_security_scans"]
    );
}

#[tokio::test]
async fn cancelling_a_scan_being_prepared_stops_it_at_once() {
    let (_d, e, rpc) = fixture().await;
    let mut scan = running("scan-1", None);
    scan.status = "preparing".into();
    scan.thread_id = None;
    scan.turn_id = None;
    e.update(|s| s.scans.push(scan)).await;
    e.cancel_scan("scan-1").await.unwrap();
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "cancelled");
    assert!(scan.finished_at.is_some());
    assert!(requests(&rpc).await.is_empty());
}

#[tokio::test]
async fn a_turn_that_ends_while_the_plugin_still_runs_keeps_the_copy_until_it_stops() {
    let (_d, e, rpc) = fixture().await;
    let (_repo, worktree) = scan_copy(&e, "scan-1").await;
    e.update(|s| s.scans.push(running("scan-1", Some(&worktree))))
        .await;
    configure(
        &rpc,
        json!({
            "holdMethod": "mcpServer/tool/call",
            "responses": {"plugin/list": plugin_list(true), "skills/list": skills_list()},
            "toolResponses": {"list_codex_security_scans": [
                plugin_scan("running"), plugin_scan("running"), plugin_scan("canceled"),
            ]},
        }),
    )
    .await;
    // The turn ends without a sealed manifest while the plugin's deep scan
    // still runs on the copy.
    e.event(notification(
        "turn/completed",
        json!({"threadId": "scan-thread", "turn": {"id": "scan-turn", "status": "completed"}}),
    ))
    .await;
    wait_releasing(&e, &rpc, |s| s.scans[0].status == "cancelling").await;
    let scan = e.snapshot().await.scans[0].clone();
    assert!(scan.stop_reason.unwrap().contains("deep-1 as running"));
    assert!(worktree.exists() && scan.worktree.is_some());
    let error = e.start_scan("a/b", "standard").await.unwrap_err();
    assert!(error.contains("already running"), "{error}");
    assert!(e.delete_scan("scan-1").await.is_err());
    assert!(worktree.exists());

    // Only once the plugin confirms its scan stopped does the scan fail and
    // its copy go.
    wait_releasing(&e, &rpc, |s| {
        s.scans[0].status == "failed" && s.scans[0].worktree.is_none()
    })
    .await;
    let scan = e.snapshot().await.scans[0].clone();
    assert!(scan.error.unwrap().contains("deep-1 as running"));
    assert!(scan.worktree.is_none() && !worktree.exists());
    let tools: Vec<_> = tool_calls(&requests(&rpc).await)
        .iter()
        .map(|c| c["tool"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        tools,
        [
            "list_codex_security_scans",
            "list_codex_security_scans",
            "cancel_codex_security_scan",
            "list_codex_security_scans"
        ]
    );
}

#[tokio::test]
async fn a_restart_or_disconnect_stops_running_scans_before_ending_them() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let repo;
    let worktree;
    {
        let e = Engine::new(state.clone(), |_| {}).unwrap();
        (repo, worktree) = scan_copy(&e, "scan-1").await;
        e.update(|s| {
            s.scans.push(running("scan-1", Some(&worktree)));
            let mut preparing = running("scan-2", None);
            preparing.status = "preparing".into();
            preparing.thread_id = None;
            preparing.turn_id = None;
            s.scans.push(preparing);
            let mut done = running("scan-3", None);
            done.status = "completed".into();
            s.scans.push(done);
        })
        .await;
        e.flush().await.unwrap();
    }
    let e = Engine::new(state, |_| {}).unwrap();
    let s = e.snapshot().await;
    // The plugin's scan may outlive Orbit, so the scan keeps its slot and
    // copy until it is stopped.
    assert_eq!(s.scans[0].status, "cancelling");
    assert_eq!(s.scans[0].stop_outcome.as_deref(), Some("interrupted"));
    assert_eq!(s.scans[1].status, "interrupted");
    assert!(s.scans[1]
        .error
        .as_deref()
        .unwrap()
        .contains("Orbit closed"));
    assert_eq!(s.scans[2].status, "completed");
    e.resume_scans().await;
    assert!(worktree.exists());

    // Once Codex connects, the old thread is reopened to stop the scan.
    let rpc = attach(&e).await;
    configure(
        &rpc,
        json!({
            "errors": {"turn/interrupt": "thread not found: scan-thread"},
            "toolResponses": {"list_codex_security_scans": [plugin_scan("running"), plugin_scan("canceled")]},
        }),
    )
    .await;
    e.retry_scan_stops(&e.snapshot().await.scans);
    wait_for(&e, |s| {
        s.scans[0].status == "interrupted" && s.scans[0].worktree.is_none()
    })
    .await;
    let scan = e.snapshot().await.scans[0].clone();
    assert!(scan.error.as_deref().unwrap().contains("Orbit closed"));
    assert!(!worktree.exists());
    let requests = requests(&rpc).await;
    let methods: Vec<_> = requests
        .iter()
        .map(|r| r["method"].as_str().unwrap())
        .collect();
    assert_eq!(&methods[..2], ["turn/interrupt", "thread/resume"]);
    assert_eq!(requests[1]["params"]["threadId"], "scan-thread");

    e.update(|s| s.scans.push(running("scan-4", None))).await;
    e.disconnected().await;
    let s = e.snapshot().await;
    assert_eq!(s.scans[3].status, "cancelling");
    assert!(s.scans[3]
        .stop_reason
        .as_deref()
        .unwrap()
        .contains("Codex disconnected"));
    drop(repo);
}
