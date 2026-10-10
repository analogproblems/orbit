use super::{test_support::*, Engine};
use crate::{buddy::*, types::*};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

/// An engine whose PR has used all 4 of its automatic rounds, with Bit's
/// rule for Continue loop, and the titles of the notifications it shows.
async fn at_round_limit(rule: &str) -> (tempfile::TempDir, Arc<Engine>, Arc<Mutex<Vec<String>>>) {
    let (d, e) = autopilot_engine(trusted_pr()).await;
    let shown = Arc::new(Mutex::new(vec![]));
    let titles = shown.clone();
    e.on_notice(move |n| titles.lock().unwrap().push(n.title.clone()));
    e.update(|s| {
        s.settings.notify = true;
        s.settings.buddy.rules.continue_loop = rule.into();
        for minute in 1..=4 {
            let mut r = Review::new(&s.prs[0], &Settings::default(), true);
            r.id = format!("run-{minute}");
            r.created_at = format!("2026-01-01T00:{minute:02}:00.000Z");
            r.status = "posted".into();
            r.findings = Some(vec![]);
            r.blocking = Some(1);
            r.orbit_verdict = Some(Verdict::ChangesRequested);
            s.reviews.push(r);
        }
    })
    .await;
    (d, e, shown)
}
fn notes(s: &Snapshot) -> Vec<(NoteKind, String)> {
    s.buddy_feed
        .notes
        .iter()
        .map(|n| (n.kind, n.text.clone()))
        .collect()
}
async fn wait_for_note(e: &Engine, kind: NoteKind) -> Snapshot {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let s = e.snapshot().await;
            if s.buddy_feed.notes.last().is_some_and(|n| n.kind == kind) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Timed out waiting for a {kind:?} note"))
}

