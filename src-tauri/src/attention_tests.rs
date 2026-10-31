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
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
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
        requested_at: String::new(),
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
fn a_follow_up_chat_does_not_announce_its_reviews_items_again() {
    // A failed run, a held report, and a loop at its round limit, each with
    // a follow-up chat that ends normally or fails.
    fn held(r: &mut Review) {
        r.status = "ready".into();
        r.error = Some("The report mentions a credential.".into());
    }
    fn at_limit(r: &mut Review) {
        r.status = "posted".into();
        r.findings = Some(vec![]);
        r.orbit_verdict = Some(Verdict::ChangesRequested);
        r.blocking = Some(1);
    }
    fn failed(r: &mut Review) {
        r.status = "failed".into();
        r.error = Some("Codex exited.".into());
    }
    let cases = [
        ("failed:run-1", failed as fn(&mut Review)),
        ("held:run-1", held),
        ("loop:a/b#1:Round limit reached.", at_limit),
    ];
    for (key, make) in cases {
        for ending in [None, Some("The follow-up failed: boom".to_string())] {
            let (mut s, p) = snapshot();
            s.settings.repositories[0].round_limit = 1;
            let mut r = run(&p, "reviewing", 1);
            make(&mut r);
            s.reviews.push(r);
            let (notices, _) = announce(&mut s);
            assert!(notices.iter().any(|n| n.item.key == key), "{key}");
            let open = notices.len();
            let r = &mut s.reviews[0];
            r.before_follow_up = Some(Outcome {
                status: std::mem::replace(&mut r.status, "reviewing".into()),
                error: r.error.take(),
            });
            // The list hides the item while the chat runs, but it stays
            // announced.
            assert!(items(&s).is_empty(), "{key}");
            assert_eq!(announce(&mut s), (vec![], false), "{key}");
            assert!(s.reviews[0].end_follow_up(ending.clone()));
            assert_eq!(keys(&s).len(), open, "{key}");
            assert_eq!(announce(&mut s), (vec![], false), "{key} {ending:?}");
        }
    }
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
    crate::review_loop::decide(&mut held, Some(1));
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
        "A/b can't sync. Not found. Orbit checks again every 2 minutes."
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
        "Orbit can't sync with GitHub. HTTP 401: Bad credentials. Orbit checks again every 2 minutes."
    );
}
#[test]
fn a_missing_or_signed_out_gh_needs_you_at_once() {
    let (mut s, _) = snapshot();
    s.settings.poll_seconds = 60;
    s.poll_failures = 1;
    s.github_error = Some(crate::github::GH_MISSING.into());
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].reason,
        format!(
            "Orbit can't sync with GitHub. {} Orbit checks again every minute.",
            crate::github::GH_MISSING
        )
    );
    s.github_error = Some(crate::github::GH_SIGNED_OUT.into());
    assert_eq!(items(&s).len(), 1);
}
#[test]
fn finds_programs_by_path_or_on_path() {
    let dir = tempfile::tempdir().unwrap();
    let gh = dir.path().join("gh");
    assert!(!crate::github::installed(&gh.to_string_lossy()));
    std::fs::write(&gh, "").unwrap();
    assert!(crate::github::installed(&gh.to_string_lossy()));
    assert!(!crate::github::installed("surely-no-such-program-here"));
}
#[test]
fn each_item_names_the_control_that_fixes_it() {
    use crate::buddy::Fix;
    let fix = |s: &Snapshot| -> Vec<Option<Fix>> { items(s).into_iter().map(|a| a.fix).collect() };
    let (mut s, mut p) = snapshot();
    p.head_repo = p.repo.clone();
    p.author_association = "MEMBER".into();
    s.prs[0] = p.clone();

    // A failed or interrupted run: Review again, while nothing else runs.
    s.reviews.push(run(&p, "failed", 1));
    assert_eq!(fix(&s), [Some(Fix::ReviewAgain)]);
    s.reviews[0].status = "cancelled".into();
    s.reviews[0].error = Some("Orbit restarted.".into());
    assert_eq!(fix(&s), [Some(Fix::ReviewAgain)]);
    let mut manual = run(&p, "queued", 2);
    manual.automatic = false;
    s.reviews.push(manual);
    assert_eq!(fix(&s), [None]);

    // A held report: Publish, unless automatic publication holds it for
    // credentials or for where the PR comes from.
    let (mut s, _) = snapshot();
    s.prs[0] = p.clone();
    let mut held = run(&p, "ready", 1);
    held.report = "Looks fine.".into();
    held.findings = Some(vec![]);
    crate::review_loop::decide(&mut held, Some(1));
    held.error = Some("Comment not confirmed: timeout.".into());
    s.reviews.push(held);
    assert_eq!(fix(&s), [Some(Fix::Publish)]);
    s.reviews[0].sensitive = vec!["GitHub token".into()];
    assert_eq!(fix(&s), [None]);
    s.reviews[0].sensitive.clear();
    s.prs[0].author_association = "NONE".into();
    assert_eq!(fix(&s), [None]);
    s.prs[0].author_association = "MEMBER".into();

    // A report waiting for you to publish it.
    s.settings.auto_post = false;
    s.reviews[0].error = None;
    s.reviews[0].findings = Some(vec![Finding {
        priority: 1,
        ..Default::default()
    }]);
    crate::review_loop::decide(&mut s.reviews[0], Some(1));
    let all = items(&s);
    assert_eq!(
        all[0].reason,
        "The report is waiting for you to publish it."
    );
    assert_eq!(all[0].fix, Some(Fix::Publish));

    // A loop at its round limit: Continue loop. Other loop reasons need you.
    s.settings.repositories[0].round_limit = 1;
    s.reviews[0].status = "posted".into();
    assert_eq!(items(&s)[0].reason, "Round limit reached.");
    assert_eq!(fix(&s), [Some(Fix::ContinueLoop)]);
    s.settings.repositories[0].round_limit = 4;
    s.reviews[0].prior_findings = vec![PriorFinding {
        id: "r0-0".into(),
        status: PriorStatus::DeclinedDisputed,
        note: String::new(),
    }];
    assert_eq!(
        items(&s)[0].reason,
        "The reviewer disputes a declined finding."
    );
    assert_eq!(fix(&s), [None]);

    // An approval has none; syncing and a paused autopilot do.
    let (mut s, _) = snapshot();
    s.reviews.push(run(&p, "awaitingApproval", 1));
    s.approvals.push(Approval {
        id: json!(1),
        review_id: "run-1".into(),
        method: "item/commandExecution/requestApproval".into(),
        params: json!({}),
        requested_at: String::new(),
    });
    s.poll_failures = SYNC_FAILURES;
    s.state_write_error = Some("disk full".into());
    s.settings.automatic = false;
    assert_eq!(
        fix(&s),
        [Some(Fix::TurnOnAutopilot), Some(Fix::SyncNow), None]
    );
}
#[test]
fn a_failed_review_is_settled_by_a_later_finished_review_of_the_same_commits() {
    let (mut s, p) = snapshot();
    let mut failed = run(&p, "failed", 1);
    failed.error = Some("Codex exited.".into());
    s.reviews.push(failed);
    assert_eq!(keys(&s).len(), 1);
    // Review again starts a manual review; while it runs the item stays.
    let mut again = Review::new(&p, &Settings::default(), false);
    again.created_at = "2026-01-01T00:02:00.000Z".into();
    again.status = "reviewing".into();
    s.reviews.push(again);
    assert_eq!(keys(&s).len(), 1);
    s.reviews.last_mut().unwrap().status = "ready".into();
    assert!(keys(&s).is_empty());
}
#[test]
fn runs_waiting_for_a_runner_need_you_with_what_orbit_sees_of_its_runners() {
    use crate::problems::{see_waiting, ProblemKind};
    use crate::runners::{Place, PlaceKind, Waiting};
    let (mut s, _) = snapshot();
    let run = Waiting {
        id: 1,
        name: "CI".into(),
        url: String::new(),
        since: String::new(),
    };
    see_waiting(&mut s.problems, "a/b", &[run], true, chrono::Utc::now());
    assert_eq!(s.problems[0].kind, ProblemKind::Waiting);
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, AttentionKind::RunsWaiting);
    assert_eq!(all[0].key, s.problems[0].item_key());
    assert_eq!(all[0].repo.as_deref(), Some("a/b"));
    assert_eq!(all[0].fix, None, "only a person can add or fix a runner");
    assert_eq!(
        all[0].reason,
        "A run of CI in a/b has waited more than 30 minutes for a runner."
    );
    s.runners.push(Place {
        name: "a/b".into(),
        kind: PlaceKind::Repository,
        read_at: "2026-10-30T12:00:00.000Z".into(),
        ..Default::default()
    });
    assert_eq!(
        items(&s)[0].reason,
        "A run of CI in a/b has waited more than 30 minutes for a runner. It has no self-hosted runners of its own. Orbit hasn't read what the a account shares yet."
    );
    let n = notice(&s, items(&s).remove(0));
    assert_eq!(n.title, "Runs are waiting for a runner");
    // Closed, it needs you no more.
    see_waiting(&mut s.problems, "a/b", &[], true, chrono::Utc::now());
    assert!(items(&s).is_empty());
}

