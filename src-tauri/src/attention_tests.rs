use super::*;
use crate::types::tests::pr;
use serde_json::json;

fn snapshot() -> (Snapshot, PullRequest) {
    let p = pr();
    let mut s = Snapshot::default();
    s.settings.automatic = true;
    s.settings.auto_post = true;
    s.settings.repositories.push(Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
    });
    s.prs.push(p.clone());
    (s, p)
}
/// An automatic run on the PR's head in `status`, created at `minute`.
fn run(p: &PullRequest, status: &str, minute: u32) -> Review {
    let mut r = Review::new(p, &Settings::default(), true);
    r.id = format!("run-{minute}");
    r.created_at = format!("2026-01-01T00:{minute:02}:00.000Z");
    r.status = status.into();
    r
}
fn keys(s: &Snapshot) -> Vec<(AttentionKind, String)> {
    items(s).into_iter().map(|a| (a.kind, a.key)).collect()
}

#[test]
fn nothing_needs_you_in_a_quiet_workspace() {
    let (s, _) = snapshot();
    assert!(items(&s).is_empty());
}
#[test]
fn a_pending_approval_is_one_item_until_answered() {
    let (mut s, p) = snapshot();
    s.reviews.push(run(&p, "awaitingApproval", 1));
    s.approvals.push(Approval {
        id: json!(7),
        review_id: "run-1".into(),
        method: "item/commandExecution/requestApproval".into(),
        params: json!({}),
    });
    let all = items(&s);
    assert_eq!(all.len(), 1, "the loop's own reason is not repeated");
    assert_eq!(all[0].kind, AttentionKind::Approval);
    assert_eq!(all[0].key, "approval:run-1:7");
    assert_eq!(all[0].pr_id.as_deref(), Some("a/b#1"));
    assert_eq!(all[0].reason, "The reviewer asks to run a command.");
    assert_eq!(all[0].approval_id, Some(json!(7)));
    s.approvals.clear();
    assert!(items(&s).is_empty());
}
#[test]
fn a_failed_or_interrupted_automatic_review_is_one_item() {
    let (mut s, p) = snapshot();
    let mut failed = run(&p, "failed", 1);
    failed.error = Some("Codex exited.".into());
    s.reviews.push(failed);
    assert_eq!(
        keys(&s),
        [(AttentionKind::Failed, "failed:run-1".to_string())]
    );
    assert_eq!(items(&s)[0].reason, "Codex exited.");
    // A newer run replaces it.
    s.reviews.push(run(&p, "reviewing", 2));
    assert!(items(&s).is_empty());
    // A restart mid-run leaves an error; a stop by the developer does not.
    s.reviews[1].status = "cancelled".into();
    assert!(items(&s).is_empty());
    s.reviews[1].error = Some("Orbit restarted while this review was active.".into());
    assert_eq!(
        keys(&s),
        [(AttentionKind::Failed, "interrupted:run-2".to_string())]
    );
}
#[test]
fn manual_and_superseded_runs_are_not_items() {
    let (mut s, p) = snapshot();
    let mut manual = run(&p, "failed", 1);
    manual.automatic = false;
    let mut replaced = run(&p, "failed", 2);
    replaced.superseded = true;
    s.reviews.extend([manual, replaced]);
    assert!(items(&s).is_empty());
}
#[test]
fn a_held_publication_is_one_item_on_the_current_head() {
    let (mut s, p) = snapshot();
    let mut held = run(&p, "ready", 1);
    held.findings = Some(vec![]);
    crate::review_loop::decide(&mut held, 1);
    held.error = Some("Not published automatically: looks like credentials.".into());
    s.reviews.push(held);
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, AttentionKind::Held);
    assert_eq!(all[0].key, "held:run-1");
    assert!(all[0].reason.contains("credentials"));
    s.prs[0].head_sha = "c".repeat(40);
    assert!(items(&s).iter().all(|a| a.kind != AttentionKind::Held));
}
#[test]
fn loop_reasons_become_items_and_a_stopped_loop_has_none() {
    let (mut s, p) = snapshot();
    s.settings.repositories[0].round_limit = 1;
    let mut r = run(&p, "posted", 1);
    r.findings = Some(vec![]);
    r.orbit_verdict = Some(Verdict::ChangesRequested);
    r.blocking = Some(1);
    s.reviews.push(r);
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, AttentionKind::Loop);
    assert_eq!(all[0].key, "loop:a/b#1:Round limit reached.");
    assert_eq!(all[0].review_id.as_deref(), Some("run-1"));
    s.loops.push(LoopControl {
        pr_id: p.id.clone(),
        stopped: true,
        extra_rounds: 0,
    });
    assert!(items(&s).is_empty());
}
#[test]
fn a_failed_save_that_paused_autopilot_is_an_item_until_it_is_back_on() {
    let (mut s, _) = snapshot();
    s.state_write_error = Some("disk full".into());
    s.settings.automatic = false;
    assert_eq!(
        keys(&s),
        [(
            AttentionKind::AutopilotPaused,
            "autopilot-paused".to_string()
        )]
    );
    assert!(items(&s)[0].reason.ends_with("disk full"));
    s.settings.automatic = true;
    assert!(items(&s).is_empty());
}
#[test]
fn a_repository_needs_you_after_three_failed_syncs_in_a_row() {
    let (mut s, _) = snapshot();
    let fail = |s: &mut Snapshot| count_sync_failures(s, &[("A/b".into(), "Not found".into())]);
    fail(&mut s);
    fail(&mut s);
    assert!(items(&s).is_empty());
    fail(&mut s);
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, AttentionKind::SyncFailing);
    assert_eq!(all[0].key, "sync:a/b");
    assert_eq!(all[0].repo.as_deref(), Some("A/b"));
    assert_eq!(
        all[0].reason,
        "A/b failed to sync 3 times in a row: Not found"
    );
    count_sync_failures(&mut s, &[]);
    assert!(s.sync_failures.is_empty());
}
#[test]
fn the_shown_snapshot_carries_items_that_are_never_saved() {
    let (mut s, _) = snapshot();
    s.state_write_error = Some("disk full".into());
    s.settings.automatic = false;
    let shown = shown(&s);
    assert_eq!(shown.attention.len(), 1);
    let saved = serde_json::to_string(&s).unwrap();
    assert!(!saved.contains("\"attention\""));
    let back: Snapshot = serde_json::from_str(&serde_json::to_string(&shown).unwrap()).unwrap();
    assert!(back.attention.is_empty());
    assert_eq!(back.state_write_error.as_deref(), Some("disk full"));
}
#[test]
fn github_needs_you_after_three_failed_polls_in_a_row() {
    let (mut s, _) = snapshot();
    s.github_error = Some("HTTP 401: Bad credentials".into());
    s.poll_failures = 2;
    assert!(items(&s).is_empty());
    s.poll_failures = 3;
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].key, "sync:github");
    assert_eq!(all[0].repo, None);
    assert_eq!(
        all[0].reason,
        "Orbit could not sync with GitHub 3 times in a row: HTTP 401: Bad credentials"
    );
}
