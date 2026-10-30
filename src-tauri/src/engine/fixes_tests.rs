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
/// Waits for the fixes Bit queued to finish.
async fn do_fixes(e: &Arc<Engine>) {
    for task in e.do_fixes() {
        task.await.unwrap();
    }
}
#[tokio::test]
async fn a_queued_fix_waits_for_nothing_that_isnt_bits_to_do_any_more() {
    let pr = trusted_pr().id;
    // You stop the loop before Bit gets to it: it stays stopped, and as the
    // item closed, there's nothing to say or notify.
    let (_d, e, shown) = at_round_limit("do_it").await;
    e.flush().await.unwrap();
    e.stop_loop(&pr).await.unwrap();
    do_fixes(&e).await;
    let s = e.snapshot().await;
    assert!(s.loops[0].stopped);
    assert_eq!(s.loops[0].extra_rounds, 0);
    assert_eq!(notes(&s).len(), 1);
    assert_eq!(notes(&s)[0].0, NoteKind::Fixing);
    assert!(shown.lock().unwrap().is_empty());

    // You change the rule to Ask first: the loop isn't continued, and it's
    // handed back to you with its button and a notification.
    let (_d, e, shown) = at_round_limit("do_it").await;
    e.flush().await.unwrap();
    e.update(|s| s.settings.buddy.rules.continue_loop = "ask_first".into())
        .await;
    do_fixes(&e).await;
    let s = e.snapshot().await;
    assert_eq!(s.loops.first().map_or(0, |c| c.extra_rounds), 0);
    let note = s.buddy_feed.notes.last().unwrap();
    assert_eq!(
        (note.kind, note.text.as_str()),
        (
            NoteKind::NeedsYou,
            "I tried to continue the loop on b#1, but it didn't work: My rule for this isn't Do it any more."
        )
    );
    assert_eq!(
        note.key.as_deref(),
        Some(format!("loop:{pr}:Round limit reached.").as_str())
    );
    assert_eq!(*shown.lock().unwrap(), ["A review loop needs you"]);

    // You turn Bit off: not done, nothing written, notified as usual.
    let (_d, e, shown) = at_round_limit("do_it").await;
    e.flush().await.unwrap();
    e.update(|s| s.settings.buddy.enabled = false).await;
    do_fixes(&e).await;
    let s = e.snapshot().await;
    assert_eq!(s.loops.first().map_or(0, |c| c.extra_rounds), 0);
    assert_eq!(notes(&s).len(), 1);
    assert_eq!(*shown.lock().unwrap(), ["A review loop needs you"]);
}
#[tokio::test]
async fn a_queued_review_again_never_starts_a_second_review() {
    let (_d, e, shown) = at_round_limit("ask_first").await;
    e.update(|s| {
        s.settings.buddy.rules.review_again = "do_it".into();
        s.reviews.retain(|r| r.id == "run-1");
        s.reviews[0].status = "failed".into();
        s.reviews[0].error = Some("Codex exited.".into());
    })
    .await;
    e.flush().await.unwrap();
    // You start a review yourself before Bit gets to it. The failed run
    // still needs you, so it's handed back rather than reviewed twice.
    let pr = trusted_pr().id;
    e.enqueue(&pr, false).await.unwrap();
    do_fixes(&e).await;
    let s = e.snapshot().await;
    assert_eq!(s.reviews.iter().filter(|r| r.pr_id == pr).count(), 2);
    assert_eq!(
        notes(&s).last().unwrap(),
        &(
            NoteKind::NeedsYou,
            "I tried to review b#1 again, but it didn't work: That isn't the fix for this any more."
                .to_string()
        )
    );
    assert_eq!(*shown.lock().unwrap(), ["An automatic review stopped"]);
}

/// Bit's publications, against a fake `gh`.
#[cfg(unix)]
mod publishing {
    use super::super::publish_tests::fake_gh::*;
    use super::*;
    use crate::github::GH;