#[tokio::test]
async fn bit_does_a_fix_on_its_own_only_when_its_rule_says_do_it() {
    // Ask first: the item is noted with its fix and notified.
    let (_d, e, shown) = at_round_limit("ask_first").await;
    e.flush().await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(
        notes(&s),
        [(
            NoteKind::NeedsYou,
            "Could you take a look at b#1? Round limit reached.".to_string()
        )]
    );
    let key = format!("loop:{}:Round limit reached.", s.prs[0].id);
    assert_eq!(s.buddy_feed.notes[0].key.as_deref(), Some(key.as_str()));
    assert_eq!(*shown.lock().unwrap(), ["A review loop needs you"]);
    assert!(e.fixing.lock().unwrap().is_empty());

    // Do it: Bit says it's on it instead of notifying, continues the loop,
    // and says so once.
    let (_d, e, shown) = at_round_limit("do_it").await;
    e.flush().await.unwrap();
    assert!(shown.lock().unwrap().is_empty());
    e.do_fixes();
    let s = wait_for_note(&e, NoteKind::Fixed).await;
    assert_eq!(s.loops[0].extra_rounds, CONTINUE_ROUNDS);
    e.flush().await.unwrap();
    assert_eq!(
        notes(&e.snapshot().await),
        [
            (
                NoteKind::Fixing,
                "b#1: Round limit reached. I'm on it!".to_string()
            ),
            (NoteKind::Fixed, "Gave b#1 two more rounds!".to_string()),
        ]
    );
    assert!(crate::attention::items(&e.snapshot().await).is_empty());
    assert!(shown.lock().unwrap().is_empty());

    // Leave it to me, or Bit turned off: noted as usual, or not at all,
    // and never done.
    let (_d, e, _) = at_round_limit("leave_it").await;
    e.flush().await.unwrap();
    assert_eq!(notes(&e.snapshot().await)[0].0, NoteKind::NeedsYou);
    assert!(e.fixing.lock().unwrap().is_empty());
    let (_d, e, shown) = at_round_limit("do_it").await;
    e.update(|s| s.settings.buddy.enabled = false).await;
    e.flush().await.unwrap();
    assert!(e.fixing.lock().unwrap().is_empty());
    assert_eq!(shown.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn a_fix_that_fails_needs_you_after_all() {
    let (_d, e, shown) = at_round_limit("ask_first").await;
    e.update(|s| {
        s.settings.buddy.rules.review_again = "do_it".into();
        s.reviews.retain(|r| r.id == "run-1");
        s.reviews[0].status = "failed".into();
        s.reviews[0].error = Some("Codex exited.".into());
        // Review again needs a reviewer.
        s.codex_ready = false;
    })
    .await;
    e.flush().await.unwrap();
    e.do_fixes();
    let s = wait_for_note(&e, NoteKind::NeedsYou).await;
    let note = s.buddy_feed.notes.last().unwrap();
    assert!(
        note.text
            .starts_with("I tried to review b#1 again, but it didn't work: "),
        "{}",
        note.text
    );
    assert_eq!(note.key.as_deref(), Some("failed:run-1"));
    assert_eq!(s.buddy_feed.notes[0].kind, NoteKind::Fixing);
    // Notified once the fix failed.
    tokio::time::timeout(Duration::from_secs(3), async {
        while shown.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(*shown.lock().unwrap(), ["An automatic review stopped"]);
}
#[tokio::test]
async fn a_button_uses_its_items_fix_and_notes_it() {
    let (_d, e, _) = at_round_limit("ask_first").await;
    let key = format!("loop:{}:Round limit reached.", trusted_pr().id);
    e.fix(&key).await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.loops[0].extra_rounds, CONTINUE_ROUNDS);
    assert_eq!(
        notes(&s).last().unwrap(),
        &(NoteKind::Fixed, "Gave b#1 two more rounds!".to_string())
    );
    assert_eq!(
        e.fix(&key).await.unwrap_err(),
        "This doesn't need you any more."
    );

    // An approval has no button.
    e.update(|s| {
        s.reviews[0].status = "awaitingApproval".into();
        s.approvals.push(Approval {
            id: serde_json::json!(7),
            review_id: s.reviews[0].id.clone(),
            method: "item/commandExecution/requestApproval".into(),
            params: serde_json::json!({}),
            requested_at: String::new(),
        });
    })
    .await;
    let approval = format!("approval:{}:7", e.snapshot().await.reviews[0].id);
    assert_eq!(
        e.fix(&approval).await.unwrap_err(),
        "There's no button for this one. Open it to see what it needs."
    );
}
#[tokio::test]
async fn turning_autopilot_back_on_is_a_button() {
    let (_d, e) = autopilot_engine(trusted_pr()).await;
    e.update(|s| {
        s.settings.automatic = false;
        s.state_write_error = Some("Disk full".into());
        // Even with every rule on Do it, a settings change waits for a tap.
        s.settings.buddy.rules = Rules {
            continue_loop: "do_it".into(),
            review_again: "do_it".into(),
            publish: "do_it".into(),
            sync_now: "do_it".into(),
        };
    })
    .await;
    e.flush().await.unwrap();
    assert!(e.fixing.lock().unwrap().is_empty());
    e.fix("autopilot-paused").await.unwrap();
    let s = e.snapshot().await;
    assert!(s.settings.automatic);
    assert_eq!(s.state_write_error, None);
    assert_eq!(
        notes(&s).last().unwrap(),
        &(NoteKind::Fixed, "Autopilot is back on!".to_string())
    );
}
#[tokio::test]
async fn a_question_is_answered_in_the_feed() {
    let (_d, e, _) = at_round_limit("ask_first").await;
    e.ask_buddy(Question::NeedsYou).await.unwrap();
    let s = e.snapshot().await;
    let answer = s.buddy_feed.notes.last().unwrap();
    assert_eq!(answer.kind, NoteKind::Answer);
    assert_eq!(answer.question.as_deref(), Some("What needs me?"));
    assert_eq!(answer.text, "1 thing needs you. Here's the list:");
    assert_eq!(answer.lines.len(), 1);
    assert_eq!(answer.lines[0].text, "b#1: Round limit reached.");
}
