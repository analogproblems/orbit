//! Questions you type to Bit, answered by Codex in a thread of their own:
//! read-only, in an empty folder, with no network and no approvals. The
//! answer's buttons run only when you tap them.
use super::{publish::By, Engine};
use crate::{
    ask::{self, Action},
    buddy,
    types::*,
};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

/// How long Bit waits for Codex's answer.
pub(super) const ANSWER_TIMEOUT: Duration = Duration::from_secs(180);

/// The question Codex is answering.
pub(super) struct Asking {
    /// The question's note.
    note: u64,
    thread: Option<String>,
    turn: Option<String>,
    /// Codex's latest message, its answer once the turn completes.
    text: String,
}

impl Engine {
    /// Asks Codex a question you typed. The question shows straight away
    /// and its answer follows in the same note.
    pub async fn ask_buddy_anything(self: &Arc<Self>, question: &str) -> Result<(), String> {
        let question = question.trim();
        if question.is_empty() || question.chars().count() > ask::MAX_QUESTION {
            return Err(format!(
                "Ask a question of 1 to {} characters.",
                ask::MAX_QUESTION
            ));
        }
        let (note, prompt, instructions, selection) = {
            let mut s = self.state.lock().await;
            let name = s.settings.buddy.name.clone();
            if !s.settings.buddy.enabled {
                return Err(format!("Turn {name} on in Settings first."));
            }
            if self.asking.lock().unwrap().is_some() {
                return Err(format!("{name} is still answering your last question."));
            }
            let selection = ModelSelection {
                model: s.settings.model.clone(),
                effort: s.settings.effort.clone(),
            };
            selection.validate_execution(&s).map_err(|e| {
                format!("{name} answers typed questions with Codex, which isn't ready: {e}")
            })?;
            let prompt = ask::prompt(&s, question);
            let instructions = ask::instructions(&s.settings.buddy);
            let note = buddy::asked(&mut s, question);
            *self.asking.lock().unwrap() = Some(Asking {
                note,
                thread: None,
                turn: None,
                text: String::new(),
            });
            self.dirty.store(true, Ordering::SeqCst);
            (note, prompt, instructions, selection)
        };
        let _ = self.flush().await;
        if let Err(error) = self
            .start_answer(note, &prompt, &instructions, &selection)
            .await
        {
            self.answer_done(note, Err(error)).await;
            return Ok(());
        }
        let e = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(ANSWER_TIMEOUT).await;
            e.answer_done(note, Err("Codex took too long.".into()))
                .await;
        });
        Ok(())
    }
    /// Starts Codex on the question, in a new thread that can't change
    /// anything: read-only, in an empty folder of Orbit's, with no network
    /// and no approvals.
    async fn start_answer(
        &self,
        note: u64,
        prompt: &str,
        instructions: &str,
        selection: &ModelSelection,
    ) -> Result<(), String> {
        let rpc = self.client().await?;
        let folder = self.root.join("bit");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let thread = rpc
            .request(
                "thread/start",
                json!({
                    "cwd": folder,
                    "approvalPolicy": "never",
                    "sandbox": "read-only",
                    "model": selection.model,
                    "config": {"model_reasoning_effort": selection.effort},
                    "developerInstructions": instructions,
                }),
            )
            .await?;
        let thread_id = thread["thread"]["id"]
            .as_str()
            .ok_or("Codex didn't start a thread.")?
            .to_string();
        let claimed = selection.verify_response(&thread).and_then(|()| {
            self.asking_mut(note, |a| a.thread = Some(thread_id.clone()))
                .ok_or_else(|| "The question was dropped.".to_string())
        });
        if let Err(error) = claimed {
            let _ = rpc
                .request_timeout("thread/archive", json!({"threadId": thread_id}), 10)
                .await;
            return Err(error);
        }
        let turn = rpc
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "model": selection.model,
                    "effort": selection.effort,
                    "input": [{"type": "text", "text": prompt}],
                    "outputSchema": ask::output_schema(),
                }),
            )
            .await?;
        let turn_id = turn["turn"]["id"].as_str().map(str::to_owned);
        self.asking_mut(note, |a| a.turn = turn_id);
        Ok(())
    }
    /// Changes the question being answered, if it is still `note`.
    fn asking_mut(&self, note: u64, f: impl FnOnce(&mut Asking)) -> Option<()> {
        let mut asking = self.asking.lock().unwrap();
        let a = asking.as_mut().filter(|a| a.note == note)?;
        f(a);
        Some(())
    }
    /// Whether a Codex thread is the one answering Bit's question.
    pub(super) fn answering(&self, thread: Option<&str>) -> bool {
        let asking = self.asking.lock().unwrap();
        thread.is_some() && asking.as_ref().and_then(|a| a.thread.as_deref()) == thread
    }
    /// Applies a Codex notification from the answering thread.
    pub(super) async fn answer_event(self: &Arc<Self>, method: &str, p: &Value) {
        match method {
            "item/completed" if p["item"]["type"] == "agentMessage" => {
                let text = p["item"]["text"].as_str().unwrap_or_default().to_string();
                let mut asking = self.asking.lock().unwrap();
                if let Some(a) = asking.as_mut() {
                    a.text = text;
                }
            }
            "turn/completed" => {
                let Some((note, text)) = self
                    .asking
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|a| (a.note, a.text.clone()))
                else {
                    return;
                };
                let result = match p["turn"]["status"].as_str() {
                    Some("completed") => Ok(text),
                    Some("interrupted") => Err("Codex stopped before it finished.".into()),
                    _ => Err(match p["turn"]["error"]["message"].as_str() {
                        Some(message) => {
                            let message = message.trim();
                            let end = if message.ends_with(['.', '!', '?']) {
                                ""
                            } else {
                                "."
                            };
                            format!("Codex said: {message}{end}")
                        }
                        None => "Codex couldn't finish.".into(),
                    }),
                };
                self.answer_done(note, result).await;
            }
            _ => {}
        }
    }
    /// Ends question `note` with Codex's reply or why there is none, and
    /// puts its thread away. Only the first ending counts.
    pub(super) async fn answer_done(self: &Arc<Self>, note: u64, reply: Result<String, String>) {
        let asking = {
            let mut asking = self.asking.lock().unwrap();
            if asking.as_ref().map(|a| a.note) != Some(note) {
                return;
            }
            asking.take().unwrap()
        };
        if let Some(thread) = asking.thread {
            let rpc = self.rpc.lock().await.clone();
            let unfinished = reply.is_err().then_some(asking.turn).flatten();
            if let Some(rpc) = rpc {
                tokio::spawn(async move {
                    if let Some(turn) = unfinished {
                        let _ = rpc
                            .request_timeout(
                                "turn/interrupt",
                                json!({"threadId": thread, "turnId": turn}),
                                10,
                            )
                            .await;
                    }
                    let _ = rpc
                        .request_timeout("thread/archive", json!({"threadId": thread}), 10)
                        .await;
                });
            }
        }
        self.update(|s| match reply.and_then(|reply| ask::answer(s, &reply)) {
            Ok((text, proposals)) => buddy::answered(s, note, text, proposals),
            Err(error) => buddy::not_answered(s, note, &error),
        })
        .await;
        let _ = self.flush().await;
    }
    /// Ends the question being answered, if any, without an answer.
    pub(super) async fn end_question(self: &Arc<Self>, why: &str) {
        let note = self.asking.lock().unwrap().as_ref().map(|a| a.note);
        if let Some(note) = note {
            self.answer_done(note, Err(why.into())).await;
        }
    }
    /// Uses a button from an answer, after checking it against Orbit's
    /// state now, as the same control elsewhere in the app does.
    pub async fn use_proposal(self: &Arc<Self>, note: u64, proposal: u32) -> Result<(), String> {
        let p = {
            let s = self.state.lock().await;
            buddy::proposal(&s, note, proposal).cloned()
        }
        .ok_or("This button isn't there any more.")?;
        if p.done {
            return Err("That's done already.".into());
        }
        let pr = || {
            p.pr_id
                .as_deref()
                .ok_or_else(|| "This button isn't about a pull request.".to_string())
        };
        match p.action {
            Action::ContinueLoop => {
                self.continue_loop(pr()?, crate::buddy::CONTINUE_ROUNDS)
                    .await
            }
            Action::StopLoop => self.stop_loop(pr()?).await,
            Action::ReviewAgain => self.enqueue(pr()?, false).await.map(|_| ()),
            // Bit's publication: never one automatic publication would
            // hold, judged on the fresh PR before each write.
            Action::Publish => {
                let id = p
                    .review_id
                    .as_deref()
                    .ok_or("There's no report to publish.")?;
                self.publish_by(id, By::Bit(&|_: &Snapshot| Ok(()))).await
            }
            Action::SyncNow => {
                let turn = self.poll_lock.lock().await;
                let synced = self.poll_holding(turn).await;
                if synced.is_ok() {
                    self.update_clones_soon();
                }
                synced
            }
            Action::ChangeSettings => {
                let patch: SettingsPatch = p
                    .settings
                    .clone()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .ok_or("This settings change isn't valid.")?;
                if patch.trusted_authors.is_some() {
                    return Err("Change trusted accounts in Settings yourself.".into());
                }
                self.update_settings(patch).await.map(|_| ())
            }
        }?;
        self.update(|s| buddy::proposal_done(s, note, proposal))
            .await;
        let _ = self.flush().await;
        Ok(())
    }
}
