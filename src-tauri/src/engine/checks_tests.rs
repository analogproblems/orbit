use super::{test_support::*, worktrees::git, Engine};
use crate::{
    devcontainer::{
        self,
        tests::{calls, fake, wait_for},
    },
    types::*,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Setup {
    _d: tempfile::TempDir,
    e: Arc<Engine>,
    id: String,
    clone: PathBuf,
    worktree: PathBuf,
    base: String,
    docker: PathBuf,
}

async fn commit_all(repo: &Path, message: &str) -> String {
    git(repo, &["add", "."]).await.unwrap();
    git(
        repo,
        &[
            "-c",
            "user.name=Orbit",
            "-c",
            "user.email=orbit@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            message,
        ],
    )
    .await
    .unwrap();
    git(repo, &["rev-parse", "HEAD"])
        .await
        .unwrap()
        .trim()
        .to_string()
}

/// A review of a trusted PR whose base commit has a dev container recipe
/// and whose head changes it, prepared in a worktree, with fake Docker.
async fn review_with_recipe(recipe: bool) -> Setup {
    let (d, clone) = git_repo().await;
    if recipe {
        std::fs::create_dir_all(clone.join(".devcontainer")).unwrap();
        std::fs::write(
            clone.join(".devcontainer/devcontainer.json"),
            r#"{"image":"base-recipe"}"#,
        )
        .unwrap();
    }
    std::fs::write(clone.join("README.md"), "base\n").unwrap();
    let base = commit_all(&clone, "base").await;
    if recipe {
        std::fs::write(
            clone.join(".devcontainer/devcontainer.json"),
            r#"{"image":"pull-request-recipe"}"#,
        )
        .unwrap();
    }
    std::fs::write(clone.join("README.md"), "head\n").unwrap();
    let head = commit_all(&clone, "head").await;
    let worktree = d.path().join("worktree");
    git(
        &clone,
        &[
            "worktree",
            "add",
            "--detach",
            &worktree.to_string_lossy(),
            &head,
        ],
    )
    .await
    .unwrap();
    let docker = d.path().join("docker");
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    e.use_dev_tools(fake(&docker));
    let pr = trusted_pr();
    let id = {
        let mut s = e.state.lock().await;
        s.settings.repositories.push(Repository {
            name: pr.repo.clone(),
            path: clone.to_string_lossy().into(),
            automatic: false,
            setup_command: "cat README.md > setup.txt".into(),
            network_access: false,
            round_limit: 4,
            approval_minutes: 0,
            keep_updated: false,
            provider: Provider::Claude,
            dev_container: DevContainer {
                enabled: true,
                check_command: "cat setup.txt".into(),
                reviewer_tool: false,
            },
        });
        let mut review = Review::new(&pr, &s.settings, false);
        review.status = "preparing".into();
        review.base_sha = base.clone();
        review.head_sha = head;
        review.worktree = Some(worktree.to_string_lossy().into());
        let id = review.id.clone();
        s.prs.push(pr);
        s.reviews.push(review);
        id
    };
    Setup {
        _d: d,
        e,
        id,
        clone,
        worktree,
        base,
        docker,
    }
}
async fn run(t: &Setup) -> Result<(), String> {
    run_keeping(t, false).await
}
async fn run_keeping(t: &Setup, keep: bool) -> Result<(), String> {
    let repo = t.e.snapshot().await.settings.repositories[0].clone();
    t.e.run_container_checks(&t.id, &repo, &t.clone, &t.worktree, &t.base, keep)
        .await
}

#[tokio::test]
async fn builds_from_the_base_recipe_and_keeps_the_results() {
    let t = review_with_recipe(true).await;
    run(&t).await.unwrap();
    let r = t.e.snapshot().await.reviews[0].clone();
    let checks = r.checks.unwrap();
    assert!(checks.setup.as_ref().unwrap().passed());
    let check = checks.check.unwrap();
    assert!(check.passed(), "{check:?}");
    // The pull request's files, set up in the container.
    assert_eq!(check.output, "head");
    assert!(checks.image.starts_with("orbit-devcontainer:"));
    // The image was built from the base commit's recipe, not the PR's.
    let images: Vec<_> = std::fs::read_dir(t.docker.join("images"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    assert_eq!(images, [r#"{"image":"base-recipe"}"#]);
    // Its checkout is gone, from disk and from the clone.
    assert!(std::fs::read_dir(t.e.root.join("recipes"))
        .unwrap()
        .next()
        .is_none());
    assert_eq!(worktree_count(&t.clone).await, 2);
    assert!(r
        .messages
        .iter()
        .any(|m| m.text.starts_with("Building the dev container")));
    let results = r
        .messages
        .iter()
        .find(|m| m.text.starts_with("Dev container results:"))
        .unwrap();
    assert!(
        results.text.contains("Checks (`cat setup.txt`): passed in"),
        "{}",
        results.text
    );
    // The next review reuses the image.
    let builds = |docker: &Path| {
        calls(docker)
            .iter()
            .filter(|c| c[0] == "devcontainer" && c[1] == "build")
            .count()
    };
    assert_eq!(builds(&t.docker), 1);
    run(&t).await.unwrap();
    assert_eq!(builds(&t.docker), 1);
}

#[tokio::test]
async fn a_base_without_a_recipe_fails_the_review() {
    let t = review_with_recipe(false).await;
    let error = run(&t).await.unwrap_err();
    assert!(
        error.contains("no .devcontainer/devcontainer.json"),
        "{error}"
    );
    assert!(calls(&t.docker).is_empty());
}

#[tokio::test]
async fn a_failed_build_fails_the_review_and_removes_its_checkout() {
    let t = review_with_recipe(true).await;
    std::fs::create_dir_all(&t.docker).unwrap();
    std::fs::write(t.docker.join("build-fails"), "").unwrap();
    let error = run(&t).await.unwrap_err();
    assert!(error.contains("could not be built"), "{error}");
    assert!(error.contains("the recipe's build failed"), "{error}");
    assert_eq!(worktree_count(&t.clone).await, 2);
    assert!(t.e.snapshot().await.reviews[0].checks.is_none());
}

#[tokio::test]
async fn a_failing_check_is_a_result_not_an_error() {
    let t = review_with_recipe(true).await;
    t.e.update(|s| s.settings.repositories[0].dev_container.check_command = "exit 4".into())
        .await;
    run(&t).await.unwrap();
    let check = t.e.snapshot().await.reviews[0]
        .checks
        .clone()
        .unwrap()
        .check
        .unwrap();
    assert_eq!(check.exit_code, Some(4));
}

#[tokio::test]
async fn a_stopped_review_runs_nothing() {
    let t = review_with_recipe(true).await;
    t.e.update(|s| s.reviews[0].status = "cancelled".into())
        .await;
    assert!(run(&t).await.is_err());
    assert!(calls(&t.docker).is_empty());
}

#[tokio::test]
async fn stopping_a_review_stops_its_running_setup_and_removes_its_container() {
    let t = review_with_recipe(true).await;
    let started = t.worktree.with_file_name("started");
    let finished = t.worktree.with_file_name("finished");
    let setup = format!(
        "touch '{}' && sleep 2 && touch '{}'",
        started.display(),
        finished.display()
    );
    t.e.update(|s| s.settings.repositories[0].setup_command = setup)
        .await;
    let repo = t.e.snapshot().await.settings.repositories[0].clone();
    // Prepared like a queued review, so Stop aborts it.
    let task = {
        let (e, id, clone, worktree, base) = (
            t.e.clone(),
            t.id.clone(),
            t.clone.clone(),
            t.worktree.clone(),
            t.base.clone(),
        );
        tokio::spawn(async move {
            e.run_container_checks(&id, &repo, &clone, &worktree, &base, false)
                .await
        })
    };
    t.e.preparing
        .lock()
        .unwrap()
        .insert(t.id.clone(), task.abort_handle());
    wait_for("the setup command", || started.exists()).await;
    t.e.cancel(&t.id).await.unwrap();
    assert!(task.await.unwrap_err().is_cancelled());
    let container = t
        .docker
        .join("containers")
        .join(devcontainer::container(&t.id));
    wait_for("the container's removal", || !container.exists()).await;
    // The setup command stopped with its container.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!finished.exists());
}

#[tokio::test]
async fn reports_docker_and_removes_leftovers_at_launch() {
    let t = review_with_recipe(true).await;
    t.e.check_dev_containers().await.unwrap();
    let status = t.e.snapshot().await.dev_containers;
    assert!(status.ready, "{status:?}");
    assert_eq!(
        status.version.as_deref(),
        Some("Docker 29.0.0 · Dev Container CLI 0.80.1")
    );
    // A container and a recipe checkout an earlier run left behind.
    std::fs::create_dir_all(t.docker.join("containers/orbit-checks-old/fs")).unwrap();
    let recipe = t.e.root.join("recipes").join("old");
    git(
        &t.clone,
        &[
            "worktree",
            "add",
            "--detach",
            &recipe.to_string_lossy(),
            &t.base,
        ],
    )
    .await
    .unwrap();
    t.e.remove_leftover_containers().await;
    assert!(!t.docker.join("containers/orbit-checks-old").exists());
    assert!(!recipe.exists());
    assert_eq!(worktree_count(&t.clone).await, 2);
    std::fs::write(t.docker.join("no-daemon"), "").unwrap();
    assert!(t.e.check_dev_containers().await.is_err());
    let status = t.e.snapshot().await.dev_containers;
    assert!(!status.ready);
    assert!(status.error.unwrap().contains("Docker daemon"));
}

#[tokio::test]
async fn keeps_the_container_for_the_reviewer_until_the_review_ends() {
    let t = review_with_recipe(true).await;
    run_keeping(&t, true).await.unwrap();
    let r = t.e.snapshot().await.reviews[0].clone();
    // The checks still ran first, and their results are kept.
    assert!(r.checks.as_ref().unwrap().check.as_ref().unwrap().passed());
    assert!(r.messages.iter().any(|m| m
        .text
        .contains("The reviewer can run its own commands there")));
    let container = t
        .docker
        .join("containers")
        .join(devcontainer::container(&t.id));
    assert!(container.exists());
    let server = t.e.box_server(&t.id).unwrap();
    assert_eq!(server.spec.container, devcontainer::container(&t.id));
    let repo = t.e.snapshot().await.prs[0].repo.clone();
    assert_eq!(
        server.spec.folder,
        format!("/workspaces/{}", repo.rsplit('/').next().unwrap())
    );
    // The setup's files are there for the reviewer.
    assert!(container
        .join("fs")
        .join(server.spec.folder.trim_start_matches('/'))
        .join("setup.txt")
        .exists());
    // A running review keeps it; an ended one loses it.
    t.e.close_boxes().await;
    assert!(container.exists());
    t.e.update(|s| s.reviews[0].status = "ready".into()).await;
    t.e.close_boxes().await;
    assert!(!container.exists());
    assert!(t.e.box_server(&t.id).is_none());
}

#[tokio::test]
async fn the_reviewers_container_needs_no_commands_and_goes_when_stopped() {
    let t = review_with_recipe(true).await;
    t.e.update(|s| {
        let repo = &mut s.settings.repositories[0];
        repo.setup_command.clear();
        repo.dev_container.check_command.clear();
        repo.dev_container.reviewer_tool = true;
    })
    .await;
    run_keeping(&t, true).await.unwrap();
    let r = t.e.snapshot().await.reviews[0].clone();
    assert!(r.checks.is_none());
    assert!(!r.messages.iter().any(|m| m.text.starts_with("Running ")));
    let container = t
        .docker
        .join("containers")
        .join(devcontainer::container(&t.id));
    assert!(container.exists());
    t.e.cancel(&t.id).await.unwrap();
    assert!(!container.exists());
    assert!(t.e.box_server(&t.id).is_none());
}

#[tokio::test]
async fn a_kept_container_stops_a_setup_that_runs_out_of_time() {
    let t = review_with_recipe(true).await;
    t.e.update(|s| {
        let repo = &mut s.settings.repositories[0];
        // Longer than the tests' three-second limit.
        repo.setup_command = "(sleep 4; touch child) & sleep 4; touch late".into();
        repo.dev_container.reviewer_tool = true;
    })
    .await;
    run_keeping(&t, true).await.unwrap();
    let setup = t.e.snapshot().await.reviews[0]
        .checks
        .clone()
        .unwrap()
        .setup
        .unwrap();
    assert!(setup.timed_out);
    let server = t.e.box_server(&t.id).unwrap();
    let copy = t
        .docker
        .join("containers")
        .join(devcontainer::container(&t.id))
        .join("fs")
        .join(server.spec.folder.trim_start_matches('/'));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // The container stays for the reviewer, without the stopped setup.
    assert!(copy.join("README.md").exists());
    assert!(!copy.join("late").exists());
    assert!(!copy.join("child").exists());
    t.e.close_box(&t.id).await;
}

#[tokio::test]
async fn a_setup_that_cant_be_stopped_costs_the_reviewer_its_container() {
    let t = review_with_recipe(true).await;
    t.e.update(|s| {
        let repo = &mut s.settings.repositories[0];
        repo.setup_command = "sleep 5".into();
        repo.dev_container.reviewer_tool = true;
    })
    .await;
    std::fs::create_dir_all(&t.docker).unwrap();
    std::fs::write(t.docker.join("stop-fails"), "").unwrap();
    run_keeping(&t, true).await.unwrap();
    let r = t.e.snapshot().await.reviews[0].clone();
    let setup = r.checks.unwrap().setup.unwrap();
    assert!(setup.timed_out);
    assert!(setup.output.contains("so it removed the container"));
    assert!(r
        .messages
        .iter()
        .any(|m| m.text.contains("The reviewer can't run commands there")));
    assert!(t.e.box_server(&t.id).is_none());
    assert!(!t
        .docker
        .join("containers")
        .join(devcontainer::container(&t.id))
        .exists());
}

#[tokio::test]
async fn launch_clean_up_waits_for_a_review_setting_up_its_container() {
    let t = review_with_recipe(true).await;
    t.e.check_dev_containers().await.unwrap();
    std::fs::create_dir_all(t.docker.join("containers/orbit-checks-old/fs")).unwrap();
    // A review is setting up its dev container as Orbit launches.
    let setting_up = t.e.containers.read().await;
    let cleaning = tokio::spawn({
        let e = t.e.clone();
        async move { e.remove_leftover_containers().await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(t.docker.join("containers/orbit-checks-old").exists());
    // Its container starts while the clean-up waits.
    let ours = t
        .docker
        .join("containers")
        .join(devcontainer::container(&t.id));
    std::fs::create_dir_all(ours.join("fs")).unwrap();
    t.e.update(|s| s.reviews[0].status = "preparing".into())
        .await;
    drop(setting_up);
    cleaning.await.unwrap();
    assert!(!t.docker.join("containers/orbit-checks-old").exists());
    assert!(ours.exists(), "the review's own container stays");
}
