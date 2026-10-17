use super::{
    sign_in::{github_prompt, Waiting},
    Engine,
};
use crate::{
    ask::Action,
    buddy::{Note, NoteKind},
    rpc::Rpc,
};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc;

fn engine(dir: &Path) -> Arc<Engine> {
    Engine::new(dir.join("state"), |_| {}).unwrap()
}
/// A stand-in for `gh` whose sign-in gives a code, worded as `code` says
/// it, then is refused.
fn refusing_gh(dir: &Path, code: &str) -> std::path::PathBuf {
    let path = dir.join("gh");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/log"
echo "{code}" >&2
echo "Open this URL to continue in your web browser: https://github.com/login/device" >&2
sleep 1
echo "authorization was denied" >&2
exit 1
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
async fn last_note(e: &Engine) -> Note {
    e.snapshot().await.buddy_feed.notes.last().unwrap().clone()
}

#[test]
fn reads_the_code_and_page_from_what_gh_says() {
    let said = "\n! First copy your one-time code: 1A2B-3C4D\nOpen this URL to continue in your web browser: https://github.com/login/device\n";
    assert_eq!(
        github_prompt(said),
        Some(("1A2B-3C4D".into(), "https://github.com/login/device".into()))
    );
    // With gh set to copy it, the code comes in brackets.
    let copied = "! One-time code (1A2B-3C4D) copied to clipboard\nOpen this URL to continue in your web browser: https://github.com/login/device\n";
    assert_eq!(
        github_prompt(copied),
        Some(("1A2B-3C4D".into(), "https://github.com/login/device".into()))
    );
    // Not until it has said both, and never a page Orbit doesn't know or
    // a code that isn't one.
    assert_eq!(
        github_prompt(
            "! One-time code (`rm -rf`) copied\nweb browser: https://github.com/login/device\n"
        ),
        None
    );
    assert_eq!(
        github_prompt("! First copy your one-time code: 1A2B-3C4D\n"),
        None
    );
    assert_eq!(
        github_prompt("one-time code: X\nweb browser: https://example.com/login\n"),
        None
    );
}

#[tokio::test]
async fn a_github_sign_in_shows_its_code_and_says_how_it_ended() {
    for code in [
        "! First copy your one-time code: ABCD-1234",
        "! One-time code (ABCD-1234) copied to clipboard",
    ] {
        github_sign_in(code).await;
    }
}
async fn github_sign_in(code: &str) {
    let dir = tempfile::tempdir().unwrap();
    let gh = refusing_gh(dir.path(), code);
    let e = engine(dir.path());
    crate::github::GH
        .scope(gh.clone(), e.sign_in("github"))
        .await
        .unwrap();
    let note = last_note(&e).await;
    assert_eq!(note.kind, NoteKind::SignIn);
    assert_eq!(note.lines[0].text, "Your code: `ABCD-1234`");
    let page = &note.proposals[0];
    assert_eq!(page.action, Action::OpenPage);
    assert_eq!(
        page.link.as_deref(),
        Some("https://github.com/login/device")
    );
    assert_eq!(page.label.as_deref(), Some("Open github.com/login/device"));
    let log = std::fs::read_to_string(dir.path().join("log")).unwrap();
    assert_eq!(
        log.trim(),
        "auth login --web --hostname github.com --git-protocol https --skip-ssh-key"
    );

    // One sign-in at a time.
    let again = crate::github::GH.scope(gh, e.sign_in("github")).await;
    assert!(again.unwrap_err().contains("waiting for you already"));

    for _ in 0..50 {
        if last_note(&e).await.id != note.id {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let ended = last_note(&e).await;
    assert_eq!(ended.kind, NoteKind::SignIn);
    assert!(
        ended
            .text
            .starts_with("Signing in to GitHub didn't finish: authorization was denied"),
        "{}",
        ended.text
    );
    assert!(e.signing_in.lock().unwrap().is_empty());
    // The code is no use any more.
    let s = e.snapshot().await;
    let old = s.buddy_feed.notes.iter().find(|n| n.id == note.id).unwrap();
    assert_eq!(old.lines[0].text, "That code is no longer needed.");
    assert!(old.proposals.is_empty());
}

#[tokio::test]
async fn only_github_and_codex_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    assert!(e.sign_in("claude").await.is_err());
    // Codex that can't start says so, and nothing is left waiting.
    let error = e.sign_in("codex").await.unwrap_err();
    assert!(error.contains("Install the Codex CLI"), "{error}");
    assert!(e.signing_in.lock().unwrap().is_empty());
}

/// A fake Codex, connected as the engine's current connection.
async fn fake_codex(e: &Arc<Engine>) -> Arc<Rpc> {
    let (tx, rx) = mpsc::channel(64);
    let mut cmd = tokio::process::Command::new("node");
    cmd.args(["-e", include_str!("../model_fixture.cjs")]);
    let rpc = Rpc::from_command(cmd, tx).await.unwrap();
    *e.rpc.lock().await = Some(rpc.clone());
    tokio::spawn(e.clone().pump(rpc.clone(), rx));
    rpc
}
/// Codex that isn't signed in, and gives `login` when asked to sign in.
async fn signed_out(rpc: &Rpc, login: &str) {
    rpc.request(
        "fixture/configure",
        json!({
            "errors": {"account/read": "Codex is not signed in."},
            "responses": {"account/login/start": {
                "loginId": login,
                "userCode": "WXYZ-98765",
                "verificationUrl": "https://auth.openai.com/codex/device",
            }},
        }),
    )
    .await
    .unwrap();
}
async fn until(what: impl Fn() -> bool) {
    for _ in 0..50 {
        if what() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("it never happened");
}

#[tokio::test]
async fn codex_says_only_its_own_sign_in_finished() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let waiting = |login: &str| {
        let (finished, ended) = tokio::sync::oneshot::channel();
        let w = Waiting {
            login: login.into(),
            finished,
            rpc: Default::default(),
        };
        (w, ended)
    };
    let (w, mut ended) = waiting("mine");
    *e.codex_login.lock().unwrap() = Some(w);
    e.codex_signed_in(&json!({"loginId": "theirs", "success": true}));
    assert!(ended.try_recv().is_err());
    e.codex_signed_in(&json!({"loginId": "mine", "success": false, "error": "Denied."}));
    assert_eq!(ended.try_recv().unwrap(), Err("Denied.".into()));
    assert!(e.codex_login.lock().unwrap().is_none());
    // Codex may leave the id out, or have nothing to say.
    for p in [
        json!({"loginId": null, "success": true}),
        json!({"success": false}),
    ] {
        let (w, mut ended) = waiting("mine");
        *e.codex_login.lock().unwrap() = Some(w);
        e.codex_signed_in(&p);
        let said = ended.try_recv().unwrap();
        assert_eq!(said.is_ok(), p["success"] == true, "{p}");
    }
    // With nothing waiting, it changes nothing.
    e.codex_signed_in(&json!({"loginId": "late", "success": true}));
    assert!(e.codex_login.lock().unwrap().is_none());
}

#[tokio::test]
async fn a_codex_sign_in_ends_with_its_connection_and_can_start_again() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let rpc = fake_codex(&e).await;
    signed_out(&rpc, "login-1").await;
    e.sign_in("codex").await.unwrap();
    let code = last_note(&e).await;
    assert_eq!(code.kind, NoteKind::SignIn);
    assert_eq!(code.lines[0].text, "Your code: `WXYZ-98765`");
    assert_eq!(
        code.proposals[0].link.as_deref(),
        Some("https://auth.openai.com/codex/device")
    );
    assert!(e.sign_in("codex").await.unwrap_err().contains("waiting"));

    // Another connection going away leaves it waiting.
    let other = {
        let (tx, _rx) = mpsc::channel(64);
        let mut cmd = tokio::process::Command::new("node");
        cmd.args(["-e", include_str!("../model_fixture.cjs")]);
        Rpc::from_command(cmd, tx).await.unwrap()
    };
    e.codex_login_lost(Some(&other));
    assert!(e.codex_login.lock().unwrap().is_some());

    // Its own going away ends it straight away.
    e.disconnected().await;
    until(|| e.signing_in.lock().unwrap().is_empty()).await;
    let s = e.snapshot().await;
    let ended = s.buddy_feed.notes.last().unwrap();
    assert!(
        ended
            .text
            .starts_with("Signing in to Codex didn't finish: Codex stopped"),
        "{}",
        ended.text
    );
    let old = s.buddy_feed.notes.iter().find(|n| n.id == code.id).unwrap();
    assert!(old.proposals.is_empty());

    // Codex is back, and a new sign-in starts at once and finishes.
    signed_out(&rpc, "login-2").await;
    e.sign_in("codex").await.unwrap();
    assert_eq!(
        e.codex_login
            .lock()
            .unwrap()
            .as_ref()
            .map(|w| w.login.clone()),
        Some("login-2".into())
    );
    rpc.request("fixture/configure", json!({})).await.unwrap();
    e.event(json!({"method": "account/login/completed",
        "params": {"loginId": "login-2", "success": true}}))
        .await;
    until(|| e.signing_in.lock().unwrap().is_empty()).await;
    let done = last_note(&e).await;
    assert_eq!(done.kind, NoteKind::Fixed);
    assert_eq!(done.text, "You're signed in to Codex. All good!");
    assert!(e.snapshot().await.codex_ready);
}
