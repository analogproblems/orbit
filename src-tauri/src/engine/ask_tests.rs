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
/// Bit's answer to a question it can't answer yet: its lines and buttons.
async fn cant_answer(e: &Arc<Engine>, question: &str) -> (Vec<String>, Vec<String>) {
    e.ask_buddy_anything(question).await.unwrap();
    let note = last(&e.snapshot().await);
    assert_eq!(note.question.as_deref(), Some(question));
    assert_eq!(note.kind, NoteKind::Answer);
    assert!(!note.pending);
    assert!(e.asking.lock().unwrap().is_none());
    (
        note.lines.iter().map(|l| l.text.clone()).collect(),
        note.proposals
            .iter()
            .map(|p| p.label.clone().unwrap_or_else(|| format!("{:?}", p.action)))
            .collect(),
    )
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

    // Without Codex, or anything else to answer with, Bit's answer is
    // how to connect something.
    e.update(|s| s.codex_ready = false).await;
    let (lines, buttons) = cant_answer(&e, "Hello?").await;
    assert_eq!(
        last(&e.snapshot().await).text,
        "I'd love to answer that, but I need something to answer with first. Tap a button to connect one:"
    );
    assert_eq!(
        lines[0],
        "First, I need something to answer your questions with: Codex, Claude, Antigravity, Ollama or your own API key. Tap one below to set it up."
    );
    assert!(lines[1].starts_with("Codex isn't connected."), "{lines:?}");
    assert_eq!(
        buttons,
        [
            "Get Claude Code",
            "Get Antigravity",
            "Use Ollama",
            "Use an OpenAI API key",
            "Use an Anthropic API key",
            "Use a Gemini API key",
            "CheckConnections",
        ]
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
                repo: None,
                repository: None,
                connect: false,
                service: None,
                link: None,
                label: None,
                done: false,
            }],
            vec![],
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
            repo: None,
            repository: None,
            connect: false,
            service: None,
            link: None,
            label: None,
            done: false,
        };
        answered(&mut s, id, "Tap below.".into(), vec![sync], vec![]);
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
#[tokio::test]
async fn bit_keeps_what_it_remembers_until_you_forget_it() {
    let (dir, e, _rpc) = fixture().await;
    e.update(|s| {
        let mut repo: Repository = serde_json::from_value(json!({
            "name": "a/b", "path": "/tmp/a-b", "automatic": false,
            "setupCommand": "", "networkAccess": false,
        }))
        .unwrap();
        repo.round_limit = 4;
        s.settings.repositories.push(repo);
    })
    .await;
    e.ask_buddy_anything("Please remember I like short answers.")
        .await
        .unwrap();
    let reply = json!({
        "answer": "Got it.",
        "proposals": [],
        "memory": {"forget": [], "remember": [
            {"repository": null, "line": "Jason likes short answers."},
            {"repository": "A/B", "line": "Reviews often flag missing tests."},
        ]},
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let s = e.snapshot().await;
    assert_eq!(s.buddy_feed.memory.general, ["Jason likes short answers."]);
    assert_eq!(
        s.buddy_feed.memory.repositories["a/b"],
        ["Reviews often flag missing tests."]
    );
    assert_eq!(last(&s).memory.len(), 2);
    // Saved with the state, and shown to Codex next time.
    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("state").join("state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        saved["buddyFeed"]["memory"]["general"],
        json!(["Jason likes short answers."])
    );
    assert_eq!(
        crate::ask::briefing(&s, &crate::ask::Lookups::default())["memory"]["repositories"]["a/b"],
        json!(["Reviews often flag missing tests."])
    );

    e.forget_memory(Some("A/B"), "reviews often flag missing tests.")
        .await
        .unwrap();
    let s = e.snapshot().await;
    assert!(s.buddy_feed.memory.repositories.is_empty());
    assert_eq!(
        e.forget_memory(None, "Not a line.").await.unwrap_err(),
        "That line isn't there any more."
    );
    e.clear_memory().await.unwrap();
    assert_eq!(e.snapshot().await.buddy_feed.memory, Memory::default());
}

/// What Bit remembers, as saved in `state.json`.
fn saved_memory(dir: &tempfile::TempDir) -> Value {
    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("state").join("state.json")).unwrap(),
    )
    .unwrap();
    saved["buddyFeed"]["memory"].clone()
}

