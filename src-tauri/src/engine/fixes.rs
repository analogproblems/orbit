use super::{publish::By, Engine};
use crate::{
    buddy::{Fix, Question, CONTINUE_ROUNDS},
    types::*,
};
use std::sync::Arc;

/// Whether a fix Bit queued is still Bit's to do: its item is still open
/// about the same report with the same fix, Bit is on and the fix's rule is
/// still Do it.
fn still_mine(s: &Snapshot, item: &Attention, fix: Fix) -> Result<(), String> {
    let open = crate::attention::items(s)
        .into_iter()
        .find(|a| a.key == item.key)
        .ok_or("This doesn't need you any more.")?;
    if open.fix != Some(fix) || open.review_id != item.review_id {
        return Err("That isn't the fix for this any more.".into());
    }
    if crate::buddy::will_fix(s, &open) != Some(fix) {
        return Err("My rule for this isn't Do it any more.".into());
    }
    Ok(())
}

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
        self.run_fix(&item, fix, false).await?;
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
    /// notes how each went. A fix that fails, or that stopped being Bit's to
    /// do, while its item is still open is handed back to you: noted with
    /// its button, and notified as the item would have been. One whose item
    /// closed meanwhile is dropped.
    pub(super) fn do_fixes(self: &Arc<Self>) -> Vec<tokio::task::JoinHandle<()>> {
        let queued = std::mem::take(&mut *self.fixing.lock().unwrap());
        let mut tasks = vec![];
        for (item, fix) in queued {
            let e = self.clone();
            tasks.push(tokio::spawn(async move {
                let result = e.run_fix(&item, fix, true).await;
                let mut handed_back = false;
                e.update(|s| match &result {
                    Ok(()) => crate::buddy::fixed(s, &item, fix),
                    Err(error) => {
                        handed_back = crate::attention::items(s).iter().any(|a| a.key == item.key);
                        if handed_back {
                            crate::buddy::not_fixed(s, &item, fix, error);
                        }
                    }
                })
                .await;
                if handed_back {
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
            }));
        }
        tasks
    }
    /// Uses one of Orbit's own controls. None of them answers a reviewer's
    /// request or touches a repository's setup or the code under review.
    /// A fix Bit does `on_its_own` goes ahead only while it is still Bit's to
    /// do, checked again under the same lock as the change, or for a
    /// publication, again before each write.
    pub(super) async fn run_fix(
        self: &Arc<Self>,
        item: &Attention,
        fix: Fix,
        on_its_own: bool,
    ) -> Result<(), String> {
        let still = |s: &Snapshot| {
            if on_its_own {
                still_mine(s, item, fix)
            } else {
                Ok(())
            }
        };
        still(&*self.state.lock().await)?;
        let pr = || {
            item.pr_id
                .as_deref()
                .ok_or_else(|| "This isn't about a pull request.".to_string())
        };
        match fix {
            Fix::ContinueLoop => self.continue_loop_if(pr()?, CONTINUE_ROUNDS, still).await,
            Fix::ReviewAgain => self.enqueue_if(pr()?, false, still).await.map(|_| ()),
            // Bit's publication: never one automatic publication would hold,
            // judged on the fresh PR before each write.
            Fix::Publish => {
                let id = item
                    .review_id
                    .as_deref()
                    .ok_or("There's no report to publish.")?;
                self.publish_by(id, By::Bit(&still)).await
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
