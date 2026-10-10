use super::Engine;
use crate::{
    buddy::{Fix, Question, CONTINUE_ROUNDS},
    types::*,
};
use std::sync::Arc;

impl Engine {
    /// Uses an open item's fix, as Bit's buttons do, and notes it.
    pub async fn fix(self: &Arc<Self>, key: &str) -> Result<(), String> {
        let item = {
            let s = self.state.lock().await;
            crate::attention::items(&s)
                .into_iter()
                .find(|a| a.key == key)
                .ok_or("This doesn't need you any more.")?
        };
        let fix = item
            .fix
            .ok_or("There's no button for this one. Open it to see what it needs.")?;
        self.run_fix(&item, fix).await?;
        self.update(|s| crate::buddy::fixed(s, &item, fix)).await;
        let _ = self.flush().await;
        Ok(())
    }
    /// Answers a quick question in Bit's feed, after any notes still to be
    /// written.
    pub async fn ask_buddy(&self, question: Question) -> Result<(), String> {
        let _ = self.flush().await;
        self.update(|s| crate::buddy::ask(s, question)).await;
        let _ = self.flush().await;
        Ok(())
    }
    /// Does the fixes Bit does on its own, for items that just opened, and
    /// notes how each went. A failed fix is notified as the item would have
    /// been.
    pub(super) fn do_fixes(self: &Arc<Self>) {
        let queued = std::mem::take(&mut *self.fixing.lock().unwrap());
        for (item, fix) in queued {
            let e = self.clone();
            tokio::spawn(async move {
                let result = e.run_fix(&item, fix).await;
                e.update(|s| match &result {
                    Ok(()) => crate::buddy::fixed(s, &item, fix),
                    Err(error) => crate::buddy::not_fixed(s, &item, fix, error),
                })
                .await;
                if result.is_err() {
                    let notice = {
                        let s = e.state.lock().await;
                        s.settings
                            .notify
                            .then(|| crate::attention::notice(&s, item.clone()))
                    };
                    if let (Some(notice), Some(notify)) = (notice, e.notifier.get()) {
                        notify(&notice);
                    }
                }
            });
        }
    }
    /// Uses one of Orbit's own controls. None of them answers a reviewer's
    /// request or touches a repository's setup or the code under review.
    async fn run_fix(self: &Arc<Self>, item: &Attention, fix: Fix) -> Result<(), String> {
        let pr = || {
            item.pr_id
                .as_deref()
                .ok_or_else(|| "This isn't about a pull request.".to_string())
        };
        match fix {
            Fix::ContinueLoop => self.continue_loop(pr()?, CONTINUE_ROUNDS).await,
            Fix::ReviewAgain => self.enqueue(pr()?, false).await.map(|_| ()),
            // As a manual publication that never confirms credentials: the
            // fix is only offered for a report without them, and publishing
            // checks again.
            Fix::Publish => {
                let id = item
                    .review_id
                    .as_deref()
                    .ok_or("There's no report to publish.")?;
                self.publish(id, false, false).await
            }
            Fix::SyncNow => {
                let synced = self.sync_now().await;
                if synced.is_ok() {
                    self.update_clones_soon();
                }
                let s = self.state.lock().await;
                match crate::attention::items(&s)
                    .into_iter()
                    .find(|a| a.key == item.key)
                {
                    Some(still) => Err(still.reason),
                    None => synced,
                }
            }
            Fix::TurnOnAutopilot => self
                .update_settings(SettingsPatch {
                    automatic: Some(true),
                    ..SettingsPatch::default()
                })
                .await
                .map(|_| ()),
        }
    }
}
