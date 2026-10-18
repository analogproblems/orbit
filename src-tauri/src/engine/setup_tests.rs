use super::Engine;
use crate::{
    ask::{Action, Proposal},
    buddy::{answered, asked, NoteKind, Question},
    setup::RepositoryPatch,
    types::*,
};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

/// A stand-in for `gh` that lists `repos.json` and clones a repository by
/// making an empty Git working tree whose origin is it.
pub(super) fn fake_gh(dir: &Path) -> std::path::PathBuf {
    let d = dir.display();
    let script = format!(
        r#"#!/bin/sh
echo "$*" >> {d}/log
if [ "$1 $2" = "repo clone" ]; then
  [ -e {d}/clone-fails ] && {{ echo "repository not found" >&2; exit 1; }}
  [ -e {d}/slow ] && sleep 1
  if [ -e {d}/gate ]; then
    touch {d}/cloning
    while [ ! -e {d}/release ]; do sleep 0.05; done
  fi
  mkdir -p "$4" && cd "$4" && git init -q && git remote add origin "https://github.com/$3.git" && touch package-lock.json
  exit $?
fi
case "$*" in
  *user/repos*) cat {d}/repos.json ;;
  *) echo '[]' ;;
esac
"#
    );
    let path = dir.join("gh");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        dir.join("repos.json"),
        serde_json::json!([
            {"full_name": "acme/api", "private": true, "description": "The API", "pushed_at": "2026-10-11T00:00:00Z"},
            {"full_name": "a/b", "private": false, "description": null, "pushed_at": "2026-10-10T00:00:00Z"},
        ])
        .to_string(),
    )
    .unwrap();
    path
}
fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("log")).unwrap_or_default()
}
/// An answer offering to set up `repo`, and its note.
async fn offered(e: &Engine, repo: &str, patch: RepositoryPatch, connect: bool) -> u64 {
    let mut s = e.state.lock().await;
    let id = asked(&mut s, "Can you set it up?");
    let p = Proposal {
        id: 0,
        action: Action::SetUpRepository,
        pr_id: None,
        review_id: None,
        settings: None,
        repo: Some(repo.into()),
        repository: Some(patch),
        connect,
        service: None,
        link: None,
        label: None,
        done: false,
    };
    answered(&mut s, id, "Tap below.".into(), vec![p], vec![]);
    id
}
fn engine(dir: &Path) -> Arc<Engine> {
    Engine::new(dir.join("state"), |_| {}).unwrap()
}

#[tokio::test]
async fn a_tap_downloads_orbits_own_copy_and_connects_it() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    let patch = RepositoryPatch {
        setup_command: Some("npm ci".into()),
        network_access: Some(true),
        ..Default::default()
    };
    let note = offered(&e, "acme/api", patch, true).await;
    crate::github::GH
        .scope(gh.clone(), e.use_proposal(note, 0))
        .await
        .unwrap();
    // Downloaded beside, then moved into place.
    let download = dir.path().join("state/downloads/acme/api");
    assert!(log(dir.path()).contains(&format!("repo clone acme/api {}", download.display())));
    assert!(!download.exists());
    let folder = dir.path().join("state/clones/acme/api");
    assert!(folder.join("package-lock.json").is_file());
    let s = e.snapshot().await;
    let repo = &s.settings.repositories[0];
    assert_eq!(repo.name, "acme/api");
    assert_eq!(
        Path::new(&repo.path),
        std::fs::canonicalize(&folder).unwrap()
    );
    // Orbit's defaults, then the change.
    assert!(!repo.automatic);
    assert!(repo.keep_updated);
    assert_eq!(repo.provider, Provider::Codex);
    assert_eq!(repo.setup_command, "npm ci");
    assert!(repo.network_access);
    let answer = s.buddy_feed.notes.iter().find(|n| n.id == note).unwrap();
    assert!(answer.proposals[0].done);

    // A later change goes to the connected repository; no new copy.
    std::fs::remove_file(dir.path().join("log")).unwrap();
    let patch = RepositoryPatch {
        round_limit: Some(2),
        ..Default::default()
    };
    let note = offered(&e, "ACME/API", patch, false).await;
    crate::github::GH
        .scope(gh, e.use_proposal(note, 0))
        .await
        .unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.repositories.len(), 1);
    assert_eq!(s.settings.repositories[0].round_limit, 2);
    assert_eq!(s.settings.repositories[0].setup_command, "npm ci");
    assert_eq!(log(dir.path()), "");
}

