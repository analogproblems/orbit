//! Checking Orbit's own health by rules (`crate::health`).
use super::Engine;
use crate::health;
use std::{sync::atomic::Ordering, sync::Arc, time::Duration};

/// How often Orbit checks its own health.
pub(super) const EVERY: Duration = Duration::from_secs(60);

impl Engine {
    /// Starts `check_health` if it hasn't run in the last minute.
    pub(super) fn health_soon(self: &Arc<Self>) {
        {
            let mut last = self.last_health.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < EVERY) {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        tokio::spawn(self.clone().check_health());
    }
    /// Reads the free space where Orbit keeps its data, then opens, updates
    /// or closes what is wrong with Orbit itself. Saves only on a change.
    pub(super) async fn check_health(self: Arc<Self>) {
        let root = self.root.clone();
        let disk = tokio::task::spawn_blocking(move || health::disk(&root))
            .await
            .ok()
            .flatten();
        self.see_health(disk, chrono::Utc::now()).await;
    }
    pub(super) async fn see_health(
        &self,
        disk: Option<health::Disk>,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        let mut s = self.state.lock().await;
        let down = {
            let mut down = self.codex_down.lock().unwrap();
            if s.codex_ready {
                *down = None;
            } else if down.is_none() {
                *down = Some(now);
            }
            *down
        };
        let found = health::find(&s, disk, down, now);
        let seen = health::see(&mut s.health, &found, now);
        let pruned = health::prune(&mut s.health, now);
        if seen || pruned {
            self.dirty.store(true, Ordering::SeqCst);
        }
    }
}
