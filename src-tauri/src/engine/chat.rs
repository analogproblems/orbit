use super::{extras, Engine};
use crate::types::*;
use serde_json::json;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};

impl Engine {
    pub(super) fn chat_available(r: &Review) -> Result<(), String> {
        if active(&r.status) {
            return Err("Wait for the current turn to finish, or stop it first.".into());
        }
        if r.status == "publishing" {
            return Err("Wait for publication to finish.".into());
        }
        if r.cleanup_pending {
            return Err(
                "This worktree is being removed. Your saved chat will stay readable.".into(),
            );
        }
        if r.thread_id.is_none() {
            return Err(format!(
                "Start a review first to create its {} chat.",
                r.provider.label()
            ));
        }
        if !r.worktree.as_deref().is_some_and(|w| Path::new(w).is_dir()) {
            return Err("This review's worktree is unavailable. Start a new review of an open pull request to continue.".into());
        }
        Ok(())
    }
    pub async fn set_legacy_chat_model(
        &self,
        id: &str,
        model: String,
        effort: String,
    ) -> Result<(), String> {
        let selection = ModelSelection { model, effort };
        let mut s = self.state.lock().await;
        let r = s
            .reviews
            .iter()
            .find(|r| r.id == id)
            .ok_or("Unknown review")?;
        Self::chat_available(r)?;
        selection.validate_for(r.provider, &s)?;
        if ModelSelection::requested(r).complete() {
            return Err("This chat already has its original model and effort pinned.".into());
        }
        if let Some(pinned) = &r.legacy_chat_selection {
            return if pinned == &selection {
                Ok(())
            } else {
                Err("This chat's model and effort have already been pinned. Start a new review to use another selection.".into())
            };
        }
        self.save(
            &mut s,
            |next| {
                let r = next.reviews.iter_mut().find(|r| r.id == id).unwrap();
                r.legacy_chat_selection = Some(selection);
                Ok(())
            },
            |e| format!("Chat model was not saved: {e}"),
        )?;
        (self.emit)(&s);
        Ok(())
    }
    pub(super) async fn pending_message(&self, id: &str, message_id: &str) -> Result<(), String> {
        let state = self.state.lock().await;
        if state.reviews.iter().any(|r| {
            r.id == id
                && active(&r.status)
                && r.before_follow_up.is_some()
                && r.messages
                    .iter()
                    .any(|m| m.id == message_id && m.delivery == Some(MessageDelivery::Pending))
        }) {
            Ok(())
        } else {
            Err("Follow-up was stopped.".into())
        }
    }
    pub async fn message(self: &Arc<Self>, id: &str, text: &str) -> Result<(), String> {
        if text.trim().is_empty() || text.chars().count() > 32000 {
            return Err("Enter a message between 1 and 32,000 characters.".into());
        }
        let (r, selection, message_id) = {
            let mut s = self.state.lock().await;
            let r = s
                .reviews
                .iter()
                .find(|r| r.id == id)
                .ok_or("Unknown review")?;
            Self::chat_available(r)?;
            let selection = ModelSelection::chat(r)?;
            selection.validate_for(r.provider, &s)?;
            if s.reviews
                .iter()
                .filter(|r| active(&r.status) && r.status != "queued")
                .count()
                >= s.settings.concurrency
            {
                return Err("All review slots are in use. Wait for a review to finish before sending a follow-up.".into());
            }
            let r = s.reviews.iter_mut().find(|r| r.id == id).unwrap();
            let old = r.clone();
            r.before_follow_up = Some(Outcome {
                status: std::mem::replace(&mut r.status, "reviewing".into()),
                error: r.error.take(),
            });
            r.turn_id = None;
            r.is_review = false;
            let mut message = Message::new("user", text);
            message.delivery = Some(MessageDelivery::Pending);
            let message_id = message.id.clone();
            r.messages.push(message);
            self.dirty.store(true, Ordering::SeqCst);
            (self.emit)(&s);
            (old, selection, message_id)
        };
        let result = async {
            self.flush().await.map_err(|e| {
                format!("Could not save the pending question; no message was sent: {e}")
            })?;
            if r.provider != Provider::Codex {
                self.pending_message(id, &message_id).await?;
                return self
                    .send_cli_follow_up(&r, &selection, &message_id, text)
                    .await;
            }
            let rpc = self.client().await?;
            let thread = r.thread_id.as_ref().unwrap();
            // v0.156.1 applies changed overrides to an idle loaded thread only
            // after its subscribers detach. Still verify the response exactly.
            if r.legacy_chat_selection.is_some()
                && !r
                    .messages
                    .iter()
                    .any(|m| m.role == "user" && m.confirmed_selection.is_some())
            {
                rpc.request("thread/unsubscribe", json!({"threadId":thread}))
                    .await?;
            }
            self.pending_message(id, &message_id).await?;
            // The extra tools you've added to Codex stay off, as for the review.
            let mut config = extras::off(&rpc, &r.worktree).await?;
            config.extend([
                ("review_model".into(), json!(selection.model)),
                ("model_reasoning_effort".into(), json!(selection.effort)),
            ]);
            let resume = json!({
                "threadId": thread,
                "cwd": r.worktree,
                "approvalPolicy": "on-request",
                "approvalsReviewer": "user",
                "sandbox": "workspace-write",
                "model": selection.model,
                "config": config,
            });
            let response = rpc.request("thread/resume", resume).await?;
            selection.verify_response(&response)?;
            extras::check(&rpc, thread, None)
                .await
                .map_err(|left| left.message("your message wasn't sent"))?;
            {
                let mut state = self.state.lock().await;
                self.save(
                    &mut state,
                    |next| {
                        let run = next
                            .reviews
                            .iter_mut()
                            .find(|r| {
                                r.id == id && r.before_follow_up.is_some() && active(&r.status)
                            })
                            .ok_or("Follow-up was stopped.")?;
                        let message = run
                            .messages
                            .iter_mut()
                            .find(|m| {
                                m.id == message_id && m.delivery == Some(MessageDelivery::Pending)
                            })
                            .ok_or("Follow-up was stopped.")?;
                        message.confirmed_selection = Some(selection.clone());
                        Ok(())
                    },
                    |e| format!("Could not save confirmed chat settings; no message was sent: {e}"),
                )?;
                self.changed.store(true, Ordering::SeqCst);
            }
            self.pending_message(id, &message_id).await?;
            let turn = rpc
                .request(
                    "turn/start",
                    json!({
                        "threadId":thread, "model":selection.model, "effort":selection.effort,
                        "input":[{"type":"text","text":text}],
                    }),
                )
                .await?;
            let mut accepted = false;
            self.update(|s| {
                if let Some(run) = s.reviews.iter_mut().find(|r| r.id == id) {
                    if let Some(message) = run.messages.iter_mut().find(|m| m.id == message_id) {
                        accepted = message.delivery == Some(MessageDelivery::Pending)
                            && active(&run.status)
                            && run.before_follow_up.is_some();
                        message.delivery = Some(MessageDelivery::Sent);
                        if accepted {
                            run.turn_id = turn["turn"]["id"].as_str().map(str::to_owned);
                        }
                    }
                }
            })
            .await;
            if !accepted {
                if let Some(turn_id) = turn["turn"]["id"].as_str() {
                    let _ = rpc
                        .request(
                            "turn/interrupt",
                            json!({"threadId": thread, "turnId": turn_id}),
                        )
                        .await;
                }
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = &result {
            self.update(|s| {
                if let Some(run) = s.reviews.iter_mut().find(|r| r.id == id) {
                    // A late response from a cancelled attempt cannot end a
                    // newer follow-up or attach confirmation to its question.
                    if run
                        .messages
                        .iter()
                        .any(|m| m.id == message_id && m.delivery == Some(MessageDelivery::Pending))
                    {
                        run.end_follow_up(Some(format!("The follow-up failed: {error}")));
                    }
                }
            })
            .await;
        }
        let _ = self.flush().await;
        result
    }
}
