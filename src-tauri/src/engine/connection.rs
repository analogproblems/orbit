use super::Engine;
use crate::{models, rpc::Rpc, types::*, usage::Usage};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{atomic::Ordering, Arc},
};
use tokio::sync::mpsc;

impl Engine {
    pub async fn connect(self: &Arc<Self>) -> Result<(), String> {
        let _guard = self.connect_lock.lock().await;
        self.retire_dead_connection().await;
        let existing = self
            .rpc
            .lock()
            .await
            .clone()
            .filter(|r| r.alive.load(Ordering::SeqCst));
        let rpc = if let Some(rpc) = existing {
            rpc
        } else {
            let (tx, rx) = mpsc::channel(2048);
            let scan_root = self.scan_root();
            let _ = std::fs::create_dir_all(&scan_root);
            let rpc = match Rpc::start(tx, &scan_root).await {
                Ok(rpc) => rpc,
                Err(err) => {
                    self.update(|s| {
                        s.codex_error = Some(err.clone());
                        s.codex_ready = false;
                    })
                    .await;
                    return Err(err);
                }
            };
            *self.rpc.lock().await = Some(rpc.clone());
            tokio::spawn(self.clone().pump(rpc.clone(), rx));
            rpc
        };
        let account = rpc
            .request("account/read", json!({"refreshToken":false}))
            .await;
        if let Ok(account) = &account {
            self.switch_usage_account(&account["account"]).await;
        }
        let account_error = match account {
            Err(error) => Some(error),
            Ok(account)
                if account["account"].is_null() && account["requiresOpenaiAuth"] == true =>
            {
                Some("Codex is not signed in. Run codex login, then reconnect.".to_string())
            }
            _ => None,
        };
        if let Some(error) = account_error {
            self.update(|s| {
                s.codex_ready = false;
                s.model_catalog_ready = false;
                s.codex_error = Some(error.clone());
            })
            .await;
            return Err(error);
        }
        self.update(|s| {
            s.codex_ready = true;
            s.codex_error = None;
            // A refresh must not invalidate the last verified catalog while
            // preparations and follow-ups are using it.
            if !s.model_catalog_ready {
                s.model_configuration_error = Some("Refreshing available models…".into());
            }
        })
        .await;
        let _ = self.flush().await;
        tokio::spawn(self.clone().read_usage());
        self.retry_scan_stops(&self.snapshot().await.scans);
        let discovery = async {
            let mut models = vec![];
            let mut cursor = Value::Null;
            let mut cursors = HashSet::new();
            loop {
                let page = rpc
                    .request("model/list", json!({"limit":100,"cursor":cursor}))
                    .await?;
                models.extend(
                    page["data"]
                        .as_array()
                        .ok_or("Codex returned an invalid model catalog.")?
                        .clone(),
                );
                cursor = page["nextCursor"].clone();
                if cursor.is_null() {
                    break;
                }
                if !cursors.insert(cursor.to_string()) {
                    return Err("Codex repeated a model catalog page.".into());
                }
            }
            models::validate_catalog(&models)?;
            Ok::<_, String>(models)
        }
        .await;
        let models = match discovery {
            Ok(models) => models,
            Err(error) => {
                let error =
                    format!("Model discovery failed: {error} Reconnect in Settings to retry.");
                self.update(|s| {
                    s.model_catalog_ready = false;
                    s.models.clear();
                    s.model_configuration_error = Some(error.clone());
                })
                .await;
                let _ = self.flush().await;
                return Err(error);
            }
        };
        let needs_config = !ModelSelection::workspace(&self.state.lock().await.settings).complete();
        let config = if needs_config {
            rpc.request("config/read", json!({"includeLayers":false}))
                .await
        } else {
            Ok(Value::Null)
        };
        self.apply_model_discovery(models, config).await
    }
    /// Clears the meter when Codex is now signed in as a different account,
    /// or with an API key, whose usage Codex does not report.
    pub(super) async fn switch_usage_account(&self, account: &Value) {
        let current = (account["type"] == "chatgpt")
            .then(|| format!("chatgpt:{}", account["email"].as_str().unwrap_or_default()));
        let previous = std::mem::replace(&mut *self.usage_account.lock().unwrap(), current.clone());
        if current.is_none() || current != previous {
            self.update_live(|s| s.usage = None).await;
        }
    }
    /// Reads the account's usage limits for the meter. A failed read keeps the
    /// last numbers, since a reconnect as another account clears them first;
    /// an account without usage windows hides the meter.
    pub(super) async fn read_usage(self: Arc<Self>) {
        let Ok(rpc) = self.client().await else { return };
        let Ok(response) = rpc
            .request(
                "account/rateLimits/read",
                json!({"excludeResetCreditDetails":true}),
            )
            .await
        else {
            return;
        };
        self.update_live(|s| s.usage = Usage::from_read(&response, now()))
            .await;
    }
    pub(super) async fn apply_model_discovery(
        &self,
        models: Vec<Value>,
        config: Result<Value, String>,
    ) -> Result<(), String> {
        let mut s = self.state.lock().await;
        s.models = models;
        s.model_catalog_ready = true;
        let result: Result<(), String> = (|| {
            let resolution = models::resolve(&s.settings, &s.models, &config)?;
            let selection = resolution.selection;
            let mut next = s.clone();
            next.model_configuration_error = None;
            next.model_selection_notice = resolution.notice;
            let selection_changed = selection != ModelSelection::workspace(&s.settings);
            if selection_changed {
                next.settings.model = selection.model.clone();
                next.settings.effort = selection.effort.clone();
            }
            if selection_changed || next.model_selection_notice != s.model_selection_notice {
                self.persist(&next).map_err(|e| format!("Resolved model selection was not saved: {e}. Reconnect to retry or choose a selection in Settings."))?;
            }
            *s = next;
            Ok(())
        })();
        if let Err(error) = &result {
            s.model_configuration_error = Some(error.clone());
        }
        self.changed.store(true, Ordering::SeqCst);
        (self.emit)(&s);
        result
    }
    /// Delivers one connection's events. Events from a connection that has been
    /// replaced are dropped, so its late disconnect cannot fail newer reviews.
    pub(super) async fn pump(self: Arc<Self>, rpc: Arc<Rpc>, mut events: mpsc::Receiver<Value>) {
        while let Some(event) = events.recv().await {
            let current = self
                .rpc
                .lock()
                .await
                .as_ref()
                .is_some_and(|r| Arc::ptr_eq(r, &rpc));
            if current {
                self.event(event).await;
            }
        }
    }
    /// A dead connection's own disconnect event is dropped once it is replaced,
    /// so its reviews are ended before a new connection takes over.
    pub(super) async fn retire_dead_connection(self: &Arc<Self>) {
        let dead = self
            .rpc
            .lock()
            .await
            .as_ref()
            .is_some_and(|r| !r.alive.load(Ordering::SeqCst));
        if dead {
            self.disconnected().await;
        }
    }
    /// Ends everything that ran on a Codex connection that has gone away.
    pub(super) async fn disconnected(self: &Arc<Self>) {
        let rpc = self.rpc.lock().await.clone();
        let why = rpc
            .as_ref()
            .and_then(|r| r.last_error.lock().unwrap().clone());
        self.codex_login_lost(rpc.as_ref());
        self.items.lock().await.clear();
        let mut recovered = vec![];
        self.update(|s| {
            s.codex_ready = false;
            if s.codex_error.is_none() {
                s.codex_error = Some(match &why {
                    Some(why) => format!(
                        "Codex disconnected. It said: {} Reconnect in Settings.",
                        crate::text::sentence(why)
                    ),
                    None => "Codex disconnected. Reconnect in Settings.".into(),
                });
            }
            s.approvals.clear();
            for scan in &mut s.scans {
                const DISCONNECTED: &str =
                    "Codex disconnected while this scan was running. Run a new scan to start over.";
                match scan.status.as_str() {
                    "preparing" => {
                        scan.status = "interrupted".into();
                        scan.finished_at = Some(now());
                        scan.error = Some(DISCONNECTED.into());
                    }
                    // Stopped once Codex reconnects, before the scan ends.
                    "running" | "cancelling" => super::scans::begin_stop(
                        scan,
                        Some("interrupted"),
                        Some(DISCONNECTED.into()),
                    ),
                    _ => {}
                }
            }
            // Other providers' reviews never ran on this connection, and a
            // queued review hasn't started: it waits for Codex to come back.
            for r in s
                .reviews
                .iter_mut()
                .filter(|r| r.provider == Provider::Codex && r.status != "queued")
            {
                if !active(&r.status)
                    || r.end_follow_up(Some("Codex disconnected during the follow-up.".into()))
                {
                    continue;
                }
                if r.end_extraction(Some("Codex disconnected during extraction.".into())) {
                    // The report is complete, so an automatic review still publishes it.
                    if r.automatic {
                        recovered.push(r.id.clone());
                    }
                } else {
                    r.status = "failed".into();
                    r.error = Some("Codex disconnected. Start a new review to retry.".into());
                }
            }
        })
        .await;
        for id in recovered {
            self.autopost(&id);
        }
        self.end_codex_question("Codex disconnected.").await;
        self.end_codex_write_up();
    }
    pub(super) async fn client(&self) -> Result<Arc<Rpc>, String> {
        self.rpc
            .lock()
            .await
            .clone()
            .filter(|r| r.alive.load(Ordering::SeqCst))
            .ok_or("Connect Codex in Settings first.".into())
    }
}