#[tokio::test]
async fn an_answer_cant_bring_back_what_you_forgot_while_it_was_coming() {
    let (dir, e, rpc) = fixture().await;
    e.update(|s| {
        s.buddy_feed.memory.general = vec!["Jason likes long answers.".into()];
    })
    .await;
    e.ask_buddy_anything("How should you answer me?")
        .await
        .unwrap();
    // You forget that line while Codex is still answering...
    e.forget_memory(None, "Jason likes long answers.")
        .await
        .unwrap();
    // ...and the answer, written with the old memory, remembers it again.
    let reply = json!({
        "answer": "At length.",
        "proposals": [],
        "memory": {"forget": [], "remember": [
            {"repository": null, "line": "Jason likes long answers."},
        ]},
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let s = e.snapshot().await;
    assert!(!last(&s).pending);
    assert_eq!(last(&s).text, "At length.");
    assert!(last(&s).memory.is_empty());
    assert!(s.buddy_feed.memory.general.is_empty());
    assert_eq!(saved_memory(&dir)["general"], json!([]));

    // The next answer, asked after you forgot, remembers as usual.
    rpc.request("fixture/configure", json!({})).await.unwrap();
    e.ask_buddy_anything("Please remember I like short answers.")
        .await
        .unwrap();
    let reply = json!({
        "answer": "Got it.",
        "proposals": [],
        "memory": {"forget": [], "remember": [
            {"repository": null, "line": "Jason likes short answers."},
        ]},
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let s = e.snapshot().await;
    assert_eq!(s.buddy_feed.memory.general, ["Jason likes short answers."]);
    assert_eq!(
        saved_memory(&dir)["general"],
        json!(["Jason likes short answers."])
    );
}

#[tokio::test]
async fn an_answer_cant_refill_a_memory_you_cleared_while_it_was_coming() {
    let (dir, e, _rpc) = fixture().await;
    e.update(|s| {
        s.buddy_feed.memory.general = vec!["Jason likes long answers.".into()];
    })
    .await;
    e.ask_buddy_anything("What do you know about me?")
        .await
        .unwrap();
    e.clear_memory().await.unwrap();
    let reply = json!({
        "answer": "You like long answers.",
        "proposals": [],
        "memory": {"forget": [], "remember": [
            {"repository": null, "line": "Jason likes long answers."},
            {"repository": null, "line": "Jason asks what Bit knows."},
        ]},
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let s = e.snapshot().await;
    assert!(!last(&s).pending);
    assert_eq!(last(&s).text, "You like long answers.");
    assert_eq!(s.buddy_feed.memory, Memory::default());
    assert_eq!(saved_memory(&dir)["general"], json!([]));
}

#[tokio::test]
async fn bit_can_offer_to_connect_a_repository_github_lists() {
    let (dir, e, rpc) = fixture().await;
    let gh = super::setup_tests::fake_gh(dir.path());
    crate::github::GH
        .scope(gh, e.ask_buddy_anything("Can you set up acme/api?"))
        .await
        .unwrap();
    let turn = &sent(&rpc, "turn/start").await[0];
    let prompt = turn["input"][0]["text"].as_str().unwrap();
    assert!(
        prompt.contains("\"connectableRepositories\": [\n"),
        "{prompt}"
    );
    assert!(prompt.contains("\"name\": \"acme/api\""));
    let reply = json!({
        "answer": "Tap to connect it.",
        "proposals": [
            {"action": "set_up_repository", "pr": null, "repository": "acme/api", "changes": [
                {"setting": "setupCommand", "value": "npm ci"},
                {"setting": "networkAccess", "value": "true"},
            ]},
            {"action": "set_up_repository", "pr": null, "repository": "someone/else", "changes": []},
        ],
    });
    codex_says(&e, &reply.to_string(), "completed").await;
    let note = last(&e.snapshot().await);
    assert_eq!(note.proposals.len(), 1);
    let p = &note.proposals[0];
    assert_eq!(p.repo.as_deref(), Some("acme/api"));
    assert!(p.connect);
    // Nothing is connected before a tap.
    assert!(e.snapshot().await.settings.repositories.is_empty());
}

/// Waits for Bit's latest note to stop waiting for its answer.
async fn answer_of(e: &Engine) -> Note {
    for _ in 0..200 {
        let note = last(&e.snapshot().await);
        if !note.pending {
            return note;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("Bit never answered");
}
fn bit_answer() -> Value {
    json!({
        "answer": "b#1 is waiting for its author to push a fix.",
        "proposals": [{"action": "stop_loop", "pr": "a/b#1", "changes": []}],
        "memory": {"forget": [], "remember": []},
    })
}
/// The fixture's engine, with Bit answering with `answerer`.
async fn answering_with(answerer: &str) -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>) {
    let (dir, e, rpc) = fixture().await;
    e.update(|s| {
        s.settings.buddy.answerer = answerer.into();
        s.claude.ready = true;
        s.antigravity.ready = true;
        s.settings.antigravity_model = "gemini-3.1-pro-high".into();
        s.antigravity_models = crate::antigravity::catalog("gemini-3.1-pro-high (Gemini)");
    })
    .await;
    (dir, e, rpc)
}

#[tokio::test]
async fn claude_code_answers_with_no_tools_and_the_same_answer_shape() {
    let (dir, e, rpc) = answering_with("claude").await;
    let log = dir.path().join("claude-log");
    std::fs::write(log.with_extension("answer"), bit_answer().to_string()).unwrap();
    e.use_claude(crate::claude::tests::fake(&format!("ok@{}", log.display())));
    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    let note = answer_of(&e).await;
    assert_eq!(note.text, "b#1 is waiting for its author to push a fix.");
    assert_eq!(note.proposals[0].action, Action::StopLoop);
    assert!(!note.proposals[0].done);

    let args: Vec<String> = serde_json::from_str(
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(after("--tools"), "");
    assert_eq!(after("--setting-sources"), "");
    assert!(args.contains(&"--strict-mcp-config".to_string()));
    assert_eq!(
        serde_json::from_str::<Value>(&after("--json-schema")).unwrap(),
        crate::ask::output_schema()
    );
    assert!(after("--append-system-prompt").starts_with("You are Bit,"));
    let selection = crate::models::claude_selection(&e.snapshot().await.settings);
    assert_eq!(after("--model"), selection.model);
    let input = std::fs::read_to_string(log.with_extension("stdin")).unwrap();
    assert!(input.starts_with("The person asks: What's up with b#1?"));
    // Codex isn't asked.
    assert!(sent(&rpc, "thread/start").await.is_empty());
}

#[tokio::test]
async fn antigravity_answers_in_its_sandbox_with_bits_instructions_first() {
    let (dir, e, rpc) = answering_with("antigravity").await;
    let log = dir.path().join("agy-log");
    std::fs::write(log.with_extension("answer"), bit_answer().to_string()).unwrap();
    e.use_antigravity(crate::antigravity::tests::fake(
        &format!("ok@{}", log.display()),
        &dir.path().join("agy-settings.json"),
    ));
    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    let note = answer_of(&e).await;
    assert_eq!(note.text, "b#1 is waiting for its author to push a fix.");

    let call: Value = serde_json::from_str(
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let args: Vec<String> = serde_json::from_value(call["args"].clone()).unwrap();
    assert!(args.contains(&"--sandbox".to_string()));
    let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(after("--model"), "gemini-3.1-pro-high");
    assert_eq!(
        serde_json::from_str::<Value>(&after("--json-schema")).unwrap(),
        crate::ask::output_schema()
    );
    let prompt = call["prompt"].as_str().unwrap();
    assert!(prompt.starts_with("You are Bit,"));
    // Its own folder, emptied for each question.
    let folder = dir.path().join("state").join("bit-antigravity");
    std::fs::write(folder.join("left-behind"), "from the last answer").unwrap();
    e.ask_buddy_anything("And now?").await.unwrap();
    answer_of(&e).await;
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    assert!(prompt.contains("The person asks: What's up with b#1?"));
    assert!(sent(&rpc, "thread/start").await.is_empty());
}

#[tokio::test]
async fn antigravity_doesnt_answer_when_it_wouldnt_ask_first() {
    let (dir, e, _rpc) = answering_with("antigravity").await;
    e.use_antigravity(crate::antigravity::tests::fake(
        "always-proceed",
        &dir.path().join("agy-settings.json"),
    ));
    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    let note = answer_of(&e).await;
    assert_eq!(note.kind, NoteKind::Answer);
    assert!(
        note.text.contains("permission mode: always-proceed"),
        "{}",
        note.text
    );
    assert!(note.proposals.is_empty());
}

#[tokio::test]
async fn bit_says_when_the_answerer_you_chose_isnt_ready_and_picks_the_one_that_is() {
    let (_dir, e, _rpc) = answering_with("claude").await;
    e.update(|s| {
        s.claude.ready = false;
        s.claude.error = Some("Claude is not signed in.".into());
    })
    .await;
    // Codex and Antigravity are both ready, so you choose.
    let (lines, buttons) = cant_answer(&e, "Hi?").await;
    assert_eq!(
        lines,
        [
            "Claude isn't connected. Claude is not signed in.",
            "Codex and Antigravity are ready, so tap one to answer with it instead.",
        ]
    );
    assert_eq!(
        buttons,
        [
            "Get Claude Code",
            "Answer with Codex",
            "Answer with Antigravity",
            "CheckConnections"
        ]
    );
    assert_eq!(e.snapshot().await.settings.buddy.answerer, "claude");
    let note = last(&e.snapshot().await).id;
    e.use_proposal(note, 2).await.unwrap();
    assert_eq!(e.snapshot().await.settings.buddy.answerer, "antigravity");

    // With only Codex signed in, Bit switches to it, says so, and answers.
    e.update(|s| {
        s.settings.buddy.answerer = "claude".into();
        s.antigravity.ready = false;
    })
    .await;
    e.ask_buddy_anything("Hi again?").await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.buddy.answerer, "codex");
    let n = s.buddy_feed.notes.len();
    let switched = &s.buddy_feed.notes[n - 2];
    assert_eq!(switched.kind, NoteKind::Fixed);
    assert_eq!(
        switched.text,
        "Claude isn't ready, but Codex is, so I'll answer your questions with Codex from now on! You can change that in Settings, under Buddy."
    );
    assert_eq!(
        s.buddy_feed.notes[n - 1].question.as_deref(),
        Some("Hi again?")
    );
    assert!(e.asking.lock().unwrap().is_some());
}

#[tokio::test]
async fn codex_disconnecting_leaves_another_answerer_alone_and_ending_it_stops_it() {
    let (dir, e, _rpc) = answering_with("claude").await;
    let log = dir.path().join("claude.log");
    e.use_claude(crate::claude::tests::fake(&format!(
        "slow@{}",
        log.display()
    )));
    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    // Claude is running, with a command of its own.
    while !log.with_extension("log.child").exists() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    e.end_codex_question("Codex disconnected.").await;
    assert!(last(&e.snapshot().await).pending);
    let note = last(&e.snapshot().await).id;
    e.answer_done(note, Err("Claude took too long.".into()))
        .await;
    let note = last(&e.snapshot().await);
    assert!(!note.pending);
    assert!(note.text.contains("Claude took too long."), "{}", note.text);
    assert!(e.asking.lock().unwrap().is_none());
    super::test_support::child_ends(&log).await;
}

#[tokio::test]
async fn an_answer_from_another_model_isnt_used() {
    for (answerer, mode, reported) in [
        ("claude", "wrong-model", "claude-other"),
        ("claude", "no-model", "none"),
        ("antigravity", "wrong-model", "gemini-other"),
        ("antigravity", "no-model", "none"),
    ] {
        let (dir, e, _rpc) = answering_with(answerer).await;
        let log = dir.path().join("cli-log");
        let reply = json!({
            "answer": "From the wrong model.",
            "proposals": [{"action": "sync_now", "pr": null, "changes": []}],
            "memory": {"forget": [], "remember": [{"repository": null, "line": "Wrong model was here."}]},
        });
        std::fs::write(log.with_extension("answer"), reply.to_string()).unwrap();
        let fake = format!("{mode}@{}", log.display());
        match answerer {
            "claude" => e.use_claude(crate::claude::tests::fake(&fake)),
            _ => e.use_antigravity(crate::antigravity::tests::fake(
                &fake,
                &dir.path().join("agy-settings.json"),
            )),
        }
        e.ask_buddy_anything("What's up with b#1?").await.unwrap();
        let note = answer_of(&e).await;
        let s = e.snapshot().await;
        let model = s
            .settings
            .selection(s.settings.buddy.answered_by().provider().unwrap())
            .model;
        assert!(
            note.text.contains(&format!(
                "didn't confirm the model '{model}' (reported: {reported})"
            )),
            "{answerer} {mode}: {}",
            note.text
        );
        assert!(note.proposals.is_empty());
        assert!(note.memory.is_empty());
        assert!(s.buddy_feed.memory.general.is_empty());
    }
}

/// The fixture's engine, with Bit answering with a fake Ollama.
async fn answering_with_ollama(
    fake: crate::ollama::tests::Fake,
) -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>) {
    let (dir, e, rpc) = fixture().await;
    let url = crate::ollama::tests::serve(fake).await;
    let patch: SettingsPatch = serde_json::from_value(json!({"buddy": {
        "answerer": "ollama",
        "ollamaUrl": url,
        "ollamaModel": "llama3.2:3b",
    }}))
    .unwrap();
    e.update_settings(patch).await.unwrap();
    (dir, e, rpc)
}
fn ollama_with(content: &str) -> crate::ollama::tests::Fake {
    crate::ollama::tests::Fake {
        models: vec!["llama3.2:3b".into(), "qwen3:8b".into()],
        content: content.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn ollama_answers_with_bits_instructions_and_the_answer_form() {
    let fake = ollama_with(&bit_answer().to_string());
    let chats = fake.chats.clone();
    let (_dir, e, rpc) = answering_with_ollama(fake).await;
    // Choosing Ollama checked it straight away.
    let s = e.snapshot().await;
    assert!(s.ollama.ready, "{:?}", s.ollama.error);
    assert_eq!(s.ollama_models, ["llama3.2:3b", "qwen3:8b"]);

    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    let note = answer_of(&e).await;
    assert_eq!(note.text, "b#1 is waiting for its author to push a fix.");
    assert_eq!(note.proposals[0].action, Action::StopLoop);
    let chat = chats.lock().unwrap()[0].clone();
    assert_eq!(chat["model"], "llama3.2:3b");
    assert_eq!(chat["format"], crate::ask::output_schema());
    assert!(chat["messages"][0]["content"]
        .as_str()
        .unwrap()
        .starts_with("You are Bit,"));
    assert!(chat["messages"][1]["content"]
        .as_str()
        .unwrap()
        .starts_with("The person asks: What's up with b#1?"));
    assert!(sent(&rpc, "thread/start").await.is_empty());

    // Back to Codex, Ollama is no longer checked.
    let patch: SettingsPatch =
        serde_json::from_value(json!({"buddy": {"answerer": "codex"}})).unwrap();
    e.update_settings(patch).await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.ollama, CliStatus::default());
    assert!(s.ollama_models.is_empty());
}

#[tokio::test]
async fn bit_says_what_ollama_needs_before_asking_it() {
    let (_dir, e, _rpc) = answering_with_ollama(ollama_with("{}")).await;
    let patch: SettingsPatch =
        serde_json::from_value(json!({"buddy": {"ollamaModel": "mistral"}})).unwrap();
    e.update_settings(patch).await.unwrap();
    // Never switched away from Ollama, even with Codex signed in.
    let (lines, buttons) = cant_answer(&e, "Hi?").await;
    assert_eq!(
        lines,
        [
            "I answer your questions with Ollama, which isn't ready: Ollama doesn't have model 'mistral'. Choose one in Settings.",
            "Codex is ready, so tap Answer with Codex to use it instead.",
        ]
    );
    assert_eq!(buttons, ["Answer with Codex", "CheckConnections"]);
    assert_eq!(e.snapshot().await.settings.buddy.answerer, "ollama");
    // Nothing at the address.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let patch: SettingsPatch =
        serde_json::from_value(json!({"buddy": {"ollamaUrl": gone}})).unwrap();
    e.update_settings(patch).await.unwrap();
    let (lines, buttons) = cant_answer(&e, "Hi?").await;
    assert!(
        lines[0].contains(&format!("Nothing answered at {gone}.")),
        "{lines:?}"
    );
    assert_eq!(buttons[0], "Get Ollama");
    // A server with no models.
    let empty = crate::ollama::tests::serve(Default::default()).await;
    let patch: SettingsPatch =
        serde_json::from_value(json!({"buddy": {"ollamaUrl": empty}})).unwrap();
    e.update_settings(patch).await.unwrap();
    let (lines, _) = cant_answer(&e, "Hi?").await;
    assert!(lines[0].contains("has no models yet"), "{lines:?}");
}

#[tokio::test]
async fn an_ollama_answer_from_another_model_isnt_used_and_ending_a_question_stops_it() {
    let mut fake = ollama_with(&bit_answer().to_string());
    fake.reported = Some("qwen3:8b".into());
    let (_dir, e, _rpc) = answering_with_ollama(fake).await;
    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    let note = answer_of(&e).await;
    assert!(
        note.text
            .contains("didn't confirm the model 'llama3.2:3b' (reported: qwen3:8b)"),
        "{}",
        note.text
    );
    assert!(note.proposals.is_empty());

    let mut slow = ollama_with(&bit_answer().to_string());
    slow.delay = std::time::Duration::from_secs(30);
    let chats = slow.chats.clone();
    let (_dir, e, _rpc) = answering_with_ollama(slow).await;
    e.ask_buddy_anything("What's up with b#1?").await.unwrap();
    while chats.lock().unwrap().is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    e.end_codex_question("Codex disconnected.").await;
    assert!(last(&e.snapshot().await).pending);
    let note = last(&e.snapshot().await).id;
    e.answer_done(note, Err("Ollama took too long.".into()))
        .await;
    assert!(e.asking.lock().unwrap().is_none());
    let note = last(&e.snapshot().await);
    assert!(note.text.contains("Ollama took too long."), "{}", note.text);
}

/// The fixture's engine, with Bit answering with your key for `api`, kept
/// in a password store in memory, at fake providers.
async fn answering_with_api(
    api: crate::api::Api,
    model: &str,
    fake: crate::api::tests::Fake,
) -> (
    tempfile::TempDir,
    Arc<Engine>,
    Arc<crate::api::tests::Memory>,
) {
    let (dir, e, _rpc) = fixture().await;
    let base = crate::api::tests::serve(fake).await;
    let store = Arc::new(crate::api::tests::Memory::default());
    *e.apis.lock().unwrap() = crate::api::Setup {
        secrets: store.clone(),
        bases: [base.clone(), base.clone(), base],
    };
    let mut buddy = json!({"answerer": api.key()});
    buddy[format!("{}Model", api.key())] = json!(model);
    let patch: SettingsPatch = serde_json::from_value(json!({ "buddy": buddy })).unwrap();
    e.update_settings(patch).await.unwrap();
    (dir, e, store)
}
fn provider_with(content: &str) -> crate::api::tests::Fake {
    crate::api::tests::Fake {
        models: ["gpt-5", "claude-opus-5-5", "gemini-2.5-flash"]
            .map(String::from)
            .to_vec(),
        content: content.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn an_api_key_answers_with_bits_instructions_and_stays_out_of_orbits_files() {
    use crate::api::{tests::KEY, Api};
    for (api, model) in [
        (Api::OpenAi, "gpt-5"),
        (Api::Anthropic, "claude-opus-5-5"),
        (Api::Google, "gemini-2.5-flash"),
    ] {
        let fake = provider_with(&bit_answer().to_string());
        let requests = fake.requests.clone();
        let (dir, e, store) = answering_with_api(api, model, fake).await;
        // Without a key, Bit says where to get and save one.
        let (lines, buttons) = cant_answer(&e, "Hi?").await;
        assert!(
            lines[0].ends_with("then save it in Settings, under Buddy."),
            "{lines:?}"
        );
        assert!(buttons[0].starts_with("Get "), "{buttons:?}");
        e.set_api_key(api.key(), Some(format!("  {KEY}\n")))
            .await
            .unwrap();
        let s = e.snapshot().await;
        let saved = s.api_keys.get(api);
        assert!(saved.saved);
        assert!(saved.status.ready);
        assert!(saved.models.contains(&model.to_string()));

        e.ask_buddy_anything("What's up with b#1?").await.unwrap();
        let note = answer_of(&e).await;
        assert_eq!(note.text, "b#1 is waiting for its author to push a fix.");
        assert_eq!(note.proposals[0].action, Action::StopLoop);
        let sent = requests.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent.1, KEY);
        let body = sent.2.to_string();
        assert!(body.contains("You are Bit,"), "{body}");
        assert!(body.contains("The person asks: What's up with b#1?"));

        // The key is only in the password store.
        assert_eq!(store.keys.lock().unwrap().len(), 1);
        let _ = e.flush().await;
        let mut folders = vec![dir.path().to_path_buf()];
        while let Some(folder) = folders.pop() {
            for file in std::fs::read_dir(folder).unwrap() {
                let path = file.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).to_string();
                    assert!(!text.contains(KEY), "{}", path.display());
                }
            }
        }
        assert!(!serde_json::to_string(&s).unwrap().contains(KEY));

        // Removing it.
        e.set_api_key(api.key(), None).await.unwrap();
        assert!(store.keys.lock().unwrap().is_empty());
        assert_eq!(e.snapshot().await.api_keys.get(api), &ApiKey::default());
    }
}

#[tokio::test]
async fn bit_says_what_an_api_key_needs_before_using_it() {
    use crate::api::{tests::KEY, Api};
    let (_dir, e, store) = answering_with_api(Api::OpenAi, "gpt-9", provider_with("{}")).await;
    for key in ["", "sk test 1234567"] {
        assert!(e.set_api_key("openai", Some(key.into())).await.is_err());
    }
    assert!(e.set_api_key("mistral", Some(KEY.into())).await.is_err());
    assert!(store.keys.lock().unwrap().is_empty());

    e.set_api_key("openai", Some(KEY.into())).await.unwrap();
    assert_eq!(
        cant_answer(&e, "Hi?").await.0[0],
        "I answer your questions with the OpenAI API, which isn't ready: OpenAI doesn't offer model 'gpt-9' to your key. Choose one in Settings."
    );
    let patch: SettingsPatch =
        serde_json::from_value(json!({"buddy": {"openaiModel": ""}})).unwrap();
    e.update_settings(patch).await.unwrap();
    assert!(cant_answer(&e, "Hi?").await.0[0]
        .ends_with("Choose a model for your OpenAI key in Settings."));

    // A key the provider turns down.
    e.set_api_key("openai", Some("sk-revoked-0000".into()))
        .await
        .unwrap();
    let s = e.snapshot().await;
    assert!(s.api_keys.openai.saved);
    assert!(s
        .api_keys
        .openai
        .status
        .error
        .as_deref()
        .is_some_and(|e| e.contains("OpenAI didn't accept your key")));
    let (lines, _) = cant_answer(&e, "Hi?").await;
    assert!(lines[0].contains("didn't accept your key"), "{lines:?}");
    let help = crate::guide::connections(&s);
    let c = help.iter().find(|c| c.key == "openai-api").unwrap();
    assert!(!c.ready && c.for_bit);
    assert_eq!(c.page, "https://platform.openai.com/api-keys");

    // A key that went from the password store isn't counted as saved.
    store.keys.lock().unwrap().clear();
    e.check_bit_answerer().await;
    assert_eq!(e.snapshot().await.api_keys.openai, ApiKey::default());
}

#[tokio::test]
async fn a_password_store_that_wont_open_says_so() {
    use crate::api::{tests::KEY, Api};
    let (_dir, e, _store) =
        answering_with_api(Api::Anthropic, "claude-opus-5-5", provider_with("{}")).await;
    e.apis.lock().unwrap().secrets = Arc::new(crate::api::tests::Memory {
        broken: Some("Orbit couldn't use your system's password store: locked".into()),
        ..Default::default()
    });
    assert_eq!(
        e.set_api_key("anthropic", Some(KEY.into()))
            .await
            .unwrap_err(),
        "Orbit couldn't use your system's password store: locked"
    );
    assert!(!e.snapshot().await.api_keys.anthropic.saved);
}
