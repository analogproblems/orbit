use super::{test_support::*, worktrees::git, Engine};
use crate::{antigravity, claude::tests::fake, review_instructions::SCAN_START, types::*};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// An engine with no Codex at all: never connected, so any call to Codex
/// would fail. Repository a/b is connected, to be scanned by `provider`.
async fn engine(provider: Provider) -> (tempfile::TempDir, Arc<Engine>) {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    let checkout = d.path().to_string_lossy().into_owned();
    e.update(|s| {
        s.settings.antigravity_model = "gemini-3.1-pro-high".into();
        s.settings.repositories.push(Repository {
            name: "a/b".into(),
            path: checkout,
            automatic: false,
            setup_command: String::new(),
            network_access: false,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider,
            dev_container: Default::default(),
        });
    })
    .await;
    (d, e)
}
/// A scan by `provider` being prepared, with a real copy of a fresh
/// repository, as `prepare_scan` leaves it before its turn starts.
async fn prepared(e: &Engine, id: &str, provider: Provider) -> (tempfile::TempDir, PathBuf) {
    let (repo_dir, repo) = git_repo().await;
    let worktree = e.scan_worktree(id);
    std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    git(
        &repo,
        &["worktree", "add", "--detach", worktree.to_str().unwrap()],
    )
    .await
    .unwrap();
    let commit = git(&worktree, &["rev-parse", "HEAD"])
        .await
        .unwrap()
        .trim()
        .to_string();
    e.update(|s| {
        let selection = s.settings.selection(provider);
        s.scans.insert(
            0,
            SecurityScan {
                id: id.into(),
                repo: "a/b".into(),
                provider,
                mode: "standard".into(),
                status: "preparing".into(),
                created_at: now(),
                branch: Some("main".into()),
                commit: Some(commit),
                model: selection.model,
                effort: selection.effort,
                worktree: Some(worktree.to_string_lossy().into()),
                ..SecurityScan::default()
            },
        );
    })
    .await;
    (repo_dir, worktree)
}
async fn wait_for(e: &Engine, done: impl Fn(&SecurityScan) -> bool) -> SecurityScan {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let scan = e.snapshot().await.scans[0].clone();
            if done(&scan) {
                return scan;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Timed out: {:?}",
            e.state.try_lock().map(|s| s.scans[0].clone())
        )
    })
}
/// The turns the fake ran, without the checks of its version and sign-in.
fn turns(log: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str::<Vec<String>>(l).unwrap())
        .filter(|args| args.contains(&"-p".to_string()))
        .collect()
}
fn after(args: &[String], flag: &str) -> String {
    args[args.iter().position(|a| a == flag).unwrap() + 1].clone()
}
fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[tokio::test]
async fn a_claude_scan_runs_with_no_codex_and_its_findings_land_with_its_report() {
    let (d, e) = engine(Provider::Claude).await;
    let log = d.path().join("calls.log");
    e.use_claude(fake(&format!("ok@{}", log.display())));
    e.check_claude().await.unwrap();
    let (_repo, worktree) = prepared(&e, "scan-1", Provider::Claude).await;
    e.start_cli_scan("scan-1", &worktree).await.unwrap();
    let scan = wait_for(&e, |x| x.status == "completed").await;

    // Nothing asked Codex, which was never connected.
    assert!(e.rpc.lock().await.is_none());
    assert!(!e.snapshot().await.codex_ready);

    // Its findings, in the form reviews use, for the Findings page.
    assert_eq!(scan.findings.len(), 1);
    let f = &scan.findings[0];
    assert_eq!(f.id, "scan-1-0");
    assert_eq!(f.title, "SQL injection in find_user");
    assert_eq!(f.priority, 1);
    assert_eq!(f.category, Some(Category::Security));
    assert_eq!(f.path.as_deref(), Some("src/db.rs"));
    assert_eq!((f.start_line, f.end_line), (12, 14));
    assert_eq!(f.confirmed, Some(true));
    assert_eq!(f.suggested_fix.as_deref(), Some("Use a bound parameter."));
    assert_eq!(f.number, None);
    assert!(scan.has_report && scan.error.is_none() && scan.warnings.is_empty());
    assert!(scan.finished_at.is_some() && scan.progress.is_none());
    assert!(scan.disk_bytes.unwrap() > 0);
    let report = e.scan_report("scan-1").await.unwrap();
    assert!(report.starts_with("A web service with a public login page."));
    assert!(report.contains("### Problem 1: SQL injection in find_user"));
    // Its copy is gone once its findings are in.
    assert!(scan.worktree.is_none() && !worktree.exists());

    // The scan, then its findings restated on the same conversation.
    let turns = turns(&log);
    assert_eq!(turns.len(), 2);
    let (scan_turn, extraction) = (&turns[0], &turns[1]);
    let session = after(scan_turn, "--session-id");
    assert_eq!(scan.thread_id.as_deref(), Some(session.as_str()));
    let prompt = after(scan_turn, "--stdin");
    assert!(prompt.starts_with(SCAN_START), "{prompt}");
    assert!(prompt.ends_with(&format!("Commit: {}", scan.commit.as_deref().unwrap())));
    // The isolation a review has: no settings, MCP servers or plugins,
    // the sandbox required, and a review's tools on an allow list.
    assert_eq!(after(scan_turn, "--setting-sources"), "");
    assert!(scan_turn.contains(&"--strict-mcp-config".to_string()));
    assert!(!scan_turn.contains(&"--mcp-config".to_string()));
    assert_eq!(after(scan_turn, "--permission-mode"), "dontAsk");
    assert_eq!(after(scan_turn, "--tools"), "Bash,Read,Grep,Glob,Task");
    assert!(scan_turn.contains(&"--allowedTools".to_string()));
    let sandbox: Value = serde_json::from_str(&after(scan_turn, "--settings")).unwrap();
    assert_eq!(sandbox["sandbox"]["failIfUnavailable"], true);
    assert!(!scan_turn.contains(&"--json-schema".to_string()));
    assert_eq!(after(extraction, "--resume"), session);
    assert_eq!(after(extraction, "--tools"), "");
    assert!(after(extraction, "--stdin").starts_with("Restate the findings from the security scan"));
    let schema: Value = serde_json::from_str(&after(extraction, "--json-schema")).unwrap();
    assert!(schema["properties"]["findings"].is_object());
}