    /// A structured report from a member's PR whose automatic publication
    /// wasn't confirmed, so Bit offers Publish, with Bit's rule for it. It is
    /// the only thing that needs you.
    async fn held(rule: &str) -> (tempfile::TempDir, Arc<Engine>, Attention) {
        let (d, e, id) = structured_autopost().await;
        e.update(|s| {
            s.reviews[0].orbit_verdict = Some(Verdict::ChangesRequested);
            s.reviews[0].error = Some("Comment not confirmed: timed out.".into());
            s.settings.buddy.rules.publish = rule.into();
        })
        .await;
        let item = crate::attention::items(&e.snapshot().await)
            .into_iter()
            .find(|a| a.key == format!("held:{id}"))
            .unwrap();
        assert_eq!(item.fix, Some(Fix::Publish));
        assert_eq!(crate::attention::items(&e.snapshot().await).len(), 1);
        (d, e, item)
    }
    /// What GitHub says about the PR's author from now on.
    fn fresh_author(dir: &std::path::Path, association: &str) {
        let path = dir.join("pr.json");
        let mut pr: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        pr["author_association"] = association.into();
        std::fs::write(path, pr.to_string()).unwrap();
    }

    #[tokio::test]
    async fn bit_publishes_only_while_the_fresh_pr_comes_from_a_trusted_source() {
        // Orbit last saw a member's PR, but GitHub now says otherwise:
        // neither the button nor Do it publishes.
        for on_its_own in [false, true] {
            let (d, e, item) = held("do_it").await;
            let gh = fake_gh(d.path());
            fresh_author(d.path(), "CONTRIBUTOR");
            let result = if on_its_own {
                GH.scope(gh, e.run_fix(&item, Fix::Publish, true)).await
            } else {
                GH.scope(gh, e.fix(&item.key)).await
            };
            let error = result.unwrap_err();
            assert!(
                error.contains("doesn't come from a source Orbit trusts"),
                "{error}"
            );
            assert_eq!(posts(d.path()), 0);
            assert_eq!(e.snapshot().await.reviews[0].status, "ready");
        }
        // Still trusted: Do it publishes, whatever the publication switch
        // says.
        let (d, e, item) = held("do_it").await;
        e.update(|s| s.settings.auto_post = false).await;
        let gh = fake_gh(d.path());
        GH.scope(gh, e.run_fix(&item, Fix::Publish, true))
            .await
            .unwrap();
        assert_eq!(posts(d.path()), 1);
        assert_eq!(e.snapshot().await.reviews[0].status, "posted");
    }
    #[tokio::test]
    async fn a_failed_publication_is_tried_once_then_handed_back() {
        let (d, e, item) = held("do_it").await;
        let shown = Arc::new(Mutex::new(vec![]));
        let titles = shown.clone();
        e.on_notice(move |n| titles.lock().unwrap().push(n.title.clone()));
        e.update(|s| s.settings.notify = true).await;
        let gh = fake_gh(d.path());
        std::fs::write(d.path().join("fail"), "").unwrap();
        e.flush().await.unwrap();
        let queued = std::mem::take(&mut *e.fixing.lock().unwrap());
        assert_eq!(queued.len(), 1);
        assert!(shown.lock().unwrap().is_empty());
        GH.scope(gh, async {
            for (queued, fix) in queued {
                e.do_fix(queued, fix).await;
            }
            // Later looks don't take the report for a new item.
            for _ in 0..3 {
                e.flush().await.unwrap();
                assert!(e.fixing.lock().unwrap().is_empty());
            }
        })
        .await;
        assert_eq!(posts(d.path()), 1);
        assert_eq!(*shown.lock().unwrap(), ["A report was held back"]);
        let s = e.snapshot().await;
        let note = s.buddy_feed.notes.last().unwrap();
        assert_eq!(
            (note.kind, note.key.as_deref()),
            (NoteKind::NeedsYou, Some(item.key.as_str()))
        );
        assert_eq!(s.reviews[0].status, "ready");
    }
    #[tokio::test]
    async fn a_queued_sync_checks_again_once_the_running_sync_ends() {
        let (d, e) = autopilot_engine(trusted_pr()).await;
        e.update(|s| {
            s.poll_failures = crate::attention::SYNC_FAILURES;
            s.github_error = Some("timed out".into());
            s.settings.buddy.rules.sync_now = "do_it".into();
        })
        .await;
        let item = crate::attention::items(&e.snapshot().await)
            .into_iter()
            .find(|a| a.key == "sync:github")
            .unwrap();
        let gh = fake_gh(d.path());
        // A sync is running; the rule changes before it ends.
        let running = e.poll_lock.lock().await;
        let revoke = async {
            e.update(|s| s.settings.buddy.rules.sync_now = "ask_first".into())
                .await;
            drop(running);
        };
        let (result, ()) = GH
            .scope(gh, async {
                tokio::join!(e.run_fix(&item, Fix::SyncNow, true), revoke)
            })
            .await;
        assert_eq!(
            result.unwrap_err(),
            "My rule for this isn't Do it any more."
        );
        // No sync started, so no GitHub requests and no clone updates.
        assert!(!d.path().join("log").exists());
        let s = e.snapshot().await;
        assert!(!s.polling);
        assert_eq!(s.poll_failures, crate::attention::SYNC_FAILURES);
    }
    #[tokio::test]
    async fn bit_checks_again_before_it_writes() {
        // While the diff is fetched: the rule changes, Bit is turned off,
        // the author leaves the organization, or you stop trusting them.
        for change in ["rule", "bit off", "left", "untrusted"] {
            let (d, e, item) = held("do_it").await;
            let gh = fake_gh(d.path());
            if change == "untrusted" {
                fresh_author(d.path(), "CONTRIBUTOR");
                e.update(|s| {
                    s.prs[0].author_association = "CONTRIBUTOR".into();
                    s.settings.trusted_authors = vec!["a".into()];
                })
                .await;
            }
            std::fs::write(d.path().join("pause"), "").unwrap();
            let revoke = async {
                wait_for_file(d.path().join("fetching")).await;
                match change {
                    "rule" => {
                        e.update(|s| s.settings.buddy.rules.publish = "ask_first".into())
                            .await
                    }
                    "bit off" => e.update(|s| s.settings.buddy.enabled = false).await,
                    "left" => fresh_author(d.path(), "CONTRIBUTOR"),
                    _ => e.update(|s| s.settings.trusted_authors.clear()).await,
                }
                std::fs::write(d.path().join("release"), "").unwrap();
            };
            let (result, ()) = GH
                .scope(gh, async {
                    tokio::join!(e.run_fix(&item, Fix::Publish, true), revoke)
                })
                .await;
            assert!(result.is_err(), "{change}");
            assert_eq!(posts(d.path()), 0, "{change}");
            assert_eq!(e.snapshot().await.reviews[0].status, "ready", "{change}");
        }

        // Waiting behind another publication, the rule changes.
        let (d, e, item) = held("do_it").await;
        let gh = fake_gh(d.path());
        let turn = e.publish_lock.lock().await;
        let revoke = async {
            e.update(|s| s.settings.buddy.rules.publish = "ask_first".into())
                .await;
            drop(turn);
        };
        let (result, ()) = GH
            .scope(gh, async {
                tokio::join!(e.run_fix(&item, Fix::Publish, true), revoke)
            })
            .await;
        assert_eq!(
            result.unwrap_err(),
            "My rule for this isn't Do it any more."
        );
        assert_eq!(posts(d.path()), 0);
        assert_eq!(e.snapshot().await.reviews[0].status, "ready");
    }
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
            comment_on_prs: "do_it".into(),
            open_issues: "do_it".into(),
            rerun_flaky: "do_it".into(),
            rerun_again: "do_it".into(),
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
