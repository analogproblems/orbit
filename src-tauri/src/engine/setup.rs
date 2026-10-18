//! Repositories Bit sets up when you tap an answer's button, and what Bit
//! looks up about your repositories for a question you type.
use super::{
    settings::{checkout_io, validate_checkout},
    Engine,
};
use crate::{
    ask::Lookups,
    github, guide,
    setup::{self, RepositoryPatch},
    types::*,
};
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// What a repository's copy at `path` holds, or why Orbit can't tell.
async fn copy_facts(path: &str) -> Result<setup::CloneFacts, String> {
    let path = path.to_string();
    checkout_io(
        move || setup::clone_facts(Path::new(&path)),
        Duration::from_secs(5),
    )
    .await
}
/// How long Bit waits for GitHub to list your repositories.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs `work` in a task of its own, so a download carries on even if the
/// window or browser waiting for it goes away.
pub(super) async fn carry_on<T: Send + 'static>(
    work: impl Future<Output = Result<T, String>> + Send + 'static,
) -> Result<T, String> {
    // Tests reach their stand-in for `gh` from the new task too.
    #[cfg(test)]
    let work = {
        let gh = github::GH.try_with(|p| p.clone()).ok();
        async move {
            match gh {
                Some(gh) => github::GH.scope(gh, work).await,
                None => work.await,
            }
        }
    };
    tokio::spawn(work).await.map_err(|e| e.to_string())?
}