#[tokio::test]
async fn starting_a_scan_by_claude_or_antigravity_needs_no_codex() {
    let (d, e) = engine(Provider::Claude).await;
    e.use_claude(fake("ok"));
    e.check_claude().await.unwrap();
    let error = e
        .start_scan("a/b", "deep", Some(Provider::Claude))
        .await
        .unwrap_err();
    assert!(error.contains("standard scan"), "{error}");
    // Codex Security needs Codex.
    assert!(e.start_scan("a/b", "standard", None).await.is_err());
    assert!(e.snapshot().await.scans.is_empty());
    let id = e
        .start_scan("A/B", "standard", Some(Provider::Claude))
        .await
        .unwrap();
    let s = e.snapshot().await;
    let scan = &s.scans[0];
    assert_eq!(scan.id, id);
    assert_eq!(
        (scan.repo.as_str(), scan.provider),
        ("a/b", Provider::Claude)
    );
    assert_eq!((scan.model.clone(), scan.effort.clone()), {
        let c = s.settings.selection(Provider::Claude);
        (c.model, c.effort)
    });
    // The fixture's checkout is no clone of a/b, so preparing it fails.
    let scan = wait_for(&e, |x| x.status == "failed").await;
    assert!(scan.error.as_deref().unwrap().contains("git"), "{scan:?}");
    // One scan at a time, whoever runs it.
    e.update(|s| s.scans[0].status = "running".into()).await;
    let error = e
        .start_scan("a/b", "standard", Some(Provider::Claude))
        .await
        .unwrap_err();
    assert!(error.contains("already running"), "{error}");
    e.update(|s| s.scans.clear()).await;

    // A reviewer that isn't ready can't scan.
    e.use_claude(fake("signed-out"));
    let _ = e.check_claude().await;
    let error = e
        .start_scan("a/b", "standard", Some(Provider::Claude))
        .await
        .unwrap_err();
    assert!(error.contains("not signed in"), "{error}");
    e.use_antigravity(antigravity::tests::fake("ok", &d.path().join("agy.json")));
    e.check_antigravity().await.unwrap();
    e.update(|s| s.settings.antigravity_model.clear()).await;
    let error = e
        .start_scan("a/b", "standard", Some(Provider::Antigravity))
        .await
        .unwrap_err();
    assert!(error.contains("Choose an Antigravity model"), "{error}");
    assert!(e.snapshot().await.scans.is_empty());
}

