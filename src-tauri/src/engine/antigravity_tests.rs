use super::{test_support::*, worktrees::git, Engine};
use crate::{antigravity::tests::fake, types::*};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Setup {
    d: tempfile::TempDir,
    e: Arc<Engine>,
    id: String,
    worktree: PathBuf,
    settings: PathBuf,
}

/// An engine with a fake `agy` and one Antigravity review of a trusted PR,
/// prepared in a worktree whose HEAD adds `src/lib.rs` to the base commit.
/// The clone configures a diff driver for `.rs` files that leaves a marker
/// if it ever runs.
async fn antigravity_review(mode: &str) -> Setup {
    let (d, worktree) = git_repo().await;
    let base = git(&worktree, &["rev-parse", "HEAD"])
        .await
        .unwrap()
        .trim()
        .to_string();
    let marker = d.path().join("textconv-ran");
    let driver = d.path().join("driver.sh");
    std::fs::write(
        &driver,
        format!("#!/bin/sh\ntouch '{}'\ncat \"$1\"\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(
        &worktree,
        &[
            "config",
            "diff.orbit-test.textconv",
            &driver.to_string_lossy(),
        ],
    )
    .await
    .unwrap();
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    std::fs::write(worktree.join(".gitattributes"), "*.rs diff=orbit-test\n").unwrap();
    std::fs::write(worktree.join("src/lib.rs"), "fn first() {}\n").unwrap();
    git(&worktree, &["add", "."]).await.unwrap();
    git(
        &worktree,
        &[
            "-c",
            "user.name=Orbit",
            "-c",
            "user.email=orbit@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "add lib",
        ],
    )
    .await
    .unwrap();
    let settings = d.path().join("agy-settings.json");
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    e.use_antigravity(fake(mode, &settings));
    let _ = e.check_antigravity().await;
    let pr = trusted_pr();
    let id = {
        let mut s = e.state.lock().await;
        s.settings.antigravity_model = "gemini-3.1-pro-high".into();
        s.settings.repositories.push(Repository {
            name: pr.repo.clone(),
            path: worktree.to_string_lossy().into(),
            automatic: false,
            setup_command: String::new(),
            network_access: false,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Provider::Antigravity,
        });
        let mut review = Review::new(&pr, &s.settings, false);
        review.status = "preparing".into();
        review.base_sha = base;
        review.worktree = Some(worktree.to_string_lossy().into());
        let id = review.id.clone();
        s.prs.push(pr);
        s.reviews.push(review);
        id
    };
    Setup {
        d,
        e,
        id,
        worktree,
        settings,
    }
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
/// Each turn the fake ran: its arguments and the prompt it read.
fn calls(log: &Path) -> Vec<(Vec<String>, String)> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|c| c["args"][0] != "models")
        .map(|c| {
            (
                serde_json::from_value(c["args"].clone()).unwrap(),
                c["prompt"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}
fn after(args: &[String], flag: &str) -> String {
    args[args.iter().position(|a| a == flag).unwrap() + 1].clone()
}
fn logged(t: &Setup, mode: &str) -> PathBuf {
    let log = t.d.path().join("calls.log");
    t.e.use_antigravity(fake(&format!("{mode}@{}", log.display()), &t.settings));
    log
}

#[tokio::test]
async fn reviews_with_orbits_instructions_then_extracts_findings_in_the_same_conversation() {
    let t = antigravity_review("ok").await;
    let s = t.e.snapshot().await;
    assert!(s.antigravity.ready, "{:?}", s.antigravity.error);
    assert_eq!(s.antigravity_models.len(), 2);
    let log = logged(&t, "ok");
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    let r = wait_until(&t.e, |r| r.status == "ready").await;
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(r.report.starts_with("Not ready to merge."), "{}", r.report);
    assert_eq!(r.thread_id.as_deref(), Some("conv-1"));
    let findings = r.findings.as_ref().expect("structured findings");
    assert_eq!(findings[0].path.as_deref(), Some("src/lib.rs"));
    assert_eq!(r.verdict, Some(Verdict::ChangesRequested));
    let selection = ModelSelection {
        model: "gemini-3.1-pro-high".into(),
        effort: "high".into(),
    };
    assert_eq!(r.confirmed_review_selection.as_ref(), Some(&selection));
    assert_eq!(r.extraction_selection.as_ref(), Some(&selection));
    assert!(r
        .messages
        .iter()
        .any(|m| m.text.contains("view_file") && m.text.contains("src/lib.rs")));
    let calls = calls(&log);
    assert_eq!(calls.len(), 2);
    let ((review, prompt), (extraction, _)) = (&calls[0], &calls[1]);
    assert!(review.contains(&"--sandbox".to_string()));
    assert!(!review.contains(&"--conversation".to_string()));
    assert_eq!(after(review, "--model"), "gemini-3.1-pro-high");
    assert_eq!(after(review, "--effort"), "high");
    // The change is in the prompt, computed without the clone's diff driver.
    assert!(prompt.starts_with("You are reviewing a pull request."));
    assert!(prompt.contains(&format!("Base commit: {}", r.base_sha)));
    assert!(prompt.contains("+fn first() {}"), "{prompt}");
    assert!(prompt.contains("2 files changed"), "{prompt}");
    let marker = t.d.path().join("textconv-ran");
    assert!(!marker.exists());
    // The driver is real: a plain diff runs it.
    let range = format!("{}...HEAD", r.base_sha);
    git(&t.worktree, &["diff", &range]).await.unwrap();
    assert!(marker.exists());
    assert_eq!(after(extraction, "--conversation"), "conv-1");
    let schema: Value = serde_json::from_str(&after(extraction, "--json-schema")).unwrap();
    assert!(schema["required"]
        .as_array()
        .unwrap()
        .contains(&json!("completed")));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !t.e.cli_turns.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn answers_follow_ups_in_the_review_conversation() {
    let t = antigravity_review("ok").await;
    let log = logged(&t, "ok");
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    wait_until(&t.e, |r| r.status == "ready").await;
    t.e.message(&t.id, "Why?").await.unwrap();
    let r = wait_until(&t.e, |r| {
        r.status == "ready" && r.messages.iter().any(|m| m.text == "Answer: Why?")
    })
    .await;
    let question = r.messages.iter().find(|m| m.text == "Why?").unwrap();
    assert_eq!(question.delivery, Some(MessageDelivery::Sent));
    let (args, prompt) = &calls(&log)[2];
    assert_eq!(after(args, "--conversation"), "conv-1");
    assert_eq!(prompt, "Why?");
}

#[tokio::test]
async fn refuses_a_run_on_another_model() {
    let t = antigravity_review("wrong-model").await;
    let error = t.e.start_cli_review(&t.id, &t.worktree).await.unwrap_err();
    assert!(
        error.contains("did not confirm the requested model 'gemini-3.1-pro-high'")
            && error.contains("gemini-other"),
        "{error}"
    );
    let r = t.e.snapshot().await.reviews[0].clone();
    assert!(r.thread_id.is_none() && r.confirmed_review_selection.is_none());
    assert!(t.e.cli_turns.lock().unwrap().is_empty());
}

#[tokio::test]
async fn refuses_a_run_that_would_not_ask_for_approval() {
    let t = antigravity_review("always-proceed").await;
    let error = t.e.start_cli_review(&t.id, &t.worktree).await.unwrap_err();
    assert!(error.contains("always-proceed"), "{error}");
    assert!(t.e.snapshot().await.reviews[0].thread_id.is_none());
}

#[tokio::test]
async fn refuses_a_turn_in_another_conversation() {
    let t = antigravity_review("wrong-conversation").await;
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    let r = wait_until(&t.e, |r| r.error.is_some()).await;
    assert!(r.findings.is_none());
    let error = r.error.unwrap();
    assert!(
        error.contains("did not continue the review's conversation")
            && error.contains("conv-other"),
        "{error}"
    );
}

#[tokio::test]
async fn refuses_settings_that_reach_past_the_sandbox() {
    let t = antigravity_review("ok").await;
    let log = logged(&t, "ok");
    std::fs::write(
        &t.settings,
        json!({"permissions": {"allow": ["unsandboxed(git push)"]}}).to_string(),
    )
    .unwrap();
    let error = t.e.start_cli_review(&t.id, &t.worktree).await.unwrap_err();
    assert!(
        error.contains("unsandboxed(git push)") && error.contains("never changes"),
        "{error}"
    );
    assert!(calls(&log).is_empty(), "agy never ran");
    // Settings are only read.
    assert!(std::fs::read_to_string(&t.settings)
        .unwrap()
        .contains("unsandboxed(git push)"));
    t.e.check_antigravity().await.unwrap_err();
    assert!(!t.e.snapshot().await.antigravity.ready);
}

#[tokio::test]
async fn stops_a_run_whose_sandbox_cannot_start() {
    let t = antigravity_review("ok").await;
    logged(&t, "no-sandbox");
    let error = t.e.start_cli_review(&t.id, &t.worktree).await.unwrap_err();
    assert!(
        error.contains("only run inside Antigravity's sandbox")
            && error.contains("user namespaces"),
        "{error}"
    );
    // The process was killed before it could go on.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!t.d.path().join("calls.log.unsandboxed").exists());
    assert!(t.e.cli_turns.lock().unwrap().is_empty());
}

#[tokio::test]
async fn records_commands_antigravity_was_refused() {
    let t = antigravity_review("denied").await;
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    let r = wait_until(&t.e, |r| r.status == "ready").await;
    assert_eq!(r.declined_approvals.len(), 1, "{:?}", r.declined_approvals);
    assert!(r.declined_approvals[0].contains("cargo test"));
    assert!(r.declined_approvals[0].contains("Antigravity reviews can't ask for approval"));
}

#[tokio::test]
async fn a_review_that_could_not_read_the_change_fails() {
    let t = antigravity_review("incomplete").await;
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    let r = wait_until(&t.e, |r| r.status == "failed").await;
    let error = r.error.unwrap();
    assert!(
        error.contains("Antigravity could not complete the review")
            && error.contains("The diff was missing."),
        "{error}"
    );
}

#[tokio::test]
async fn a_crash_fails_the_review_with_antigravitys_reason() {
    let t = antigravity_review("crash").await;
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    let r = wait_until(&t.e, |r| r.status == "failed").await;
    assert!(r.error.unwrap().contains("fixture crashed"));
}

#[tokio::test]
async fn stop_kills_the_running_turn() {
    let t = antigravity_review("ok").await;
    let log = logged(&t, "slow");
    t.e.start_cli_review(&t.id, &t.worktree).await.unwrap();
    assert_eq!(t.e.snapshot().await.reviews[0].status, "reviewing");
    let child = log.with_extension("log.child");
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
    t.e.cancel(&t.id).await.unwrap();
    assert!(t.e.cli_turns.lock().unwrap().is_empty());
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
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(t.e.snapshot().await.reviews[0].status, "cancelled");
}

#[tokio::test]
async fn reports_a_signed_out_antigravity() {
    let t = antigravity_review("signed-out").await;
    let s = t.e.snapshot().await;
    assert!(!s.antigravity.ready);
    assert!(s.antigravity.error.unwrap().contains("not signed in"));
    let error = t.e.start_cli_review(&t.id, &t.worktree).await.unwrap_err();
    assert!(error.contains("not signed in"), "{error}");
}

#[tokio::test]
async fn queues_antigravity_reviews_only_with_a_chosen_model() {
    let t = antigravity_review("ok").await;
    t.e.update(|s| s.reviews.clear()).await;
    let pr = t.e.snapshot().await.prs[0].id.clone();
    let queued = t.e.enqueue(&pr, false).await.unwrap();
    let s = t.e.snapshot().await;
    let r = s.reviews.iter().find(|r| r.id == queued).unwrap();
    assert_eq!(r.provider, Provider::Antigravity);
    assert_eq!(
        (r.model.as_str(), r.effort.as_str()),
        ("gemini-3.1-pro-high", "high")
    );
    t.e.update(|s| {
        s.reviews.clear();
        s.settings.antigravity_model.clear();
    })
    .await;
    let error = t.e.enqueue(&pr, false).await.unwrap_err();
    assert!(error.contains("Choose an Antigravity model"), "{error}");
}

#[tokio::test]
async fn settings_refuse_a_model_or_effort_antigravity_does_not_offer() {
    let t = antigravity_review("ok").await;
    let error =
        t.e.update_settings(SettingsPatch {
            antigravity_model: Some("gemini-nope".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(error.contains("gemini-nope"), "{error}");
    let error =
        t.e.update_settings(SettingsPatch {
            antigravity_effort: Some("max".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(error.contains("'max'"), "{error}");
    t.e.update_settings(SettingsPatch {
        antigravity_model: Some("gemini-3.8-flash-medium".into()),
        antigravity_effort: Some("low".into()),
        ..Default::default()
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn antigravity_reviews_refuse_a_setup_command() {
    let t = antigravity_review("ok").await;
    t.e.update(|s| s.settings.repositories[0].setup_command = "npm ci".into())
        .await;
    let error = t.e.prepare(&t.id).await.unwrap_err();
    assert!(error.contains("Antigravity reviews can't use"), "{error}");
}