impl Engine {
    /// Sets up `name` as an answer's button asks. A connected repository
    /// gets the change, checked like a change in Repositories. A button
    /// that `connect`s, and only one, may connect a repository that isn't:
    /// Orbit downloads its own copy into its data folder, unless it has one
    /// already, and connects it with the defaults plus the change. Orbit's
    /// copy is kept up to date unless the change says otherwise. A download
    /// can take minutes, so it carries on even if the window or browser
    /// that tapped the button goes away.
    pub(super) async fn set_up_repository(
        self: &Arc<Self>,
        name: &str,
        patch: &RepositoryPatch,
        connect: bool,
    ) -> Result<(), String> {
        let (e, name, patch) = (self.clone(), name.to_string(), patch.clone());
        carry_on(async move { e.set_up(&name, &patch, connect).await }).await
    }
    async fn set_up(
        self: &Arc<Self>,
        name: &str,
        patch: &RepositoryPatch,
        connect: bool,
    ) -> Result<(), String> {
        if self.change_repository(name, patch).await? {
            return Ok(());
        }
        // A button that only changes settings never connects: its
        // repository was disconnected since, and connecting it again takes
        // a button that says so.
        if !connect {
            return Err(format!(
                "{name} isn't connected any more. Ask Bit again if you want it connected."
            ));
        }
        let _cloning = self.cloning.lock().await;
        let folder = self.own_copy(name).await?;
        let not_connected = |e: String| {
            format!(
                "Orbit's copy of {name}, in {}, couldn't be connected: {e}",
                folder.display()
            )
        };
        let path = validate_checkout(name, &folder.to_string_lossy())
            .await
            .map_err(not_connected)?;
        let mut repository = Repository::new(name, &path);
        repository.keep_updated = true;
        patch.apply(&mut repository);
        let added = {
            let mut s = self.state.lock().await;
            let mut settings = s.settings.clone();
            let connected = settings
                .repositories
                .iter_mut()
                .find(|r| same_repo(&r.name, name));
            let added = connected.is_none();
            match connected {
                // Connected meanwhile, such as from another window: it
                // gets the change instead.
                Some(connected) => patch.apply(connected),
                None => settings.repositories.push(repository),
            }
            self.commit_settings(&mut s, settings)
                .map_err(not_connected)?;
            added
        };
        if added {
            self.note_connected(name, &path).await;
        }
        // Its pull requests show up without waiting for the next sync.
        let e = self.clone();
        tokio::spawn(async move {
            let _ = e.poll().await;
        });
        Ok(())
    }
    /// Orbit's own copy of `name`, in its data folder: downloaded with the
    /// GitHub CLI unless it's there already. It is downloaded elsewhere,
    /// then moved into place whole, so a download cut short is never used
    /// as a copy. Callers hold `cloning`.
    pub(super) async fn own_copy(&self, name: &str) -> Result<PathBuf, String> {
        let folder = setup::clone_folder(&self.root, name)?;
        if folder.exists() {
            return Ok(folder);
        }
        let download = setup::download_folder(&self.root, name)?;
        let _ = tokio::fs::remove_dir_all(&download).await;
        for f in [&download, &folder] {
            let parent = f.parent().ok_or("Invalid folder")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        github::clone(name, &download)
            .await
            .map_err(|e| format!("Orbit couldn't download {name}: {e}"))?;
        tokio::fs::rename(&download, &folder)
            .await
            .map_err(|e| format!("Orbit couldn't put its copy of {name} in place: {e}"))?;
        Ok(folder)
    }
    /// Changes a connected repository's settings, returning whether it is
    /// connected.
    async fn change_repository(&self, name: &str, patch: &RepositoryPatch) -> Result<bool, String> {
        let mut s = self.state.lock().await;
        let mut settings = s.settings.clone();
        let Some(repository) = settings
            .repositories
            .iter_mut()
            .find(|r| same_repo(&r.name, name))
        else {
            return Ok(false);
        };
        patch.apply(repository);
        self.commit_settings(&mut s, settings)?;
        Ok(true)
    }
    /// What a question's summary needs beyond Orbit's state: what each
    /// connected repository's copy holds, and, when `list`, your GitHub
    /// repositories that aren't connected. A look that fails or takes too
    /// long says why instead.
    pub(super) async fn look_up(&self, connected: Vec<Repository>, list: bool) -> Lookups {
        let names: Vec<String> = connected.iter().map(|r| r.name.clone()).collect();
        let clones = async {
            let mut clones = BTreeMap::new();
            for r in connected {
                clones.insert(r.name, copy_facts(&r.path).await);
            }
            clones
        };
        let listed = async {
            if !list {
                return Ok(vec![]);
            }
            let unconnected = |repos: Vec<RepositorySummary>| {
                repos
                    .into_iter()
                    .filter(|r| !names.iter().any(|n| same_repo(n, &r.name)))
                    .collect()
            };
            match tokio::time::timeout(LIST_TIMEOUT, github::recent_repositories()).await {
                Ok(listed) => listed.map(unconnected),
                Err(_) => Err("GitHub took too long to list your repositories.".into()),
            }
        };
        let (clones, connectable) = tokio::join!(clones, listed);
        Lookups {
            clones,
            connectable,
            decisions: vec![],
        }
    }
    /// Checks every connection again, as Settings' Reconnect does: the
    /// reviewers, Codex, then GitHub with a sync. Fails with what still
    /// isn't connected.
    pub(super) async fn check_connections(self: &Arc<Self>) -> Result<(), String> {
        self.check_reviewers().await;
        let _ = self.connect().await;
        if self.poll().await.is_ok() {
            self.update_clones_soon();
        }
        self.pick_bit_answerer().await;
        let s = self.state.lock().await;
        let off: Vec<String> = guide::connections(&s)
            .into_iter()
            .filter(|c| !c.ready)
            .map(|c| match c.error {
                Some(e) => format!("{} still isn't connected: {}", c.name, e.trim()),
                None => format!("{} still isn't connected. {}", c.name, c.how),
            })
            .collect();
        if off.is_empty() {
            Ok(())
        } else {
            Err(off.join(" "))
        }
    }
    /// Bit's note that `name`, at `path`, was just connected, with buttons
    /// for its next steps.
    pub(super) async fn note_connected(&self, name: &str, path: &str) {
        if !self.state.lock().await.settings.buddy.enabled {
            return;
        }
        let facts = copy_facts(path).await;
        self.update(|s| crate::buddy::connected(s, name, Some(&facts)))
            .await;
        let _ = self.flush().await;
    }
}