#[tokio::test]
async fn an_antigravity_scan_runs_in_its_sandbox_and_its_findings_land() {
    let (d, e) = engine(Provider::Antigravity).await;
    let log = d.path().join("calls.log");
    e.use_antigravity(antigravity::tests::fake(
        &format!("ok@{}", log.display()),
        &d.path().join("agy.json"),
    ));
    e.check_antigravity().await.unwrap();
    let (_repo, worktree) = prepared(&e, "scan-1", Provider::Antigravity).await;
    std::fs::write(
        format!("{}.answer", log.display()),
        json!({
            "verdict": "changes_requested", "summary": "One weakness.", "completed": true,
            "notChecked": [], "priorFindings": [],
            "findings": [{
                "title": "Path traversal in download", "priority": 2, "category": "security",
                "locations": [{"path": format!("{}/src/files.rs", worktree.display()), "startLine": 8, "endLine": 8}],
                "problem": "The file name isn't checked.", "failureScenario": "../../etc/passwd is served.",
                "evidence": null, "confirmed": false, "suggestedFix": null, "doneWhen": null,
                "reviewerNotes": null, "confidence": 0.6,
            }],
        })
        .to_string(),
    )
    .unwrap();
    e.start_cli_scan("scan-1", &worktree).await.unwrap();
    let scan = wait_for(&e, |x| x.status == "completed").await;
    assert!(e.rpc.lock().await.is_none());
    assert_eq!(scan.thread_id.as_deref(), Some("conv-1"));
    let f = &scan.findings[0];
    assert_eq!(f.title, "Path traversal in download");
    // A path in the scan's copy is kept relative to the repository.
    assert_eq!(f.path.as_deref(), Some("src/files.rs"));
    assert_eq!(f.confirmed, Some(false));
    let report = e.scan_report("scan-1").await.unwrap();
    assert!(report.contains("### Problem 1: Path traversal in download"));
    let calls: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|c: &Value| c["args"][0] != "models")
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls[0]["prompt"].as_str().unwrap().starts_with(SCAN_START));
    for call in &calls {
        assert!(call["args"]
            .as_array()
            .unwrap()
            .contains(&json!("--sandbox")));
    }
    let extraction = calls[1]["args"].as_array().unwrap();
    assert!(extraction.contains(&json!("--json-schema")));
    let at = extraction
        .iter()
        .position(|a| a == "--conversation")
        .unwrap();
    assert_eq!(extraction[at + 1], "conv-1");
}

#[tokio::test]
async fn cancelling_a_claude_scan_stops_it_at_once() {
    let (d, e) = engine(Provider::Claude).await;
    let log = d.path().join("calls.log");
    e.use_claude(fake("ok"));
    e.check_claude().await.unwrap();
    e.use_claude(fake(&format!("slow@{}", log.display())));
    let (_repo, worktree) = prepared(&e, "scan-1", Provider::Claude).await;
    e.start_cli_scan("scan-1", &worktree).await.unwrap();
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "running");
    assert_eq!(scan.progress.unwrap().phase.as_deref(), Some("reading"));
    let child = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(format!("{}.child", log.display())) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(alive(&child));
    e.cancel_scan("scan-1").await.unwrap();
    // No waiting for anyone to confirm: the run was Orbit's own process.
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "cancelled");
    assert!(scan.finished_at.is_some() && scan.error.is_none());
    wait_for(&e, |x| x.worktree.is_none()).await;
    assert!(!worktree.exists());
    tokio::time::timeout(Duration::from_secs(10), async {
        while alive(&child) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the scan's command still runs");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "cancelled");
    assert!(scan.error.is_none());
    assert_eq!(
        e.cancel_scan("scan-1").await.unwrap_err(),
        "This scan is not running."
    );
    e.delete_scan("scan-1").await.unwrap();
    assert!(e.snapshot().await.scans.is_empty());
}

/// A scan turn Orbit can't record as confirmed is stopped there, as a
/// review's is: its command ends, and no report or extraction follows.
#[tokio::test]
async fn a_scan_turn_orbit_cant_save_is_stopped() {
    let (d, e) = engine(Provider::Claude).await;
    let log = d.path().join("calls.log");
    e.use_claude(fake("ok"));
    e.check_claude().await.unwrap();
    e.use_claude(fake(&format!("slow@{}", log.display())));
    let (_repo, worktree) = prepared(&e, "scan-1", Provider::Claude).await;
    e.flush().await.unwrap();
    let blocker = d.path().join("state").join("state.json.tmp");
    std::fs::create_dir(&blocker).unwrap();
    let error = e.start_cli_scan("scan-1", &worktree).await.unwrap_err();
    assert!(
        error.starts_with("Orbit couldn't save the confirmed Claude scan, so it stopped it: "),
        "{error}"
    );
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "preparing");
    assert!(scan.thread_id.is_none() && scan.turn_id.is_none() && scan.progress.is_none());
    assert!(e.cli_turns.lock().unwrap().is_empty());
    tokio::time::sleep(Duration::from_millis(300)).await;
    if let Ok(child) = std::fs::read_to_string(format!("{}.child", log.display())) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while alive(&child) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the scan's command still runs");
    }
    assert_eq!(turns(&log).len(), 1, "no extraction");
    assert!(!e.scan_results("scan-1").join("report.md").exists());
    std::fs::remove_dir(&blocker).unwrap();
}

