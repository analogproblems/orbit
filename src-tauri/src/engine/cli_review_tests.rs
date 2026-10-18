use super::{test_support::*, Engine};
use crate::{claude::tests::fake, types::*};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

/// An engine with a fake `claude` and one Claude review of a trusted PR,
/// prepared in a worktree and about to start.
async fn claude_review(mode: &str) -> (tempfile::TempDir, Arc<Engine>, String, PathBuf) {
    let (d, worktree) = git_repo().await;
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    e.use_claude(fake(mode));
    let _ = e.check_claude().await;
    let pr = trusted_pr();
    let id = {
        let mut s = e.state.lock().await;
        s.settings.repositories.push(Repository {
            name: pr.repo.clone(),
            path: worktree.to_string_lossy().into(),
            automatic: false,
            setup_command: String::new(),
            network_access: false,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Provider::Claude,
            dev_container: Default::default(),
        });
        let mut review = Review::new(&pr, &s.settings, false);
        review.status = "preparing".into();
        review.worktree = Some(worktree.to_string_lossy().into());
        let id = review.id.clone();
        s.prs.push(pr);
        s.reviews.push(review);
        id
    };
    (d, e, id, worktree)
}
async fn wait_until(e: &Engine, done: impl Fn(&Review) -> bool) -> Review {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let r = e.snapshot().await.reviews[0].clone();
            if done(&r) {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Timed out: {:?}",
            e.state.try_lock().map(|s| s.reviews[0].clone())
        )
    })
}
fn calls(log: &std::path::Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn after(args: &[String], flag: &str) -> String {
    args[args.iter().position(|a| a == flag).unwrap() + 1].clone()
}
/// A turn's prompt, which the fake logs after `--stdin` when it came on
/// standard input.
fn prompt(args: &[String]) -> String {
    match args.iter().position(|a| a == "--stdin") {
        Some(i) => args[i + 1].clone(),
        None => after(args, "-p"),
    }
}

#[tokio::test]
async fn reviews_with_code_review_then_extracts_findings_on_the_same_session() {
    let (d, e, id, worktree) = claude_review("ok").await;
    let log = d.path().join("calls.log");
    e.use_claude(fake(&format!("ok@{}", log.display())));
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "ready").await;
    assert_eq!(r.provider, Provider::Claude);
    assert!(
        r.report.starts_with("Claude's review found 1 issue:"),
        "{}",
        r.report
    );
    assert!(r.report.contains("**Off by one.** (`src/lib.rs:3`)"));
    let findings = r.findings.as_ref().expect("structured findings");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].path.as_deref(), Some("src/lib.rs"));
    assert_eq!(findings[0].category, Some(Category::Bug));
    assert_eq!(findings[0].other_locations[0].label(), "src/main.rs:40");
    assert_eq!(
        findings[0].failure_scenario.as_deref(),
        Some("A list with one item returns nothing.")
    );
    assert_eq!(r.completed, Some(true));
    assert_eq!(r.verdict, Some(Verdict::ChangesRequested));
    assert!(r.error.is_none(), "{:?}", r.error);
    let selection = ModelSelection {
        model: "claude-opus-5-5".into(),
        effort: "high".into(),
    };
    assert_eq!(r.confirmed_review_selection.as_ref(), Some(&selection));
    assert_eq!(r.extraction_selection.as_ref(), Some(&selection));
    assert!(r
        .messages
        .iter()
        .any(|m| m.text == "Running: git diff base...HEAD"));
    assert!(r
        .messages
        .iter()
        .any(|m| m.text == "diff --git a/src/lib.rs b/src/lib.rs"));
    let calls = calls(&log);
    assert_eq!(calls.len(), 2);
    let (review, extraction) = (&calls[0], &calls[1]);
    let session = after(review, "--session-id");
    assert_eq!(r.thread_id.as_deref(), Some(session.as_str()));
    assert_eq!(
        after(review, "-p"),
        format!("/code-review high {}...HEAD", "b".repeat(40))
    );
    assert_eq!(after(review, "--setting-sources"), "");
    assert_eq!(after(review, "--model"), "claude-opus-5-5");
    assert_eq!(after(extraction, "--resume"), session);
    assert_eq!(after(extraction, "--tools"), "");
    let schema: Value = serde_json::from_str(&after(extraction, "--json-schema")).unwrap();
    assert_eq!(schema, super::findings::output_schema());
    let prompt = prompt(extraction);
    assert!(prompt.contains("failure scenario") && prompt.contains("reviewerNotes"));
    // Only Orbit's own command goes on the command line; the rest is piped.
    assert_eq!(after(extraction, "-p"), "--output-format");
    // The finished turn is forgotten once its process has gone.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !e.cli_turns.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_later_round_classifies_earlier_findings_with_read_only_tools() {
    let (d, e, id, worktree) = claude_review("ok").await;
    let log = d.path().join("calls.log");
    e.use_claude(fake(&format!("ok@{}", log.display())));
    e.update(|s| {
        s.reviews[0].review_prompt =
            Some("<orbit-previous-round-x>F1</orbit-previous-round-x>".into())
    })
    .await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    wait_until(&e, |r| r.status == "ready").await;
    let extraction = &calls(&log)[1];
    assert!(prompt(extraction).ends_with("<orbit-previous-round-x>F1</orbit-previous-round-x>"));
    assert!(after(extraction, "--tools").contains("Read"));
    assert!(!after(extraction, "--tools").contains("Edit"));
}

