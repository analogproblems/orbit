use super::{sign_in::github_prompt, Engine};
use crate::{
    ask::Action,
    buddy::{Note, NoteKind},
};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};

fn engine(dir: &Path) -> Arc<Engine> {
    Engine::new(dir.join("state"), |_| {}).unwrap()
}
/// A stand-in for `gh` whose sign-in gives a code, then is refused.
fn refusing_gh(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("gh");
    std::fs::write(
        &path,
        r#"#!/bin/sh
echo "$*" >> "$(dirname "$0")/log"
echo "! First copy your one-time code: ABCD-1234" >&2
echo "Open this URL to continue in your web browser: https://github.com/login/device" >&2
sleep 1
echo "authorization was denied" >&2
exit 1
"#,
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
    // Not until it has said both, and never a page Orbit doesn't know.
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
    let dir = tempfile::tempdir().unwrap();
    let gh = refusing_gh(dir.path());
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

#[tokio::test]
async fn codex_says_only_its_own_sign_in_finished() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (finished, mut ended) = tokio::sync::oneshot::channel();
    *e.codex_login.lock().unwrap() = Some(("mine".into(), finished));
    e.codex_signed_in(&json!({"loginId": "theirs", "success": true}));
    assert!(ended.try_recv().is_err());
    e.codex_signed_in(&json!({"loginId": "mine", "success": false, "error": "Denied."}));
    assert_eq!(ended.try_recv().unwrap(), Err("Denied.".into()));
    assert!(e.codex_login.lock().unwrap().is_none());
}