#[tokio::test]
async fn a_scan_turn_that_fails_or_doesnt_finish_says_so() {
    for (mode, says) in [
        (
            "crash",
            "Claude could not finish the scan: Claude stopped before finishing",
        ),
        ("schema-fail", "Claude could not finish the scan"),
        (
            "incomplete",
            "Claude could not complete the scan. Its findings so far are kept. src/ was refused.",
        ),
    ] {
        let (_d, e) = engine(Provider::Claude).await;
        e.use_claude(fake("ok"));
        e.check_claude().await.unwrap();
        e.use_claude(fake(mode));
        let (_repo, worktree) = prepared(&e, "scan-1", Provider::Claude).await;
        e.start_cli_scan("scan-1", &worktree).await.unwrap();
        let scan = wait_for(&e, |x| !scan_active(&x.status) && x.worktree.is_none()).await;
        assert_eq!(scan.status, "failed", "{mode}");
        let error = scan.error.as_deref().unwrap();
        assert!(error.starts_with(says), "{mode}: {error}");
        assert!(!worktree.exists());
        // The report is kept whenever the scan turn itself finished.
        assert_eq!(scan.has_report, mode != "crash", "{mode}");
        assert_eq!(
            scan.findings.len(),
            usize::from(mode == "incomplete"),
            "{mode}"
        );
        // What it didn't get to shows on its row too.
        assert_eq!(
            scan.warnings,
            if mode == "incomplete" {
                vec!["Not checked: src/, which was refused".to_string()]
            } else {
                vec![]
            },
            "{mode}"
        );
    }
}

#[tokio::test]
async fn a_scans_refused_commands_are_among_its_warnings() {
    let (_d, e) = engine(Provider::Claude).await;
    e.use_claude(fake("denied"));
    e.check_claude().await.unwrap();
    let (_repo, worktree) = prepared(&e, "scan-1", Provider::Claude).await;
    e.start_cli_scan("scan-1", &worktree).await.unwrap();
    let scan = wait_for(&e, |x| x.status == "completed").await;
    assert!(
        scan.warnings.iter().any(
            |w| w == "Refused Claude's command `cargo test`, as a scan can't ask for approval."
        ),
        "{:?}",
        scan.warnings
    );
}

#[tokio::test]
async fn a_restart_ends_a_claude_scan_and_codex_leaves_it_alone() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let (_repo, worktree) = {
        let e = Engine::new(state.clone(), |_| {}).unwrap();
        let copy = prepared(&e, "scan-1", Provider::Claude).await;
        e.update(|s| {
            s.scans[0].status = "running".into();
            s.scans[0].thread_id = Some("claude-session".into());
        })
        .await;
        e.flush().await.unwrap();
        copy
    };
    let e = Engine::new(state, |_| {}).unwrap();
    let scan = e.snapshot().await.scans[0].clone();
    // Its run ended with Orbit, so nothing waits for it to stop.
    assert_eq!(scan.status, "interrupted");
    assert!(scan.error.as_deref().unwrap().contains("Orbit closed"));
    e.resume_scans().await;
    assert!(!worktree.exists());

    // Codex's threads and connection have nothing to do with it.
    e.update(|s| s.scans[0].status = "running".into()).await;
    assert_eq!(e.scan_for_thread(Some("claude-session")).await, None);
    e.disconnected().await;
    let scan = e.snapshot().await.scans[0].clone();
    assert_eq!(scan.status, "running");
    assert!(scan.stop_reason.is_none());
}