#[tokio::test]
async fn a_copy_orbit_already_has_is_used_and_a_broken_one_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());

    // A download that fails leaves nothing connected, and the button can
    // be tapped again.
    std::fs::write(dir.path().join("clone-fails"), "").unwrap();
    let note = offered(&e, "acme/api", Default::default(), true).await;
    let tap = || crate::github::GH.scope(gh.clone(), e.use_proposal(note, 0));
    let error = tap().await.unwrap_err();
    assert!(
        error.starts_with("Orbit couldn't download acme/api: ")
            && error.contains("repository not found"),
        "{error}"
    );
    assert!(e.snapshot().await.settings.repositories.is_empty());

    // What's left of a download cut short is thrown away, never connected.
    std::fs::remove_file(dir.path().join("clone-fails")).unwrap();
    let download = dir.path().join("state/downloads/acme/api");
    std::fs::create_dir_all(download.join(".git")).unwrap();
    std::fs::write(download.join("half-written"), "").unwrap();
    let again = offered(&e, "acme/api", Default::default(), true).await;
    crate::github::GH
        .scope(gh.clone(), e.use_proposal(again, 0))
        .await
        .unwrap();
    let folder = dir.path().join("state/clones/acme/api");
    assert!(!folder.join("half-written").exists());
    assert!(folder.join("package-lock.json").is_file());
    assert_eq!(e.snapshot().await.settings.repositories.len(), 1);
    e.update(|s| s.settings.repositories.clear()).await;
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::write(dir.path().join("clone-fails"), "").unwrap();

    // A folder there that isn't a copy of it isn't connected.
    std::fs::create_dir_all(&folder).unwrap();
    let error = tap().await.unwrap_err();
    assert!(
        error.starts_with(&format!(
            "Orbit's copy of acme/api, in {}, couldn't be connected: ",
            folder.display()
        )),
        "{error}"
    );
    assert!(e.snapshot().await.settings.repositories.is_empty());

    // A good copy already there is used as it is.
    std::fs::remove_file(dir.path().join("clone-fails")).unwrap();
    std::fs::remove_dir(&folder).unwrap();
    crate::github::GH
        .scope(gh.clone(), crate::github::clone("acme/api", &folder))
        .await
        .unwrap();
    std::fs::remove_file(dir.path().join("log")).unwrap();
    tap().await.unwrap();
    assert!(!log(dir.path()).contains("repo clone"));
    assert_eq!(e.snapshot().await.settings.repositories.len(), 1);
}

#[tokio::test]
async fn a_change_is_checked_again_when_tapped() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let clone = dir.path().join("clone");
    std::fs::create_dir(&clone).unwrap();
    e.update(|s| {
        let mut repo = Repository::new("a/b", &clone.to_string_lossy());
        repo.setup_command = "npm ci".into();
        s.settings.repositories.push(repo);
    })
    .await;
    // Claude can't run a setup command outside a dev container.
    let patch = RepositoryPatch {
        reviewer: Some(Provider::Claude),
        ..Default::default()
    };
    let note = offered(&e, "a/b", patch, false).await;
    let error = e.use_proposal(note, 0).await.unwrap_err();
    assert!(
        error.starts_with("Claude reviews can't run a setup command"),
        "{error}"
    );
    let s = e.snapshot().await;
    assert_eq!(s.settings.repositories[0].provider, Provider::Codex);
    assert!(!s.buddy_feed.notes.last().unwrap().proposals[0].done);
}

#[tokio::test]
async fn bit_looks_up_each_copy_and_what_you_could_connect() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    std::fs::write(copy.join("Cargo.lock"), "").unwrap();
    let connected = vec![
        Repository::new("A/B", &copy.to_string_lossy()),
        Repository::new("gone/repo", "/nowhere/at/all"),
    ];
    let lookups = crate::github::GH
        .scope(gh.clone(), e.look_up(connected.clone(), true))
        .await;
    assert_eq!(
        lookups.clones["A/B"].as_ref().unwrap().files,
        ["Cargo.lock"]
    );
    assert_eq!(
        lookups.clones["gone/repo"],
        Err("Orbit can't find this repository's copy.".into())
    );
    // Connected repositories aren't offered again.
    assert_eq!(lookups.connectable_names(), ["acme/api"]);

    // Without GitHub, the summary says why.
    let lookups = e.look_up(connected, true).await;
    let error = lookups.connectable.unwrap_err();
    assert!(error.starts_with("Cannot start gh"), "{error}");
}

