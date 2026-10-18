//! A repository's setup and checks in its dev container, run before the
//! review so the reviewer gets their results (see `crate::devcontainer`).
use super::{
    worktrees::{git, remove_worktree},
    Engine,
};
use crate::{boxtool, devcontainer, types::*};
use std::path::Path;

/// A review's dev container, kept while the reviewer can run commands in it.
pub(super) struct ReviewBox {
    opened: devcontainer::Opened,
    pub(super) server: boxtool::Server,
}

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
        // Reviews may already be starting, so their own containers and
        // recipe checkouts stay.
        let snapshot = self.snapshot().await;
        let mut keep: Vec<String> = snapshot
            .reviews
            .iter()
            .filter(|r| active(&r.status))
            .map(|r| r.id.clone())
            .collect();
        keep.extend(self.boxes.lock().unwrap().keys().cloned());
        if let Ok(entries) = std::fs::read_dir(self.root.join("recipes")) {
            let _git = self.git_lock.lock().await;
            for entry in entries.flatten() {
                if !keep.iter().any(|id| entry.file_name() == id.as_str()) {
                    let _ = remove_worktree(&entry.path()).await;
                }
            }
        }
        if snapshot.dev_containers.ready {
            let tools = self.dev_tools.lock().unwrap().clone();
            devcontainer::remove_leftovers(&tools, &keep).await;
        }
    }
    #[cfg(test)]
    pub(super) fn use_dev_tools(&self, tools: devcontainer::Tools) {
        *self.dev_tools.lock().unwrap() = tools;
    }
    /// Runs the repository's setup and check commands on the review's
    /// worktree in its dev container, built from the base commit's recipe,
    /// and keeps the results for the reviewer. With `keep`, the container
    /// stays for the reviewer's own commands until the review ends. A
    /// failing command is a result; a container that can't be built or
    /// started fails the review.
    pub(super) async fn run_container_checks(
        &self,
        id: &str,
        repo: &Repository,
        clone: &Path,
        worktree: &Path,
        base_sha: &str,
        keep: bool,
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
        let plan = devcontainer::Run {
            id,
            image: &image,
            repo: &repo.name,
            worktree,
            network: repo.network_access,
            setup: &repo.setup_command,
            check: &repo.dev_container.check_command,
        };
        let commands = !repo.setup_command.trim().is_empty()
            || !repo.dev_container.check_command.trim().is_empty();
        let checks = if keep {
            let opened = devcontainer::open(&tools, &plan).await?;
            let server = boxtool::Server::new(boxtool::Spec {
                docker: tools.docker.clone(),
                container: opened.name().into(),
                user: opened.user.clone(),
                folder: opened.folder.clone(),
                network: opened.network(),
            });
            let server = match server {
                Ok(server) => server,
                Err(error) => {
                    opened.remove().await;
                    return Err(error);
                }
            };
            self.say(
                id,
                format!(
                    "Started the dev container {image}, with network access {}. The reviewer can run its own commands there during the review.",
                    on_off(repo.network_access)
                ),
            )
            .await;
            let checks = if commands {
                self.say(id, running(repo, &image)).await;
                match opened.checks(&tools, plan.setup, plan.check).await {
                    Ok(checks) => Some(checks),
                    Err(error) => {
                        opened.remove().await;
                        return Err(error);
                    }
                }
            } else {
                None
            };
            if opened.removed() {
                self.say(
                    id,
                    "A command ran out of time and couldn't be stopped inside the dev container, so Orbit removed it. The reviewer can't run commands there in this review.".into(),
                )
                .await;
            } else {
                // The container goes when the review ends, however it ends.
                self.boxes
                    .lock()
                    .unwrap()
                    .insert(id.to_string(), ReviewBox { opened, server });
            }
            checks
        } else {
            self.say(id, running(repo, &image)).await;
            Some(devcontainer::run_checks(&tools, plan).await?)
        };
        self.live(id).await?;
        let Some(checks) = checks else {
            return Ok(());
        };
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
    /// The reviewer's tool for a review whose container is kept.
    pub(super) fn box_server(&self, id: &str) -> Option<boxtool::Server> {
        self.boxes.lock().unwrap().get(id).map(|b| b.server.clone())
    }
    /// Keeps a container for review `id`'s reviewer, for tests.
    #[cfg(test)]
    pub(super) fn keep_box(&self, id: &str, opened: devcontainer::Opened) -> boxtool::Server {
        let server = boxtool::Server::new(boxtool::Spec {
            docker: self.dev_tools.lock().unwrap().docker.clone(),
            container: opened.name().into(),
            user: opened.user.clone(),
            folder: opened.folder.clone(),
            network: opened.network(),
        })
        .unwrap();
        self.boxes.lock().unwrap().insert(
            id.into(),
            ReviewBox {
                opened,
                server: server.clone(),
            },
        );
        server
    }
    /// Removes the containers of reviews that have ended.
    pub(super) async fn close_boxes(&self) {
        if self.boxes.lock().unwrap().is_empty() {
            return;
        }
        let running: Vec<String> = {
            let s = self.state.lock().await;
            s.reviews
                .iter()
                .filter(|r| active(&r.status))
                .map(|r| r.id.clone())
                .collect()
        };
        let ended: Vec<ReviewBox> = {
            let mut boxes = self.boxes.lock().unwrap();
            let ids: Vec<String> = boxes
                .keys()
                .filter(|id| !running.contains(id))
                .cloned()
                .collect();
            ids.iter().filter_map(|id| boxes.remove(id)).collect()
        };
        for ended in ended {
            ended.opened.remove().await;
        }
    }
    /// Removes a review's container now, such as when it is stopped.
    pub(super) async fn close_box(&self, id: &str) {
        let ended = self.boxes.lock().unwrap().remove(id);
        if let Some(ended) = ended {
            ended.opened.remove().await;
        }
    }
    pub(super) async fn say(&self, id: &str, text: String) {
        self.update(|s| {
            if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                r.messages.push(Message::new("system", text));
            }
        })
        .await;
    }
}

/// What runs in the container, as the review's chat says it.
fn running(repo: &Repository, image: &str) -> String {
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
        on_off(repo.network_access)
    )
}
fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}