/// A pull request whose answer has waited on a person for a day, such as
/// a reviewer or a rule on GitHub, is one item while it still does, in
/// Orbit's words now and announced once. Not while Watch CI is off, nor
/// once something else is in the way.
#[test]
fn a_pull_request_left_waiting_on_a_person_needs_you_once() {
    use crate::readiness::{tests as r, Stand, Turn, WaitKind};
    let (mut s, _) = r::ready();
    s.merges[0].state = "blocked".into();
    let t0 = chrono::Utc::now();
    let pass = |s: &mut Snapshot, hours: i64| {
        let answers = crate::readiness::all(s);
        let now = t0 + chrono::Duration::hours(hours);
        crate::ops::ready(&mut s.ops, &answers, &s.prs, &[], &s.ci, true, now)
    };
    pass(&mut s, 0);
    assert!(items(&s).is_empty());
    pass(&mut s, 23);
    assert!(items(&s).is_empty());
    pass(&mut s, 24);
    let key = format!("stuck:a/b#1:{}", crate::problems::stamp(t0));
    let all = items(&s);
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].kind, &all[0].key, all[0].pr_id.as_deref()),
        (AttentionKind::Stuck, &key, Some("a/b#1"))
    );
    assert_eq!(
        all[0].reason,
        "It has waited on a person for more than a day. GitHub says a rule blocks merging it, such as a required review."
    );
    assert_eq!(notice(&s, all[0].clone()).title, "A pull request is stuck");
    let (notices, _) = announce(&mut s);
    assert_eq!(notices.len(), 1);
    assert!(announce(&mut s).0.is_empty());
    // Its reviews read: the same wait, in its words now.
    r::reviewed_by(&mut s, 1, vec![r::person("bob", Stand::Asked, "")]);
    assert_eq!(
        items(&s)[0].reason,
        "It has waited on a person for more than a day. main needs 1 approval of aaaaaaa and it has none: bob hasn't reviewed aaaaaaa yet."
    );
    assert_eq!(items(&s)[0].key, key);
    pass(&mut s, 48);
    assert_eq!(keys(&s), [(AttentionKind::Stuck, key.clone())]);
    assert!(announce(&mut s).0.is_empty());
    // Not while Watch CI is off.
    s.settings.watch_ci = false;
    assert!(items(&s).is_empty());
    s.settings.watch_ci = true;
    // Nor once something else is in the way, before Orbit works it out
    // again: the author's move, or a base it can't judge.
    let first = |s: &Snapshot| {
        let w = crate::readiness::of(s, &s.prs[0]).waits.remove(0);
        (w.kind, w.turn)
    };
    let mut theirs = s.clone();
    theirs.merges[0].state = "behind".into();
    assert_eq!(first(&theirs), (WaitKind::Merge, Turn::Author));
    assert!(items(&theirs).is_empty());
    let mut unsure = s.clone();
    r::pull(&mut unsure).ran_at = String::new();
    assert_eq!(first(&unsure), (WaitKind::Base, Turn::Person));
    assert!(items(&unsure).is_empty());
    // Nor a pull request Orbit hasn't nudged anyone about, nor one whose
    // answer Orbit hasn't worked out again since it moved on.
    let kept = |edit: fn(&mut crate::ops::Ready)| {
        let mut s = s.clone();
        edit(s.ops[0].ready.as_mut().unwrap());
        items(&s)
    };
    assert!(kept(|r| r.nudged = None).is_empty());
    assert!(kept(|r| r.head_sha = "b".repeat(40)).is_empty());
    assert!(kept(|r| r.turn = Some(Turn::Author)).is_empty());
    assert!(kept(|r| r.clear = true).is_empty());
    assert_eq!(kept(|r| r.text = "Older words.".into()).len(), 1);
    // Approved: it leaves Needs you.
    r::reviewed_by(&mut s, 1, vec![r::person("bob", Stand::Approved, r::HEAD)]);
    s.merges[0].state = "clean".into();
    pass(&mut s, 49);
    assert!(items(&s).is_empty());
}