#[tokio::test]
async fn a_download_carries_on_when_the_tap_goes_away() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    std::fs::write(dir.path().join("slow"), "").unwrap();
    let note = offered(&e, "acme/api", Default::default(), true).await;
    // Like a browser closed while the download runs.
    let tap = crate::github::GH.scope(gh, e.use_proposal(note, 0));
    let gone = tokio::time::timeout(std::time::Duration::from_millis(200), tap).await;
    assert!(gone.is_err());
    assert!(e.snapshot().await.settings.repositories.is_empty());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while e.snapshot().await.settings.repositories.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    // The button wasn't seen through, so it can be tapped again, which
    // finds the repository connected and changes nothing.
    let s = e.snapshot().await;
    assert!(!s.buddy_feed.notes.last().unwrap().proposals[0].done);
    e.use_proposal(note, 0).await.unwrap();
    assert_eq!(e.snapshot().await.settings.repositories.len(), 1);
}

#[tokio::test]
async fn a_settings_button_doesnt_connect_a_repository_disconnected_since() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    let copy = dir.path().join("copy");
    std::fs::create_dir(&copy).unwrap();
    e.update(|s| {
        let repo = Repository::new("acme/api", &copy.to_string_lossy());
        s.settings.repositories.push(repo);
    })
    .await;
    let patch = RepositoryPatch {
        round_limit: Some(2),
        autopilot: Some(true),
        ..Default::default()
    };
    let note = offered(&e, "acme/api", patch, false).await;
    // You disconnect it before tapping the old button.
    e.disconnect_repository("acme/api").await.unwrap();
    let error = crate::github::GH
        .scope(gh, e.use_proposal(note, 0))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "acme/api isn't connected any more. Ask Bit again if you want it connected."
    );
    assert_eq!(log(dir.path()), "");
    assert!(!dir.path().join("state/clones").exists());
    assert!(!dir.path().join("state/downloads").exists());
    let s = e.snapshot().await;
    assert!(s.settings.repositories.is_empty());
    assert!(!s.buddy_feed.notes.last().unwrap().proposals[0].done);
}

#[tokio::test]
async fn a_repository_connected_during_the_download_gets_the_change_instead() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    std::fs::write(dir.path().join("slow"), "").unwrap();
    let patch = RepositoryPatch {
        round_limit: Some(2),
        ..Default::default()
    };
    let note = offered(&e, "acme/api", patch, true).await;
    let tapped = e.clone();
    let tap = tokio::spawn(
        crate::github::GH.scope(gh, async move { tapped.use_proposal(note, 0).await }),
    );
    // Another window connects it while Orbit downloads its own copy.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    e.update(|s| {
        let repo = Repository::new("Acme/API", "/elsewhere/api");
        s.settings.repositories.push(repo);
    })
    .await;
    tap.await.unwrap().unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.repositories.len(), 1);
    let repo = &s.settings.repositories[0];
    assert_eq!(repo.path, "/elsewhere/api");
    assert_eq!(repo.round_limit, 2);
    assert!(s.buddy_feed.notes.last().unwrap().proposals[0].done);
}

