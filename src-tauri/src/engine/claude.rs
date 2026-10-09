//! Claude reviews. Each turn runs `claude -p` in the review's worktree (see
//! `crate::claude`); its output becomes the same review events as Codex's.
use super::{
    events::{ReviewEvent, TurnEnd},
    findings,
    run::{apply_event, Next},
    worktrees::git,
    Engine,
};
use crate::{
    claude::{self, Output, Running, Turn, TurnKind},
    types::*,
};
use serde_json::json;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::mpsc;

/// How long Claude may take to start a session before Orbit gives up.
const START_TIMEOUT: Duration = Duration::from_secs(120);

const SANDBOX_UNAVAILABLE: &str = "Claude's sandbox isn't available on this computer (on Linux it needs bubblewrap and socat), so the reviewer could only run read-only git commands.";

const EXTRACTION_PROMPT: &str = "Restate the findings from the review you just completed as JSON matching the output schema. \
    Do not run commands, read files, or review the code again. \
    Include every finding you reported, with a short title, the file path relative to the repository root, the line range, \
    the explanation as the body, and how confident you are from 0 to 1. \
    Give each finding a priority: 0 for a defect that breaks the change for most users or loses data, \
    1 for a real bug that should block merging, 2 for a minor bug, 3 for a nit. \
    Use verdict \"clean\" only when you reported no findings and no earlier finding is still_open or declined_disputed. \
    List tests or checks you could not run in testsNotRun. \
    Set completed to false only when the review could not read or check the change, for example because a command it needed was refused, \
    and then say why in the summary. \
    If no earlier findings are given below, use an empty priorFindings list.";

const PRIOR_PROMPT: &str = "\n\nThis pull request was reviewed before. \
    Before answering, check every earlier finding below against the current code, reading files and the diff as needed but changing nothing, \
    and list each in priorFindings with its ID, its status and a one-sentence note. \
    Do not report an earlier finding again as a new finding.\n\n";