#[tokio::test]
async fn refuses_a_run_on_another_model() {
    let (_d, e, id, worktree) = claude_review("wrong-model").await;
    let error = e.start_cli_review(&id, &worktree).await.unwrap_err();
    assert!(
        error.contains("did not confirm the requested model 'claude-opus-5-5'"),
        "{error}"
    );
    assert!(error.contains("claude-other"), "{error}");
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "preparing");
    assert!(r.thread_id.is_none() && r.confirmed_review_selection.is_none());
    assert!(e.cli_turns.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_crash_fails_the_review_with_claudes_reason() {
    let (_d, e, id, worktree) = claude_review("crash").await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "failed").await;
    let error = r.error.unwrap();
    assert!(error.contains("fixture crashed"), "{error}");
}

#[tokio::test]
async fn stop_kills_the_running_turn() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("calls");
    let (_d, e, id, worktree) = claude_review(&format!("slow@{}", log.display())).await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    assert_eq!(e.snapshot().await.reviews[0].status, "reviewing");
    assert_eq!(e.cli_turns.lock().unwrap().len(), 1);
    let child = log.with_extension("child");
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(&child) {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    e.cancel(&id).await.unwrap();
    assert!(e.cli_turns.lock().unwrap().is_empty());
    // The command Claude started goes with it.
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", &pid])
            .status()
            .unwrap()
            .success()
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while alive() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the turn's command outlived it");
    // The killed process's end does not touch the stopped review.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "cancelled");
    assert!(r.error.is_none());
}

#[tokio::test]
async fn records_commands_claude_was_refused() {
    let (_d, e, id, worktree) = claude_review("denied").await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "ready").await;
    assert_eq!(r.declined_approvals.len(), 2, "{:?}", r.declined_approvals);
    assert!(r.declined_approvals[0].contains("`cargo test`"));
    assert!(r.declined_approvals[0].contains("Claude reviews can't ask for approval"));
}

#[tokio::test]
async fn a_schema_failure_keeps_the_report_unstructured() {
    let (_d, e, id, worktree) = claude_review("schema-fail").await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "ready").await;
    assert!(r.findings.is_none() && !r.report.is_empty());
    let error = r.error.unwrap();
    assert!(
        error.contains("unstructured") && error.contains("schema"),
        "{error}"
    );
}

#[tokio::test]
async fn a_review_that_could_not_read_the_change_fails() {
    let (_d, e, id, worktree) = claude_review("incomplete").await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "failed").await;
    assert!(!r.extracting);
    let error = r.error.unwrap();
    assert!(
        error.contains("could not complete the review") && error.contains("git diff was refused."),
        "{error}"
    );
}

