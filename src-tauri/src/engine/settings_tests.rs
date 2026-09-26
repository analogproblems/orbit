use super::{
    settings::checkout_io, settings::matches_origin, settings::validate_checkout, test_support::*,
    worktrees::git,
};
use crate::types::*;
use std::time::Duration;

#[tokio::test]
async fn can_disable_automation_while_codex_is_disconnected() {
    let (_d, e, _) = running_engine().await;
    e.update(|s| {
        s.settings.automatic = true;
        s.settings.model = "saved-model".into();
        s.models.clear();
    })
    .await;
    let mut settings = e.snapshot().await.settings;
    settings.automatic = false;
    e.save_settings(settings).await.unwrap();
    assert!(!e.snapshot().await.settings.automatic);
}
#[test]
fn rejects_other_checkout_origins() {
    assert!(matches_origin("git@github.com:Acme/repo.git", "acme/repo"));
    assert!(matches_origin(
        "https://github.com/acme/repo.git",
        "acme/repo"
    ));
    assert!(!matches_origin(
        "https://github.com/other/repo",
        "acme/repo"
    ));
    assert!(!matches_origin(
        "https://github.com.evil/acme/repo",
        "acme/repo"
    ));
}
#[tokio::test]
async fn validates_checkout_origin_and_canonical_root_without_setup() {
    let (_d, repo) = git_repo().await;
    git(
        &repo,
        &["remote", "add", "origin", "git@github.com:a/b.git"],
    )
    .await
    .unwrap();
    let child = repo.join("nested");
    std::fs::create_dir(&child).unwrap();
    let canonical = validate_checkout("A/B", child.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(canonical, repo.canonicalize().unwrap().to_string_lossy());
    assert!(validate_checkout("a/c", repo.to_str().unwrap())
        .await
        .is_err());
    assert!(validate_checkout("a/b", "relative").await.is_err());
    let plain = tempfile::tempdir().unwrap();
    assert!(validate_checkout("a/b", plain.path().to_str().unwrap())
        .await
        .is_err());
    git(
        &repo,
        &["remote", "set-url", "origin", "https://example.com/a/b.git"],
    )
    .await
    .unwrap();
    assert!(validate_checkout("a/b", repo.to_str().unwrap())
        .await
        .is_err());
}
#[tokio::test]
async fn partial_settings_are_atomic_and_unavailable_checkouts_do_not_block_edits() {
    let (d, e, _) = running_engine().await;
    e.update(|s| {
        s.settings.automatic = true;
        s.settings.repositories.push(Repository {
            name: "a/b".into(),
            path: "/missing/checkout".into(),
            automatic: true,
            setup_command: String::new(),
            network_access: false,
        });
    })
    .await;
    let (a, b) = tokio::join!(
        e.update_settings(SettingsPatch {
            automatic: Some(false),
            ..Default::default()
        }),
        e.update_settings(SettingsPatch {
            poll_seconds: Some(60),
            ..Default::default()
        })
    );
    a.unwrap();
    b.unwrap();
    let settings = e.snapshot().await.settings;
    assert!(!settings.automatic);
    assert_eq!(settings.poll_seconds, 60);
    e.set_repository_automatic("a/b", false).await.unwrap();
    e.disconnect_repository("a/b").await.unwrap();
    // A failed rename leaves both the active and saved settings unchanged.
    std::fs::remove_file(d.path().join("state.json")).unwrap();
    std::fs::create_dir(d.path().join("state.json")).unwrap();
    let error = e
        .update_settings(SettingsPatch {
            automatic: Some(true),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(error.contains("not saved"));
    assert!(!e.snapshot().await.settings.automatic);
    assert!(e
        .update_settings(SettingsPatch {
            concurrency: Some(99),
            ..Default::default()
        })
        .await
        .is_err());
}
#[tokio::test]
async fn repository_save_validates_and_keeps_other_settings() {
    let (_d, e, _) = running_engine().await;
    let (_repo_dir, repo) = git_repo().await;
    git(
        &repo,
        &["remote", "add", "origin", "https://github.com/a/b.git"],
    )
    .await
    .unwrap();
    let child = repo.join("subdir");
    std::fs::create_dir(&child).unwrap();
    let config = Repository {
        name: "a/b".into(),
        path: child.to_string_lossy().into(),
        automatic: false,
        setup_command: "exit 1 # must not execute".into(),
        network_access: false,
    };
    e.save_repository(config.clone(), None).await.unwrap();
    assert_eq!(
        e.snapshot().await.settings.repositories[0].path,
        repo.canonicalize().unwrap().to_string_lossy()
    );
    let mut invalid = config;
    invalid.name = "wrong/repo".into();
    assert!(e
        .save_repository(invalid, Some("a/b".into()))
        .await
        .is_err());
    assert_eq!(e.snapshot().await.settings.repositories[0].name, "a/b");
}
#[tokio::test]
async fn slow_checkout_filesystem_work_does_not_block_the_runtime_or_wait_forever() {
    let (release, blocked) = std::sync::mpsc::channel();
    let result = checkout_io(
        move || {
            blocked.recv().unwrap();
            Ok(())
        },
        Duration::from_millis(20),
    )
    .await;
    release.send(()).unwrap();
    assert!(result.unwrap_err().contains("filesystem check timed out"));
}