impl Engine {
    /// Checks the Claude CLI is installed and signed in, and records whether
    /// Claude reviews can start.
    pub async fn check_claude(&self) -> Result<(), String> {
        let launcher = self.claude_launcher.lock().unwrap().clone();
        let result = launcher.check().await;
        self.update(|s| {
            s.claude = match &result {
                Ok(version) => ClaudeStatus {
                    ready: true,
                    error: None,
                    version: Some(version.clone()),
                },
                Err(error) => ClaudeStatus {
                    ready: false,
                    error: Some(error.clone()),
                    version: None,
                },
            };
        })
        .await;
        result.map(|_| ())
    }
    #[cfg(test)]
    pub(super) fn use_claude(&self, launcher: claude::Launcher) {
        *self.claude_launcher.lock().unwrap() = launcher;
    }
    /// Starts a Claude review in a prepared worktree: `/code-review` of the
    /// change against the review's base.
    pub(super) async fn start_claude_review(
        self: &Arc<Self>,
        id: &str,
        worktree: &Path,
    ) -> Result<(), String> {
        let s = self.snapshot().await;
        let r = s
            .reviews
            .iter()
            .find(|r| r.id == id)
            .ok_or("Unknown review")?;
        let selection = ModelSelection::requested(r);
        selection.validate_for(Provider::Claude, &s)?;
        let session = uuid::Uuid::new_v4().to_string();
        let turn = Turn {
            kind: TurnKind::Review,
            cwd: worktree.to_path_buf(),
            session: session.clone(),
            resume: false,
            model: selection.model.clone(),
            effort: selection.effort.clone(),
            prompt: claude::review_command(&selection.effort, &r.base_sha),
            schema: None,
        };
        self.run_claude_turn(id, turn, move |r| {
            if r.status != "preparing" {
                return Err("Review was stopped.".into());
            }
            r.thread_id = Some(session);
            r.confirmed_review_selection = Some(selection);
            r.status = "reviewing".into();
            Ok(())
        })
        .await
    }
    /// Starts the extraction turn on the review's session. On a later round
    /// it also classifies the previous round's findings.
    pub(super) async fn start_claude_extraction(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let s = self.snapshot().await;
        let r = s
            .reviews
            .iter()
            .find(|r| r.id == id)
            .ok_or("Unknown review")?;
        let selection = ModelSelection::requested(r);
        selection.validate_for(Provider::Claude, &s)?;
        let session = r
            .thread_id
            .clone()
            .ok_or("The review has no Claude session.")?;
        let worktree = r.worktree.clone().ok_or("The review has no worktree.")?;
        let (kind, prompt) = match &r.review_prompt {
            Some(notes) => (
                TurnKind::ExtractionWithTools,
                format!("{EXTRACTION_PROMPT}{PRIOR_PROMPT}{notes}"),
            ),
            None => (TurnKind::Extraction, EXTRACTION_PROMPT.to_string()),
        };
        let turn = Turn {
            kind,
            cwd: worktree.into(),
            session,
            resume: true,
            model: selection.model.clone(),
            effort: selection.effort.clone(),
            prompt,
            schema: Some(extraction_schema()),
        };
        self.run_claude_turn(id, turn, move |r| {
            if !r.extracting || !active(&r.status) {
                return Err("Review was stopped.".into());
            }
            r.extraction_selection = Some(selection);
            Ok(())
        })
        .await
    }
    /// Sends a developer's question on the review's session.
    pub(super) async fn send_claude_follow_up(
        self: &Arc<Self>,
        r: &Review,
        selection: &ModelSelection,
        message_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let turn = Turn {
            kind: TurnKind::FollowUp,
            cwd: r
                .worktree
                .clone()
                .ok_or("The review has no worktree.")?
                .into(),
            session: r
                .thread_id
                .clone()
                .ok_or("The review has no Claude session.")?,
            resume: true,
            model: selection.model.clone(),
            effort: selection.effort.clone(),
            prompt: text.into(),
            schema: None,
        };
        let selection = selection.clone();
        let message_id = message_id.to_string();
        self.run_claude_turn(&r.id, turn, move |run| {
            let waiting = active(&run.status) && run.before_follow_up.is_some();
            let message = run
                .messages
                .iter_mut()
                .find(|m| m.id == message_id && m.delivery == Some(MessageDelivery::Pending))
                .filter(|_| waiting)
                .ok_or("Follow-up was stopped.")?;
            message.confirmed_selection = Some(selection);
            message.delivery = Some(MessageDelivery::Sent);
            Ok(())
        })
        .await
    }
    /// Stops the review's running Claude turn, if any.
    pub(super) async fn stop_claude(&self, id: &str) {
        let running = self.claude_turns.lock().unwrap().remove(id);
        if let Some(running) = running {
            running.kill().await;
        }
    }
    /// Stops every running Claude turn, when Orbit exits.
    pub(super) async fn stop_all_claude(&self) {
        let all: Vec<_> = self.claude_turns.lock().unwrap().drain().collect();
        for (_, running) in all {
            running.kill().await;
        }
    }
    /// Runs one turn for review `id`. Once Claude reports the session's
    /// model, it is checked against the requested one and `confirm` records
    /// the confirmation, which fails when the review moved on; either failure
    /// stops the process. The rest of the turn then arrives as events.
    async fn run_claude_turn(
        self: &Arc<Self>,
        id: &str,
        turn: Turn,
        confirm: impl FnOnce(&mut Review) -> Result<(), String>,
    ) -> Result<(), String> {
        let launcher = self.claude_launcher.lock().unwrap().clone();
        let (running, init, outputs) = claude::start(&launcher, &turn)?;
        let running = Arc::new(running);
        self.claude_turns
            .lock()
            .unwrap()
            .insert(id.to_string(), running.clone());
        let turn_id = uuid::Uuid::new_v4().to_string();
        let started = async {
            let init = tokio::time::timeout(START_TIMEOUT, init)
                .await
                .map_err(|_| "Claude did not start a session in time.".to_string())?
                .map_err(|_| "Claude stopped before starting a session.".to_string())??;
            if init.model != turn.model {
                return Err(format!(
                    "Claude did not confirm the requested model '{}' (reported: {}). The run was stopped.",
                    turn.model,
                    if init.model.is_empty() { "none" } else { &init.model }
                ));
            }
            let mut state = self.state.lock().await;
            let mut next = state.clone();
            let r = next
                .reviews
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or("Review was stopped.")?;
            confirm(r)?;
            r.turn_id = Some(turn_id.clone());
            self.persist(&next)
                .map_err(|e| format!("Could not save the confirmed Claude run; it was stopped: {e}"))?;
            *state = next;
            self.changed.store(true, Ordering::SeqCst);
            Ok(())
        }
        .await;
        if let Err(error) = started {
            self.forget_claude_turn(id, &running);
            running.kill().await;
            return Err(error);
        }
        let e = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            e.pump_claude(&id, &turn_id, &turn, &running, outputs).await;
            e.forget_claude_turn(&id, &running);
        });
        Ok(())
    }
    fn forget_claude_turn(&self, id: &str, running: &Arc<Running>) {
        let mut turns = self.claude_turns.lock().unwrap();
        if turns.get(id).is_some_and(|r| Arc::ptr_eq(r, running)) {
            turns.remove(id);
        }
    }
    /// Delivers a turn's output to its review as events.
    async fn pump_claude(
        self: &Arc<Self>,
        id: &str,
        turn_id: &str,
        turn: &Turn,
        running: &Running,
        mut outputs: mpsc::Receiver<Output>,
    ) {
        while let Some(output) = outputs.recv().await {
            let mut events = vec![];
            match output {
                Output::Text { item, text } if turn.kind == TurnKind::FollowUp => {
                    events.push(ReviewEvent::Text { item, text })
                }
                // A review's answer is its report and an extraction's is its
                // data, both taken from the result.
                Output::Text { .. } => {}
                Output::CommandStarted { command, .. } => {
                    events.push(ReviewEvent::CommandStarted(command))
                }
                Output::CommandOutput(output) => events.push(ReviewEvent::CommandOutput(output)),
                Output::Result {
                    ok,
                    text,
                    structured,
                    denials,
                } => {
                    events.extend(denials.into_iter().map(ReviewEvent::Declined));
                    let mut continue_turn = true;
                    if ok {
                        match turn.kind {
                            TurnKind::Review => {
                                if running.sandbox_unavailable.load(Ordering::SeqCst) {
                                    events.push(ReviewEvent::Notice(SANDBOX_UNAVAILABLE.into()));
                                }
                                events.push(ReviewEvent::Report(claude::report(&text)));
                                if let Some(changed) = changed_files(&turn.cwd).await {
                                    events.push(ReviewEvent::Notice(format!(
                                        "Claude's review left changes in its worktree, from a command it ran or an edit it should not have made:\n{changed}"
                                    )));
                                }
                            }
                            // A review that could not read or check the change
                            // failed, however clean its findings look.
                            TurnKind::Extraction | TurnKind::ExtractionWithTools
                                if structured
                                    .as_ref()
                                    .is_some_and(|data| data["completed"] == false) =>
                            {
                                let said = structured.as_ref().and_then(|d| d["summary"].as_str());
                                events.push(ReviewEvent::Incomplete(format!(
                                    "Claude could not complete the review, so nothing was published. {}",
                                    said.unwrap_or_default()
                                )));
                                continue_turn = false;
                            }
                            TurnKind::Extraction | TurnKind::ExtractionWithTools => {
                                if let Some(data) = structured {
                                    events.push(ReviewEvent::Text {
                                        item: format!("{turn_id}-findings"),
                                        text: data.to_string(),
                                    });
                                }
                            }
                            TurnKind::FollowUp => {}
                        }
                        if continue_turn {
                            events.push(ReviewEvent::TurnDone(TurnEnd::Completed));
                        }
                    } else {
                        let message = if text.trim().is_empty() {
                            "Claude could not finish the turn.".to_string()
                        } else {
                            text
                        };
                        events.push(ReviewEvent::TurnDone(TurnEnd::Failed(
                            json!({ "message": message }),
                        )));
                    }
                }
                Output::Exited(reason) => events.push(ReviewEvent::TurnDone(TurnEnd::Failed(
                    json!({ "message": reason }),
                ))),
            }
            for event in events {
                self.claude_event(id, turn_id, event).await;
            }
        }
    }
    /// Applies an event from a Claude turn, unless the review has moved on
    /// to another turn or was stopped.
    async fn claude_event(self: &Arc<Self>, id: &str, turn_id: &str, event: ReviewEvent) {
        let mut next = Next::Nothing;
        self.update(|s| {
            let current = s
                .reviews
                .iter()
                .any(|r| r.id == id && active(&r.status) && r.turn_id.as_deref() == Some(turn_id));
            if current {
                next = apply_event(s, id, event);
            }
        })
        .await;
        self.follow(id, next);
    }
}

/// Files a review changed in its worktree, or None when it is unchanged or
/// cannot be checked.
async fn changed_files(worktree: &Path) -> Option<String> {
    let status = git(
        worktree,
        &["status", "--porcelain", "--untracked-files=all"],
    )
    .await
    .ok()?;
    let status = status.trim();
    (!status.is_empty()).then(|| status.chars().take(4000).collect())
}

/// Orbit's findings schema, plus whether the review could read and check
/// the change: a review that could not would otherwise look clean.
fn extraction_schema() -> serde_json::Value {
    let mut schema = findings::output_schema();
    schema["required"]
        .as_array_mut()
        .expect("schema lists its required fields")
        .push("completed".into());
    schema["properties"]["completed"] = json!({"type": "boolean"});
    schema
}