#[tokio::test]
async fn flags_files_the_reviewer_changed() {
    let (_d, e, id, worktree) = claude_review("ok").await;
    std::fs::write(worktree.join("stray.txt"), "x").unwrap();
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "ready").await;
    assert!(r
        .messages
        .iter()
        .any(|m| m.text.contains("left changes in its worktree") && m.text.contains("stray.txt")));
}

#[tokio::test]
async fn answers_follow_ups_on_the_review_session() {
    let (d, e, id, worktree) = claude_review("ok").await;
    let log = d.path().join("calls.log");
    e.use_claude(fake(&format!("ok@{}", log.display())));
    e.start_cli_review(&id, &worktree).await.unwrap();
    let r = wait_until(&e, |r| r.status == "ready").await;
    e.message(&id, "Why?").await.unwrap();
    let r2 = wait_until(&e, |r| {
        r.status == "ready" && r.messages.iter().any(|m| m.text == "Answer: Why?")
    })
    .await;
    let question = r2.messages.iter().find(|m| m.text == "Why?").unwrap();
    assert_eq!(question.delivery, Some(MessageDelivery::Sent));
    assert!(question.confirmed_selection.is_some());
    assert_eq!(r2.report, r.report);
    assert_eq!(after(&calls(&log)[2], "--resume"), r.thread_id.unwrap());
}

#[tokio::test]
async fn reports_a_signed_out_claude() {
    let (_d, e, id, worktree) = claude_review("signed-out").await;
    let s = e.snapshot().await;
    assert!(!s.claude.ready);
    assert!(s.claude.error.unwrap().contains("not signed in"));
    let error = e.start_cli_review(&id, &worktree).await.unwrap_err();
    assert!(error.contains("not signed in"), "{error}");
}

#[tokio::test]
async fn a_codex_disconnect_leaves_claude_reviews_running() {
    let (_d, e, id, worktree) = claude_review("slow").await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    e.event(json!({"method":"orbit/disconnected"})).await;
    assert_eq!(e.snapshot().await.reviews[0].status, "reviewing");
    e.cancel(&id).await.unwrap();
}

#[tokio::test]
async fn queues_claude_reviews_without_codex() {
    let (_d, e, _id, _worktree) = claude_review("ok").await;
    e.update(|s| s.reviews.clear()).await;
    let pr = e.snapshot().await.prs[0].id.clone();
    let queued = e.enqueue(&pr, false).await.unwrap();
    let s = e.snapshot().await;
    let r = s.reviews.iter().find(|r| r.id == queued).unwrap();
    assert_eq!(
        (r.model.as_str(), r.effort.as_str()),
        ("claude-opus-5-5", "high")
    );
    // Codex reviews still need Codex.
    e.update(|s| {
        s.reviews.clear();
        s.settings.repositories[0].provider = Provider::Codex;
    })
    .await;
    assert!(e
        .enqueue(&pr, false)
        .await
        .unwrap_err()
        .contains("Connect Codex"));
}

#[tokio::test]
async fn claude_reviews_refuse_a_setup_command() {
    let (_d, e, id, _worktree) = claude_review("ok").await;
    e.update(|s| s.settings.repositories[0].setup_command = "npm ci".into())
        .await;
    let error = e.prepare(&id).await.unwrap_err();
    assert!(error.contains("setup command"), "{error}");
}

