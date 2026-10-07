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
fn plugin_read() -> Value {
    json!({"plugin": {"skills": [
        {"name": "codex-security:security-scan", "path": "/p/skills/security-scan/SKILL.md"},
        {"name": "codex-security:deep-security-scan", "path": "/p/skills/deep-security-scan/SKILL.md"},
    ]}})
}
async fn fixture() -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>) {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(dir.path().join("state"), |_| {}).unwrap();
    let (tx, rx) = mpsc::channel(64);
    let mut cmd = tokio::process::Command::new("node");
    cmd.args(["-e", include_str!("../model_fixture.cjs")]);
    let rpc = Rpc::from_command(cmd, tx).await.unwrap();
    *e.rpc.lock().await = Some(rpc.clone());
    tokio::spawn(e.clone().pump(rpc.clone(), rx));
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
async fn runs_one_scan_at_a_time_and_reports_a_checkout_problem() {
    let (_d, e, rpc) = fixture().await;
    configure(
        &rpc,
        json!({"responses": {"plugin/list": plugin_list(true), "plugin/read": plugin_read()}}),
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
        .contains("without writing findings.json"));

    let mut s = Snapshot::default();
    s.scans.push(running("scan-2", None));
    let limit = json!({"turn": {"id": "scan-turn", "status": "failed",
        "error": {"message": "limit", "codexErrorInfo": "usageLimitExceeded"}}});
    assert_eq!(
        apply_scan_notification(&mut s, "scan-2", "turn/completed", &limit).as_deref(),
        Some("failed")
    );
    assert!(s.scans[0].error.as_deref().unwrap().contains("usage limit"));
    // Another turn on the thread does not end the scan.
    s.scans.push(running("scan-3", None));
    let other = json!({"turn": {"id": "cancel-turn", "status": "completed"}});
    assert_eq!(
        apply_scan_notification(&mut s, "scan-3", "turn/completed", &other),
        None
    );
    assert_eq!(s.scans[1].status, "running");
}

#[tokio::test]
async fn cancelling_interrupts_and_asks_the_plugin_to_stop() {
    let (_d, e, rpc) = fixture().await;
    let mut scan = running("scan-1", None);
    scan.plugin_scan_id = Some("plugin-id".into());
    e.update(|s| s.scans.push(scan)).await;
    e.cancel_scan("scan-1").await.unwrap();
    let requests = requests(&rpc).await;
    let interrupt = requests
        .iter()
        .find(|r| r["method"] == "turn/interrupt")
        .unwrap();
    assert_eq!(
        interrupt["params"],
        json!({"threadId": "scan-thread", "turnId": "scan-turn"})
    );
    let cancel = requests
        .iter()
        .find(|r| r["method"] == "turn/start")
        .unwrap();
    assert!(cancel["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .contains("cancel_codex_security_scan"));
    assert!(cancel["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .contains("plugin-id"));
    let s = e.snapshot().await;
    assert_eq!(s.scans[0].status, "cancelled");
    assert!(e.cancel_scan("scan-1").await.is_err());
    // Events from the cancelled scan's thread are ignored.
    e.event(notification(
        "turn/completed",
        json!({"threadId": "scan-thread", "turn": {"id": "scan-turn", "status": "interrupted"}}),
    ))
    .await;
    assert_eq!(e.snapshot().await.scans[0].status, "cancelled");
    assert!(e.delete_scan("scan-1").await.is_ok());
}

#[tokio::test]
async fn a_restart_or_disconnect_interrupts_running_scans() {
    let d = tempfile::tempdir().unwrap();
    {
        let e = Engine::new(d.path().into(), |_| {}).unwrap();
        e.update(|s| {
            s.scans.push(running("scan-1", None));
            let mut done = running("scan-2", None);
            done.status = "completed".into();
            s.scans.push(done);
        })
        .await;
        e.flush().await.unwrap();
    }
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.scans[0].status, "interrupted");
    assert!(s.scans[0]
        .error
        .as_deref()
        .unwrap()
        .contains("Orbit closed"));
    assert_eq!(s.scans[1].status, "completed");

    e.update(|s| s.scans.push(running("scan-3", None))).await;
    e.disconnected().await;
    let s = e.snapshot().await;
    assert_eq!(s.scans[2].status, "interrupted");
    assert!(s.scans[2]
        .error
        .as_deref()
        .unwrap()
        .contains("Codex disconnected"));
}