#[tokio::test]
async fn help_me_set_up_offers_your_repositories_and_bit_follows_up() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    crate::github::GH
        .scope(gh.clone(), e.ask_buddy(Question::Setup))
        .await
        .unwrap();
    let s = e.snapshot().await;
    let answer = s.buddy_feed.notes.last().unwrap();
    assert_eq!(answer.question.as_deref(), Some("Help me set up"));
    let offered = |n: &crate::buddy::Note| -> Vec<(Action, Option<String>, bool)> {
        n.proposals
            .iter()
            .map(|p| (p.action, p.repo.clone(), p.connect))
            .collect()
    };
    // GitHub and Codex aren't connected in tests, so Bit's own buttons
    // come first: Claude Code, Antigravity, Ollama and the three API keys.
    let bits = || {
        let mut b = vec![(Action::OpenPage, None, false); 2];
        b.extend(vec![(Action::ChangeSettings, None, false); 4]);
        b
    };
    let mut expected = bits();
    expected.extend([
        (Action::CheckConnections, None, false),
        (Action::SetUpRepository, Some("acme/api".into()), true),
        (Action::SetUpRepository, Some("a/b".into()), true),
    ]);
    assert_eq!(offered(answer), expected);

    // Connecting one gets a note with its next steps.
    let note = answer.id;
    crate::github::GH
        .scope(gh.clone(), e.use_proposal(note, 7))
        .await
        .unwrap();
    let s = e.snapshot().await;
    let follow_up = s.buddy_feed.notes.last().unwrap();
    assert_eq!(follow_up.kind, NoteKind::Connected);
    assert_eq!(follow_up.repo.as_deref(), Some("acme/api"));
    assert_eq!(
        follow_up.text,
        "acme/api is connected! Here's what I'd set up next:"
    );
    assert!(follow_up.lines[0]
        .text
        .starts_with("acme/api has no setup command yet. Its copy has package-lock.json, so `npm ci --ignore-scripts`"));
    assert_eq!(
        offered(follow_up),
        [
            (Action::SetUpRepository, Some("acme/api".into()), false),
            (Action::SetUpRepository, Some("acme/api".into()), false),
            (Action::ChangeSettings, None, false),
        ]
    );
    let follow_up = follow_up.id;
    for button in [0, 1] {
        e.use_proposal(follow_up, button).await.unwrap();
    }
    let repo = &e.snapshot().await.settings.repositories[0];
    assert_eq!(repo.setup_command, "npm ci --ignore-scripts");
    assert!(repo.network_access);
    assert!(repo.automatic);

    // Asked again, only what's left is offered, without looking on GitHub.
    std::fs::remove_file(dir.path().join("log")).unwrap();
    crate::github::GH
        .scope(gh, e.ask_buddy(Question::Setup))
        .await
        .unwrap();
    let s = e.snapshot().await;
    let answer = s.buddy_feed.notes.last().unwrap();
    assert!(answer.lines.iter().any(|l| l.text
        == "acme/api sets up with `npm ci --ignore-scripts`. Its pull requests are reviewed automatically."));
    // The taps above synced without the stand-in, so gh couldn't start.
    assert!(answer
        .lines
        .iter()
        .any(|l| l.text.starts_with("GitHub CLI isn't installed")));
    let mut expected = bits();
    expected.extend([
        (Action::OpenPage, None, false),
        (Action::CheckConnections, None, false),
        (Action::ChangeSettings, None, false),
    ]);
    assert_eq!(offered(answer), expected);
    assert_eq!(log(dir.path()), "");
}

#[tokio::test]
async fn bit_follows_up_on_a_repository_added_in_repositories_only_while_on() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let (_repo_dir, repo) = super::test_support::git_repo().await;
    super::worktrees::git(
        &repo,
        &["remote", "add", "origin", "https://github.com/a/b.git"],
    )
    .await
    .unwrap();
    std::fs::write(repo.join("Cargo.lock"), "").unwrap();
    let added = Repository::new("a/b", &repo.to_string_lossy());
    e.save_repository(added.clone(), None).await.unwrap();
    let s = e.snapshot().await;
    let note = s.buddy_feed.notes.last().unwrap();
    assert_eq!(note.kind, NoteKind::Connected);
    assert!(
        note.lines[0].text.contains("`cargo fetch`"),
        "{:?}",
        note.lines
    );
    // Saving it again isn't news.
    e.save_repository(added.clone(), Some("a/b".into()))
        .await
        .unwrap();
    assert_eq!(
        e.snapshot().await.buddy_feed.notes.len(),
        s.buddy_feed.notes.len()
    );

    e.disconnect_repository("a/b").await.unwrap();
    e.update(|s| s.settings.buddy.enabled = false).await;
    let before = e.snapshot().await.buddy_feed.notes.len();
    e.save_repository(added, None).await.unwrap();
    assert_eq!(e.snapshot().await.buddy_feed.notes.len(), before);
}

