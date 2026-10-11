use super::Engine;
use crate::{ask::Action, buddy::*, rpc::Rpc, types::*};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

/// An engine connected to a fake Codex, with one pull request whose loop
/// is running.
async fn fixture() -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>) {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(dir.path().join("state"), |_| {}).unwrap();
    let (tx, rx) = mpsc::channel(64);
    let mut cmd = tokio::process::Command::new("node");
    cmd.args(["-e", include_str!("../model_fixture.cjs")]);
    let rpc = Rpc::from_command(cmd, tx).await.unwrap();
    *e.rpc.lock().await = Some(rpc.clone());
    tokio::spawn(e.clone().pump(rpc.clone(), rx));
    let p = super::test_support::trusted_pr();
    let mut r = Review::new(&p, &Settings::default(), true);
    r.status = "posted".into();
    r.report = "One finding.".into();
    r.findings = Some(vec![]);
    r.blocking = Some(1);
    r.orbit_verdict = Some(Verdict::ChangesRequested);
    e.update(|s| {
        s.settings.model = "model-a".into();
        s.settings.effort = "high".into();
        s.codex_ready = true;
        s.model_catalog_ready = true;
        s.models = crate::models::tests::catalog();
        s.prs.push(p);
        s.reviews.push(r);
    })
    .await;
    e.flush().await.unwrap();
    rpc.request("fixture/configure", json!({})).await.unwrap();
    (dir, e, rpc)
}
async fn requests(rpc: &Rpc) -> Vec<Value> {
    rpc.request("fixture/requests", json!({}))
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}
async fn sent(rpc: &Rpc, method: &str) -> Vec<Value> {
    requests(rpc)
        .await
        .into_iter()
        .filter(|r| r["method"] == method)
        .map(|r| r["params"].clone())
        .collect()
}
/// Codex's reply on Bit's thread, then the end of its turn.
async fn codex_says(e: &Arc<Engine>, text: &str, status: &str) {
    e.event(json!({"method": "item/completed", "params": {
        "threadId": "fixture-thread",
        "item": {"type": "agentMessage", "id": "m1", "text": text},
    }}))
    .await;
    e.event(json!({"method": "turn/completed", "params": {
        "threadId": "fixture-thread",
        "turn": {"id": "fixture-turn", "status": status, "error": {"message": "the model is overloaded"}},
    }}))
    .await;
}
fn last(s: &Snapshot) -> Note {
    s.buddy_feed.notes.last().unwrap().clone()
}

