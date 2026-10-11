//! A repository's setup and checks in its dev container, run before the
//! review so the reviewer gets their results (see `crate::devcontainer`).
use super::{
    worktrees::{git, remove_worktree},
    Engine,
};
use crate::{devcontainer, types::*};
use std::path::Path;

impl Engine {
    /// Checks Docker and the Dev Container CLI, and records whether
    /// repositories can run their checks in dev containers.
    pub async fn check_dev_containers(&self) -> Result<(), String> {
        let tools = self.dev_tools.lock().unwrap().clone();
        let result = devcontainer::check(&tools).await;
        self.update(|s| {
            s.dev_containers = match &result {
                Ok(version) => CliStatus {
                    ready: true,
                    error: None,
                    version: Some(version.clone()),
                },
                Err(error) => CliStatus {
                    ready: false,
                    error: Some(error.clone()),
                    version: None,
                },
            }
        })
        .await;
        result.map(|_| ())
    }
    /// Removes check containers an earlier run of Orbit left behind, and
    /// recipe checkouts it didn't get to remove.
    pub(super) async fn remove_leftover_containers(&self) {
        if let Ok(entries) = std::fs::read_dir(self.root.join("recipes")) {
            let _git = self.git_lock.lock().await;
            for entry in entries.flatten() {
                let _ = remove_worktree(&entry.path()).await;
            }
        }
        if self.snapshot().await.dev_containers.ready {
            let tools = self.dev_tools.lock().unwrap().clone();
            devcontainer::remove_leftovers(&tools).await;
        }
    }
    #[cfg(test)]
    pub(super) fn use_dev_tools(&self, tools: devcontainer::Tools) {
        *self.dev_tools.lock().unwrap() = tools;
    }
    /// Runs the repository's setup and check commands on the review's
    /// worktree in its dev container, built from the base commit's recipe,
    /// and keeps the results for the reviewer. A failing command is a
    /// result; a container that can't be built or started fails the review.
    pub(super) async fn run_container_checks(
        &self,
        id: &str,
        repo: &Repository,
        clone: &Path,
        worktree: &Path,
        base_sha: &str,
    ) -> Result<(), String> {
        let tools = self.dev_tools.lock().unwrap().clone();
        let recipe = git(
            worktree,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{base_sha}:{}", devcontainer::RECIPE),
            ],
        )
        .await
        .ok()
        .map(|tree| tree.trim().to_string())
        .filter(|tree| !tree.is_empty());
        let config = format!("{base_sha}:{}/devcontainer.json", devcontainer::RECIPE);
        let has_config = git(worktree, &["cat-file", "-e", &config]).await.is_ok();
        let Some(recipe) = recipe.filter(|_| has_config) else {
            return Err(format!(
                "The base commit has no {}/devcontainer.json, so the checks can't run in a dev container. Add one to the base branch, or turn off dev container checks for {}.",
                devcontainer::RECIPE,
                repo.name
            ));
        };
        let image = devcontainer::image(&repo.name, &recipe);
        self.live(id).await?;
        {
            let _building = self.building.lock().await;
            if !devcontainer::exists(&tools, &image).await {
                self.say(
                    id,
                    format!(
                        "Building the dev container from the base commit's recipe as {image}. The first build can take a while."
                    ),
                )
                .await;
                self.build_image(id, &tools, clone, base_sha, &image)
                    .await?;
            }
        }
        self.live(id).await?;
        self.say(
            id,
            format!(
                "Running {} in the dev container {image}, with network access {}.",
                match (
                    repo.setup_command.trim().is_empty(),
                    repo.dev_container.check_command.trim().is_empty()
                ) {
                    (false, false) => "the setup and check commands",
                    (false, true) => "the setup command",
                    _ => "the check command",
                },
                if repo.network_access { "on" } else { "off" }
            ),
        )
        .await;
        let checks = devcontainer::run_checks(
            &tools,
            devcontainer::Run {
                id,
                image: &image,
                repo: &repo.name,
                worktree,
                network: repo.network_access,
                setup: &repo.setup_command,
                check: &repo.dev_container.check_command,
            },
        )
        .await?;
        self.live(id).await?;
        let summary = devcontainer::summary(&checks);
        self.update(|s| {
            if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                r.messages.push(Message::new(
                    "system",
                    format!("Dev container results:\n{summary}"),
                ));
                r.checks = Some(checks);
            }
        })
        .await;
        Ok(())
    }
    /// Builds the image from a checkout of the base commit, so the pull
    /// request can't change the recipe its code runs with.
    async fn build_image(
        &self,
        id: &str,
        tools: &devcontainer::Tools,
        clone: &Path,
        base_sha: &str,
        image: &str,
    ) -> Result<(), String> {
        let folder = self.root.join("recipes").join(id);
        std::fs::create_dir_all(folder.parent().unwrap()).map_err(|e| e.to_string())?;
        {
            let _git = self.git_lock.lock().await;
            git(
                clone,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    folder.to_str().ok_or("Invalid recipe path")?,
                    base_sha,
                ],
            )
            .await?;
        }
        let built = devcontainer::build(tools, &folder, image).await;
        {
            let _git = self.git_lock.lock().await;
            let _ = remove_worktree(&folder).await;
        }
        built
    }
    async fn say(&self, id: &str, text: String) {
        self.update(|s| {
            if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                r.messages.push(Message::new("system", text));
            }
        })
        .await;
    }
}