#[tokio::test]
async fn a_reviewer_that_isnt_connected_needs_you_until_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path());
    let mut repo = Repository::new("a/b", "/tmp/a-b");
    repo.provider = Provider::Claude;
    e.update(|s| s.settings.repositories.push(repo)).await;
    let key = "connection:claude";
    let open = |s: &Snapshot| {
        crate::attention::items(s)
            .into_iter()
            .find(|a| a.key == key)
    };
    // Not checked yet, so not known to be off.
    assert_eq!(open(&e.snapshot().await), None);

    e.use_claude(crate::claude::tests::fake("signed-out"));
    let _ = e.check_claude().await;
    let item = open(&e.snapshot().await).unwrap();
    assert_eq!(item.kind, AttentionKind::Disconnected);
    assert_eq!(item.fix, Some(crate::buddy::Fix::CheckConnections));
    assert_eq!(
        item.reason,
        "Claude isn't connected, so a/b can't be reviewed: Claude is not signed in. Run claude once to sign in, then check again in Settings."
    );
    // Checking again while it's still off says so.
    assert_eq!(e.fix(key).await.unwrap_err(), item.reason);

    // Signed in since: Check again closes it.
    e.use_claude(crate::claude::tests::fake("ok"));
    e.fix(key).await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(open(&s), None);
    let last = s.buddy_feed.notes.last().unwrap();
    assert_eq!(
        (last.kind, last.text.as_str()),
        (NoteKind::Fixed, "It's connected again. All good!")
    );
}

#[tokio::test]
async fn adding_a_repository_without_a_folder_downloads_orbits_own_copy() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    let added = Repository::new(" acme/api ", "  ");
    let settings = crate::github::GH
        .scope(gh.clone(), e.save_repository(added.clone(), None))
        .await
        .unwrap();
    let folder = dir.path().join("state/clones/acme/api");
    assert!(folder.join("package-lock.json").is_file());
    assert!(!dir.path().join("state/downloads/acme/api").exists());
    let repo = &settings.repositories[0];
    assert_eq!(repo.name, "acme/api");
    assert_eq!(
        Path::new(&repo.path),
        std::fs::canonicalize(&folder).unwrap()
    );
    assert!(repo.keep_updated);
    // Bit follows up as for any new repository.
    let s = e.snapshot().await;
    assert_eq!(s.buddy_feed.notes.last().unwrap().kind, NoteKind::Connected);

    // Adding it again doesn't download anything.
    std::fs::remove_file(dir.path().join("log")).unwrap();
    let error = crate::github::GH
        .scope(gh.clone(), e.save_repository(added, None))
        .await
        .unwrap_err();
    assert_eq!(error, "Each repository can only be configured once.");
    assert_eq!(log(dir.path()), "");

    // A download that fails says why and connects nothing.
    std::fs::write(dir.path().join("clone-fails"), "").unwrap();
    let error = crate::github::GH
        .scope(gh, e.save_repository(Repository::new("acme/web", ""), None))
        .await
        .unwrap_err();
    assert!(
        error.starts_with("Orbit couldn't download acme/web:"),
        "{error}"
    );
    assert_eq!(e.snapshot().await.settings.repositories.len(), 1);

    // A connected repository still needs its folder.
    let mut emptied = e.snapshot().await.settings.repositories[0].clone();
    emptied.path.clear();
    assert!(e
        .save_repository(emptied, Some("acme/api".into()))
        .await
        .is_err());
}

#[tokio::test]
async fn a_download_from_repositories_connects_even_when_the_caller_goes_away() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path());
    let e = engine(dir.path());
    std::fs::write(dir.path().join("gate"), "").unwrap();
    let saving = e.clone();
    let caller = tokio::spawn(crate::github::GH.scope(gh, async move {
        saving
            .save_repository(Repository::new("acme/api", ""), None)
            .await
    }));
    let wait_for = |file: &str| {
        let file = dir.path().join(file);
        async move {
            for _ in 0..200 {
                if file.exists() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            panic!("{} never appeared", file.display());
        }
    };
    // The browser goes away while Orbit downloads.
    wait_for("cloning").await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    std::fs::write(dir.path().join("release"), "").unwrap();
    let mut connected = None;
    for _ in 0..200 {
        let s = e.snapshot().await;
        if let Some(note) = s
            .buddy_feed
            .notes
            .iter()
            .find(|n| n.kind == NoteKind::Connected)
        {
            connected = Some((s.settings.repositories.clone(), note.repo.clone()));
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let (repositories, noted) = connected.expect("the repository was never connected");
    assert_eq!(repositories.len(), 1);
    assert_eq!(repositories[0].name, "acme/api");
    assert!(repositories[0].keep_updated);
    assert_eq!(noted.as_deref(), Some("acme/api"));
}