#[tokio::test]
async fn a_typed_question_is_answered_in_a_thread_that_cant_change_anything() {
    let (dir, e, rpc) = fixture().await;
    rpc.request(
        "fixture/configure",
        json!({
            "mcpServers": {"notes": {"command": "notes-server"}},
            "mcpStatus": {"data": [{"name": "notes", "runtimeStatus": "disabled"}], "nextCursor": null},
        }),
    )
    .await
    .unwrap();
    e.ask_buddy_anything("  What's up with b#1?  ")
        .await
        .unwrap();
    let note = last(&e.snapshot().await);
    assert_eq!(note.kind, NoteKind::Answer);
    assert_eq!(note.question.as_deref(), Some("What's up with b#1?"));
    assert!(note.pending);

    let start = &sent(&rpc, "thread/start").await[0];
    let folder = dir.path().join("state").join("bit");
    assert_eq!(start["cwd"], json!(folder));
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    assert_eq!(start["sandbox"], "read-only");
    assert_eq!(start["approvalPolicy"], "never");
    assert_eq!(start["model"], "model-a");
    assert_eq!(start["config"]["model_reasoning_effort"], "high");
    // None of Codex's other tools: not its features, not web search, and
    // not the tool servers its settings name, which it confirms are off.
    assert_eq!(sent(&rpc, "config/read").await[0]["cwd"], json!(folder));
    assert_eq!(start["config"]["web_search"], "disabled");
    assert_eq!(
        start["config"]["mcp_servers"],
        json!({"notes": {"enabled": false}})
    );
    for feature in crate::extras::FEATURES_OFF
        .iter()
        .chain(&crate::ask::TOOLS_OFF)
    {
        assert_eq!(start["config"]["features"][feature], false, "{feature}");
    }
    assert_eq!(
        sent(&rpc, "mcpServerStatus/list").await[0]["threadId"],
        "fixture-thread"
    );
    assert!(start["developerInstructions"]
        .as_str()
        .unwrap()
        .starts_with("You are Bit,"));
    let turn = &sent(&rpc, "turn/start").await[0];
    assert_eq!(turn["outputSchema"], crate::ask::output_schema());
    let prompt = turn["input"][0]["text"].as_str().unwrap();
    assert!(prompt.starts_with("The person asks: What's up with b#1?"));
    assert!(prompt.contains("\"id\": \"a/b#1\""));

    // One question at a time.
    assert_eq!(
        e.ask_buddy_anything("And now?").await.unwrap_err(),
        "Bit is still answering your last question."
    );

    let reply = json!({
        "answer": "b#1 is waiting for its author to push a fix.",
        "proposals": [{"action": "stop_loop", "pr": "a/b#1", "changes": []}],
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let note = last(&e.snapshot().await);
    assert!(!note.pending);
    assert_eq!(note.text, "b#1 is waiting for its author to push a fix.");
    assert_eq!(note.proposals.len(), 1);
    assert_eq!(note.proposals[0].action, Action::StopLoop);
    // Its thread is put away, and a late event changes nothing.
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while sent(&rpc, "thread/archive").await.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    codex_says(&e, "{}", "completed").await;
    assert_eq!(last(&e.snapshot().await), note);
}
#[tokio::test]
async fn a_question_codex_cant_answer_says_why() {
    let (_dir, e, rpc) = fixture().await;
    e.ask_buddy_anything("Why?").await.unwrap();
    codex_says(&e, "", "failed").await;
    assert_eq!(
        last(&e.snapshot().await).text,
        "Sorry, I couldn't answer that. Codex said: the model is overloaded. Try asking again?"
    );

    // Codex disconnecting ends the question too.
    rpc.request("fixture/configure", json!({})).await.unwrap();
    e.ask_buddy_anything("Still there?").await.unwrap();
    e.disconnected().await;
    let note = last(&e.snapshot().await);
    assert!(!note.pending);
    assert_eq!(
        note.text,
        "Sorry, I couldn't answer that. Codex disconnected. Try asking again?"
    );

    // Without Codex, the question isn't asked.
    let error = e.ask_buddy_anything("Hello?").await.unwrap_err();
    assert!(
        error.starts_with("Bit answers typed questions with Codex, which isn't ready: "),
        "{error}"
    );
    e.update(|s| s.settings.buddy.enabled = false).await;
    assert_eq!(
        e.ask_buddy_anything("Hello?").await.unwrap_err(),
        "Turn Bit on in Settings first."
    );
}
#[tokio::test]
async fn a_button_runs_only_when_tapped_and_only_once() {
    let (_dir, e, rpc) = fixture().await;
    e.ask_buddy_anything("Can you pause b#1 and sync less often?")
        .await
        .unwrap();
    // Nothing happens before a tap.
    let reply = json!({
        "answer": "Sure, tap below.",
        "proposals": [
            {"action": "stop_loop", "pr": "a/b#1", "changes": []},
            {"action": "change_settings", "pr": null, "changes": [{"setting": "pollSeconds", "value": "600"}]},
        ],
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let s = e.snapshot().await;
    let note = last(&s).id;
    assert!(s.loops.is_empty());
    assert_eq!(s.settings.poll_seconds, 120);
    let _ = rpc;

    e.use_proposal(note, 0).await.unwrap();
    e.use_proposal(note, 1).await.unwrap();
    let s = e.snapshot().await;
    assert!(s.loops[0].stopped);
    assert_eq!(s.settings.poll_seconds, 600);
    let answer = s.buddy_feed.notes.iter().find(|n| n.id == note).unwrap();
    assert!(answer.proposals.iter().all(|p| p.done));
    assert_eq!(
        e.use_proposal(note, 0).await.unwrap_err(),
        "That's done already."
    );
    assert_eq!(
        e.use_proposal(note, 7).await.unwrap_err(),
        "This button isn't there any more."
    );
}
#[tokio::test]
async fn a_tapped_button_is_checked_again() {
    let (_dir, e, _rpc) = fixture().await;
    let note = {
        let mut s = e.state.lock().await;
        let id = asked(&mut s, "Trust mallory?");
        answered(
            &mut s,
            id,
            "Tap below.".into(),
            vec![crate::ask::Proposal {
                id: 0,
                action: Action::ChangeSettings,
                pr_id: None,
                review_id: None,
                settings: Some(json!({"trustedAuthors": ["mallory"]})),
                done: false,
            }],
        );
        id
    };
    assert_eq!(
        e.use_proposal(note, 0).await.unwrap_err(),
        "Change trusted accounts in Settings yourself."
    );
    assert!(e.snapshot().await.settings.trusted_authors.is_empty());
}
#[tokio::test]
async fn a_question_isnt_asked_while_another_tool_is_on() {
    let (_dir, e, rpc) = fixture().await;
    let not_asked = |e: Arc<Engine>, rpc: Arc<Rpc>| async move {
        e.ask_buddy_anything("Publish everything?").await.unwrap();
        assert!(sent(&rpc, "turn/start").await.is_empty());
        assert_eq!(sent(&rpc, "thread/archive").await.len(), 1);
        let note = last(&e.snapshot().await);
        assert!(!note.pending);
        note.text
    };
    // A plugin's server, on the second page, is still on for the thread.
    rpc.request(
        "fixture/configure",
        json!({"mcpStatus": [
            {"data": [{"name": "notes", "runtimeStatus": "disabled"}], "nextCursor": "2"},
            {"data": [{"name": "codex-security", "runtimeStatus": "connected"}], "nextCursor": null},
        ]}),
    )
    .await
    .unwrap();
    assert_eq!(
        not_asked(e.clone(), rpc.clone()).await,
        "Sorry, I couldn't answer that. Codex kept these tools on for my question, so I didn't ask it: codex-security. Try asking again?"
    );
    // A Codex that can't say which are on isn't asked either.
    rpc.request(
        "fixture/configure",
        json!({"errors": {"mcpServerStatus/list": "unknown method"}}),
    )
    .await
    .unwrap();
    assert_eq!(
        not_asked(e.clone(), rpc.clone()).await,
        "Sorry, I couldn't answer that. I couldn't check that Codex's other tools are off: unknown method. Try asking again?"
    );
}
/// A stand-in for `gh` that logs each call and knows who you are once
/// `signed-in` exists in `dir`.
fn counting_gh(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let d = dir.display();
    let script = format!(
        r#"#!/bin/sh
echo "$*" >> {d}/log
case "$*" in
  *user*) if [ -e {d}/signed-in ]; then echo '{{"login":"me"}}'; else echo '[]'; fi ;;
  *) echo '[]' ;;
esac
"#
    );
    let path = dir.join("gh");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
#[tokio::test]
async fn a_button_runs_once_even_when_two_windows_tap_it_at_once() {
    let (dir, e, _rpc) = fixture().await;
    let note = {
        let mut s = e.state.lock().await;
        let id = asked(&mut s, "Can you sync?");
        let sync = crate::ask::Proposal {
            id: 0,
            action: Action::SyncNow,
            pr_id: None,
            review_id: None,
            settings: None,
            done: false,
        };
        answered(&mut s, id, "Tap below.".into(), vec![sync]);
        id
    };
    let gh = counting_gh(dir.path());
    let syncs = || {
        std::fs::read_to_string(dir.path().join("log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("user"))
            .count()
    };
    let tap = || e.use_proposal(note, 0);
    // A tap that fails can be tried again.
    let failed = crate::github::GH.scope(gh.clone(), tap()).await;
    assert_eq!(failed.unwrap_err(), "GitHub did not return a username.");
    assert_eq!(syncs(), 1);
    std::fs::write(dir.path().join("signed-in"), "").unwrap();
    // A sync is running. Both windows tap, then the sync ends: the first
    // tap waits for it and syncs, the second is refused, not queued.
    let running = e.poll_lock.lock().await;
    let (first, second, ()) = crate::github::GH
        .scope(gh.clone(), async {
            tokio::join!(tap(), tap(), async { drop(running) })
        })
        .await;
    first.unwrap();
    assert_eq!(second.unwrap_err(), "That's already running.");
    assert_eq!(syncs(), 2);
    assert!(last(&e.snapshot().await).proposals[0].done);
    assert_eq!(
        crate::github::GH.scope(gh, tap()).await.unwrap_err(),
        "That's done already."
    );
    assert_eq!(syncs(), 2);
}