#[tokio::test]
async fn state_saved_before_providers_restores_as_codex() {
    let d = tempfile::tempdir().unwrap();
    let pr = trusted_pr();
    let mut s = Snapshot::default();
    s.settings.repositories.push(Repository {
        name: pr.repo.clone(),
        path: d.path().to_string_lossy().into(),
        automatic: false,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Provider::Codex,
        dev_container: Default::default(),
    });
    let mut review = Review::new(&pr, &s.settings, false);
    review.status = "ready".into();
    s.reviews.push(review);
    s.prs.push(pr);
    let mut saved = serde_json::to_value(&s).unwrap();
    // What an older Orbit saved: no providers, Claude settings or status.
    saved["settings"]["repositories"][0]
        .as_object_mut()
        .unwrap()
        .remove("provider");
    saved["reviews"][0]
        .as_object_mut()
        .unwrap()
        .remove("provider");
    for key in ["claudeModel", "claudeEffort"] {
        saved["settings"].as_object_mut().unwrap().remove(key);
    }
    for key in ["claude", "claudeModels"] {
        saved.as_object_mut().unwrap().remove(key);
    }
    let root = d.path().join("state");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("state.json"), saved.to_string()).unwrap();
    let e = Engine::new(root, |_| {}).unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.repositories[0].provider, Provider::Codex);
    assert_eq!(s.reviews[0].provider, Provider::Codex);
    assert_eq!(s.reviews[0].status, "ready");
    assert!(!s.claude.ready && s.claude.error.is_none());
    assert_eq!(s.claude_models, crate::claude::catalog());
    let claude = s.settings.selection(Provider::Claude);
    assert_eq!(
        (claude.model.as_str(), claude.effort.as_str()),
        ("claude-opus-5-5", "high")
    );
}

#[tokio::test]
async fn refuses_to_review_without_claudes_sandbox() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("calls");
    let (_d, e, id, worktree) = claude_review(&format!("no-sandbox@{}", log.display())).await;
    let error = e.start_cli_review(&id, &worktree).await.unwrap_err();
    assert!(
        error.contains("only run inside Claude's sandbox") && error.contains("bubblewrap"),
        "{error}"
    );
    // Nothing ran unsandboxed, and the review did not go on.
    assert!(!log.with_extension("unsandboxed").exists());
    let r = e.snapshot().await.reviews[0].clone();
    assert!(r.report.is_empty() && r.confirmed_review_selection.is_none());
    let turns = calls(&log)
        .into_iter()
        .filter(|c| c.contains(&"-p".to_string()))
        .count();
    assert_eq!(turns, 1);
    assert!(e.cli_turns.lock().unwrap().is_empty());
}

#[tokio::test]
async fn extraction_weighs_dev_container_results_with_read_only_tools() {
    let (d, e, id, worktree) = claude_review("ok").await;
    let log = d.path().join("calls.log");
    e.use_claude(fake(&format!("ok@{}", log.display())));
    e.update(|s| {
        s.reviews[0].checks = Some(ContainerChecks {
            image: "orbit-devcontainer:a.b-abc".into(),
            network: false,
            setup: None,
            check: Some(CheckStep {
                command: "npm test".into(),
                exit_code: Some(1),
                timed_out: false,
                seconds: 2,
                output: "expected 2, got 3".into(),
            }),
        })
    })
    .await;
    e.start_cli_review(&id, &worktree).await.unwrap();
    wait_until(&e, |r| r.status == "ready").await;
    let calls = calls(&log);
    let (review, extraction) = (&calls[0], &calls[1]);
    // `/code-review` runs as it always does.
    assert_eq!(
        after(review, "-p"),
        format!("/code-review high {}...HEAD", "b".repeat(40))
    );
    let prompt = prompt(extraction);
    assert!(
        prompt.contains("The repository's checks also ran"),
        "{prompt}"
    );
    assert!(prompt.contains("  expected 2, got 3"), "{prompt}");
    // No earlier round, so nothing to classify.
    assert!(!prompt.contains("This pull request was reviewed before"));
    assert!(after(extraction, "--tools").contains("Read"));
    assert!(!after(extraction, "--tools").contains("Edit"));
}

#[tokio::test]
async fn claude_reviews_may_run_setup_in_a_dev_container() {
    let (_d, e, id, _worktree) = claude_review("ok").await;
    e.update(|s| {
        let repo = &mut s.settings.repositories[0];
        repo.setup_command = "npm ci".into();
        repo.dev_container.enabled = true;
    })
    .await;
    // Preparation gets past the setup command and stops at the clone, which
    // has no `origin` in this test.
    let error = e.prepare(&id).await.unwrap_err();
    assert!(!error.contains("setup command"), "{error}");
}
