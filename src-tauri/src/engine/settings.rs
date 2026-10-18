use super::{setup::carry_on, Engine};
use crate::{github, types::*};
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

impl Engine {
    pub(super) fn commit_settings(
        &self,
        s: &mut Snapshot,
        settings: Settings,
    ) -> Result<(), String> {
        settings.validate()?;
        let changed = settings.model != s.settings.model || settings.effort != s.settings.effort;
        // Unrelated edits, including disabling automation, remain available.
        if changed {
            ModelSelection::workspace(&settings).validate_execution(s)?;
        }
        if settings.claude_model != s.settings.claude_model
            || settings.claude_effort != s.settings.claude_effort
        {
            settings.selection(Provider::Claude).validate_claude()?;
        }
        let effort = &settings.antigravity_effort;
        if !effort.is_empty() && !crate::antigravity::EFFORTS.contains(&effort.as_str()) {
            return Err(format!(
                "Antigravity doesn't support effort '{effort}'. Choose low, medium or high."
            ));
        }
        // Antigravity's models are known once it was checked; until then a
        // model is checked when a review starts.
        if (settings.antigravity_model != s.settings.antigravity_model
            || settings.antigravity_effort != s.settings.antigravity_effort)
            && !settings.antigravity_model.is_empty()
            && !s.antigravity_models.is_empty()
        {
            settings
                .selection(Provider::Antigravity)
                .validate_antigravity(&s.antigravity_models)?;
        }
        let mut next = s.clone();
        // Turning autopilot back on acknowledges a failed save that paused it.
        if settings.automatic {
            next.state_write_error = None;
        }
        next.settings = settings;
        if ModelSelection::workspace(&next.settings)
            .validate_execution(&next)
            .is_ok()
        {
            next.model_configuration_error = None;
        }
        if changed {
            next.model_selection_notice = None;
        }
        self.persist(&next)
            .map_err(|e| format!("Settings were not saved: {e}"))?;
        *s = next;
        self.dirty.store(false, Ordering::SeqCst);
        self.changed.store(false, Ordering::SeqCst);
        (self.emit)(s);
        Ok(())
    }
    #[cfg(test)]
    pub async fn save_settings(&self, settings: Settings) -> Result<(), String> {
        self.commit_settings(&mut *self.state.lock().await, settings)
    }
    pub async fn update_settings(&self, patch: SettingsPatch) -> Result<Settings, String> {
        let ollama = |s: &Settings| (s.buddy.answered_by(), s.buddy.ollama_url.clone());
        let (saved, recheck) = {
            let mut s = self.state.lock().await;
            let before = ollama(&s.settings);
            let mut settings = s.settings.clone();
            patch.apply(&mut settings);
            self.commit_settings(&mut s, settings)?;
            (s.settings.clone(), ollama(&s.settings) != before)
        };
        // A new answerer for Bit, or Ollama at a new address, is checked
        // straight away, so Settings can list its models.
        if recheck {
            self.check_bit_answerer().await;
        }
        Ok(saved)
    }
    /// Connects a repository, or saves a connected one's settings. A new
    /// repository without a folder gets Orbit's own copy, downloaded into
    /// its data folder and kept up to date. The whole save carries on even
    /// if the window or browser that asked goes away, so a long download
    /// still ends connected.
    pub async fn save_repository(
        self: &Arc<Self>,
        repository: Repository,
        original: Option<String>,
    ) -> Result<Settings, String> {
        let e = self.clone();
        carry_on(async move { e.save_repository_now(repository, original).await }).await
    }
    async fn save_repository_now(
        &self,
        mut repository: Repository,
        original: Option<String>,
    ) -> Result<Settings, String> {
        repository.name = repository.name.trim().into();
        repository.path = repository.path.trim().into();
        // Held until the copy is connected, as for Bit's Connect button.
        let mut _cloning = None;
        if original.is_none() && repository.path.is_empty() {
            _cloning = Some(self.cloning.lock().await);
            let connected = self
                .state
                .lock()
                .await
                .settings
                .repositories
                .iter()
                .any(|r| same_repo(&r.name, &repository.name));
            if connected {
                return Err("Each repository can only be configured once.".into());
            }
            let folder = self.own_copy(&repository.name).await?;
            repository.path = folder.to_string_lossy().into();
            repository.keep_updated = true;
        }
        let validate = {
            let s = self.state.lock().await;
            original
                .as_ref()
                .and_then(|name| {
                    s.settings
                        .repositories
                        .iter()
                        .find(|r| same_repo(&r.name, name))
                })
                .is_none_or(|old| {
                    !same_repo(&old.name, &repository.name) || old.path != repository.path
                })
        };
        if validate {
            repository.path = validate_checkout(&repository.name, &repository.path).await?;
        }
        let mut s = self.state.lock().await;
        let mut settings = s.settings.clone();
        let added = original.is_none().then(|| repository.clone());
        if let Some(name) = original {
            let old = settings
                .repositories
                .iter_mut()
                .find(|r| same_repo(&r.name, &name))
                .ok_or("This repository was disconnected. Add it again.")?;
            *old = repository;
        } else {
            settings.repositories.push(repository);
        }
        self.commit_settings(&mut s, settings)?;
        let saved = s.settings.clone();
        drop(s);
        // Bit offers what a new repository still needs.
        if let Some(r) = added {
            self.note_connected(&r.name, &r.path).await;
        }
        Ok(saved)
    }
    pub async fn set_repository_automatic(
        &self,
        name: &str,
        automatic: bool,
    ) -> Result<Settings, String> {
        let mut s = self.state.lock().await;
        let mut settings = s.settings.clone();
        settings
            .repositories
            .iter_mut()
            .find(|r| same_repo(&r.name, name))
            .ok_or("Repository is no longer connected.")?
            .automatic = automatic;
        self.commit_settings(&mut s, settings)?;
        Ok(s.settings.clone())
    }
    pub async fn disconnect_repository(&self, name: &str) -> Result<Settings, String> {
        let mut s = self.state.lock().await;
        let mut settings = s.settings.clone();
        settings.repositories.retain(|r| !same_repo(&r.name, name));
        self.commit_settings(&mut s, settings)?;
        Ok(s.settings.clone())
    }
}
pub(super) async fn checkout_io<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
    timeout: Duration,
) -> Result<T, String> {
    // A timed-out filesystem syscall cannot be cancelled, but it must not hold
    // a Tokio worker or keep the repository dialog waiting indefinitely.
    tokio::time::timeout(timeout, tokio::task::spawn_blocking(operation))
        .await
        .map_err(|_| {
            "Checkout filesystem check timed out. Check the mount and try again.".to_string()
        })?
        .map_err(|e| format!("Checkout filesystem check failed: {e}"))?
}
pub async fn validate_checkout(repo: &str, path: &str) -> Result<String, String> {
    validate_repo(repo)?;
    if !Path::new(path).is_absolute() {
        return Err("Use an absolute checkout path.".into());
    }
    let path = path.to_string();
    let cwd = checkout_io(
        move || {
            let cwd =
                std::fs::canonicalize(path).map_err(|e| format!("Cannot access checkout: {e}"))?;
            if !cwd.is_dir() {
                return Err("Checkout must be a directory.".into());
            }
            Ok(cwd)
        },
        Duration::from_secs(10),
    )
    .await?;
    let read = |args: Vec<String>| {
        let cwd = cwd.clone();
        async move {
            github::run_with_timeout("git", &args, Some(&cwd), None, Duration::from_secs(10)).await
        }
    };
    let root = read(vec!["rev-parse".into(), "--show-toplevel".into()])
        .await
        .map_err(|_| {
            "Choose an existing Git working tree, not a bare repository or ordinary directory."
                .to_string()
        })?;
    let origin = read(vec!["remote".into(), "get-url".into(), "origin".into()]).await?;
    if !matches_origin(origin.trim(), repo) {
        return Err(format!(
            "Checkout origin does not match {repo}. Choose its matching clone."
        ));
    }
    checkout_io(
        move || {
            std::fs::canonicalize(root.trim())
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| e.to_string())
        },
        Duration::from_secs(10),
    )
    .await
}
pub(super) fn matches_origin(origin: &str, repo: &str) -> bool {
    let origin = origin
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(origin.trim_end_matches('/'));
    [
        format!("https://github.com/{repo}"),
        format!("git@github.com:{repo}"),
        format!("ssh://git@github.com/{repo}"),
    ]
    .iter()
    .any(|u| u.eq_ignore_ascii_case(origin))
}
