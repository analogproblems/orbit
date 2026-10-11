//! Reviews by a CLI reviewer, Claude or Antigravity. Each turn runs the CLI
//! in the review's worktree (see `crate::cli`); its output becomes the same
//! review events as Codex's.
use super::{
    events::{ReviewEvent, TurnEnd},
    findings,
    run::{apply_event, Next},
    worktrees::git,
    Engine,
};
use crate::{
    antigravity, claude,
    cli::{Init, Output, Running, Started, Turn, TurnKind},
    devcontainer, review_instructions,
    types::*,
};
use serde_json::json;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::mpsc;

/// How long a CLI may take to start a conversation before Orbit gives up.
const START_TIMEOUT: Duration = Duration::from_secs(120);

const EXTRACTION_PROMPT: &str = "Restate the findings from the review you just completed as JSON matching the output schema. \
    Do not run commands, read files, or review the code again. \
    Include every finding you reported, with a short title. \
    Give each finding a priority: 0 for a defect that breaks the change for most users or loses data, \
    1 for a real bug that should block merging, 2 for a minor bug, 3 for a nit. \
    If your review rated a finding's severity in its own terms, such as high or critical, say so in reviewerNotes. ";

/// The extraction prompt for a CLI reviewer.
fn extraction_prompt() -> String {
    format!(
        "{EXTRACTION_PROMPT}{}If no earlier findings are given below, use an empty priorFindings list.",
        findings::FORM_PROMPT
    )
}

const PRIOR_PROMPT: &str = "\n\nThis pull request was reviewed before. \
    Before answering, check every earlier finding below against the current code, reading files and the diff as needed but changing nothing, \
    and list each in priorFindings with its ID, its status and a one-sentence note. \
    Do not report an earlier finding again as a new finding.\n\n";

const CHECKS_PROMPT: &str = "\n\nThe repository's checks also ran after your review. \
    Before answering, read their results below. When a failure comes from this change and your review didn't report it, \
    add it to findings, reading files and the diff as needed but changing nothing.\n\n";

impl Engine {
    /// Checks the CLI reviewers, which are optional; their status shows in
    /// Settings either way.
    pub async fn check_reviewers(&self) {
        let _ = self.check_claude().await;
        let _ = self.check_antigravity().await;
        let _ = self.check_dev_containers().await;
    }
    /// Checks the Claude CLI is installed and signed in, and records whether
    /// Claude reviews can start.
    pub async fn check_claude(&self) -> Result<(), String> {
        let launcher = self.claude_launcher.lock().unwrap().clone();
        let result = claude::check(&launcher).await;
        self.update(|s| s.claude = status(&result, |v| Some(v.clone())))
            .await;
        result.map(|_| ())
    }
    /// Checks the Antigravity CLI is installed, signed in and set up safely,
    /// and records its models and whether Antigravity reviews can start.
    pub async fn check_antigravity(&self) -> Result<(), String> {
        let setup = self.antigravity.lock().unwrap().clone();
        let result = antigravity::check(&setup).await;
        self.update(|s| {
            s.antigravity = status(&result, |_| None);
            if let Ok(models) = &result {
                s.antigravity_models = models.clone();
            }
        })
        .await;
        result.map(|_| ())
    }
    #[cfg(test)]
    pub(super) fn use_claude(&self, launcher: crate::cli::Launcher) {
        *self.claude_launcher.lock().unwrap() = launcher;
    }
    #[cfg(test)]
    pub(super) fn use_antigravity(&self, setup: antigravity::Setup) {
        *self.antigravity.lock().unwrap() = setup;
    }
    /// Starts a review by a CLI reviewer in a prepared worktree. Claude runs
    /// `/code-review` against the review's base; Antigravity follows Orbit's
    /// own instructions, with the change in the prompt.
    pub(super) async fn start_cli_review(
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
        let provider = r.provider;
        let selection = ModelSelection::requested(r);
        selection.validate_for(provider, &s)?;
        let (session, prompt) = match provider {
            Provider::Antigravity => {
                let mut prompt = change_prompt(worktree, &r.base_sha).await?;
                if let Some(checks) = &r.checks {
                    prompt.push_str("\n\n");
                    prompt.push_str(&devcontainer::notes(checks));
                }
                (String::new(), prompt)
            }
            _ => (
                uuid::Uuid::new_v4().to_string(),
                claude::review_command(&selection.effort, &r.base_sha),
            ),
        };
        let turn = Turn {
            kind: TurnKind::Review,
            cwd: worktree.to_path_buf(),
            session,
            resume: false,
            model: selection.model.clone(),
            effort: selection.effort.clone(),
            prompt,
            schema: None,
            tool: self.box_server(id).filter(|_| provider == Provider::Claude),
        };
        self.run_cli_turn(id, provider, turn, move |r, session| {
            if r.status != "preparing" {
                return Err("Review was stopped.".into());
            }
            r.thread_id = Some(session.to_string());
            r.confirmed_review_selection = Some(selection);
            r.status = "reviewing".into();
            Ok(())
        })
        .await
    }
    /// Starts the extraction turn on the review's conversation. On a later
    /// round it also classifies the previous round's findings.
    pub(super) async fn start_cli_extraction(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let s = self.snapshot().await;
        let r = s
            .reviews
            .iter()
            .find(|r| r.id == id)
            .ok_or("Unknown review")?;
        let provider = r.provider;
        let selection = ModelSelection::requested(r);
        selection.validate_for(provider, &s)?;
        let session = r.thread_id.clone().ok_or(format!(
            "The review has no {} conversation.",
            provider.label()
        ))?;
        let worktree = r.worktree.clone().ok_or("The review has no worktree.")?;
        let mut prompt = extraction_prompt();
        if let Some(notes) = &r.review_prompt {
            prompt.push_str(PRIOR_PROMPT);
            prompt.push_str(notes);
        }
        // Antigravity had the checks' results with the change; Claude's
        // `/code-review` turn didn't, so it weighs them now.
        let checks = r.checks.as_ref().filter(|_| provider == Provider::Claude);
        if let Some(checks) = checks {
            prompt.push_str(CHECKS_PROMPT);
            prompt.push_str(&devcontainer::notes(checks));
        }
        let kind = if r.review_prompt.is_some() || checks.is_some() {
            TurnKind::ExtractionWithTools
        } else {
            TurnKind::Extraction
        };
        let turn = Turn {
            kind,
            cwd: worktree.into(),
            session,
            resume: true,
            model: selection.model.clone(),
            effort: selection.effort.clone(),
            prompt,
            schema: Some(findings::output_schema()),
            tool: None,
        };
        self.run_cli_turn(id, provider, turn, move |r, _| {
            if !r.extracting || !active(&r.status) {
                return Err("Review was stopped.".into());
            }
            r.extraction_selection = Some(selection);
            Ok(())
        })
        .await
    }
    /// Sends a developer's question on the review's conversation.
    pub(super) async fn send_cli_follow_up(
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
            session: r.thread_id.clone().ok_or(format!(
                "The review has no {} conversation.",
                r.provider.label()
            ))?,
            resume: true,
            model: selection.model.clone(),
            effort: selection.effort.clone(),
            prompt: text.into(),
            schema: None,
            tool: None,
        };
        let selection = selection.clone();
        let message_id = message_id.to_string();
        self.run_cli_turn(&r.id, r.provider, turn, move |run, _| {
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
    /// Stops the review's running CLI turn, if any.
    pub(super) async fn stop_cli_turn(&self, id: &str) {
        let running = self.cli_turns.lock().unwrap().remove(id);
        if let Some(running) = running {
            running.kill().await;
        }
    }
    /// Stops every running CLI turn, when Orbit exits.
    pub(super) async fn stop_all_cli_turns(&self) {
        let all: Vec<_> = self.cli_turns.lock().unwrap().drain().collect();
        for (_, running) in all {
            running.kill().await;
        }
    }
    fn start_turn(&self, provider: Provider, turn: &Turn) -> Result<Started, String> {
        match provider {
            Provider::Antigravity => {
                let setup = self.antigravity.lock().unwrap().clone();
                antigravity::start(&setup, turn)
            }
            _ => {
                let launcher = self.claude_launcher.lock().unwrap().clone();
                claude::start(&launcher, turn)
            }
        }
    }
    /// Runs one turn for review `id`. Once the CLI reports the conversation's
    /// start, its model, conversation and permission mode are checked, and
    /// `confirm` records the confirmation with the conversation's id, which
    /// fails when the review moved on; any failure stops the process. The
    /// rest of the turn then arrives as events.
    async fn run_cli_turn(
        self: &Arc<Self>,
        id: &str,
        provider: Provider,
        turn: Turn,
        confirm: impl FnOnce(&mut Review, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        let name = provider.label();
        let (running, init, outputs) = self.start_turn(provider, &turn)?;
        let running = Arc::new(running);
        self.cli_turns
            .lock()
            .unwrap()
            .insert(id.to_string(), running.clone());
        let turn_id = uuid::Uuid::new_v4().to_string();
        let started = async {
            let init = tokio::time::timeout(START_TIMEOUT, init)
                .await
                .map_err(|_| format!("{name} did not start a conversation in time."))?
                .map_err(|_| format!("{name} stopped before starting a conversation."))??;
            let session = confirmed(provider, &turn, &init)?;
            let mut state = self.state.lock().await;
            let mut next = state.clone();
            let r = next
                .reviews
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or("Review was stopped.")?;
            confirm(r, &session)?;
            r.turn_id = Some(turn_id.clone());
            self.persist(&next).map_err(|e| {
                format!("Could not save the confirmed {name} run; it was stopped: {e}")
            })?;
            *state = next;
            self.changed.store(true, Ordering::SeqCst);
            Ok(())
        }
        .await;
        if let Err(error) = started {
            self.forget_cli_turn(id, &running);
            running.kill().await;
            return Err(error);
        }
        let e = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            e.pump_cli(&id, provider, &turn_id, &turn, outputs).await;
            e.forget_cli_turn(&id, &running);
        });
        Ok(())
    }
    fn forget_cli_turn(&self, id: &str, running: &Arc<Running>) {
        let mut turns = self.cli_turns.lock().unwrap();
        if turns.get(id).is_some_and(|r| Arc::ptr_eq(r, running)) {
            turns.remove(id);
        }
    }
    /// Delivers a turn's output to its review as events.
    async fn pump_cli(
        self: &Arc<Self>,
        id: &str,
        provider: Provider,
        turn_id: &str,
        turn: &Turn,
        mut outputs: mpsc::Receiver<Output>,
    ) {
        let name = provider.label();
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
                                events.push(ReviewEvent::Report(match provider {
                                    Provider::Claude => claude::report(&text),
                                    _ => text.trim().to_string(),
                                }));
                                if let Some(changed) = changed_files(&turn.cwd).await {
                                    events.push(ReviewEvent::Notice(format!(
                                        "{name}'s review left changes in its worktree, from a command it ran or an edit it should not have made:\n{changed}"
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
                                    "{name} could not complete the review, so nothing was published. {}",
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
                            format!("{name} could not finish the turn.")
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
                self.cli_event(id, turn_id, event).await;
            }
        }
    }
    /// Applies an event from a CLI turn, unless the review has moved on to
    /// another turn or was stopped.
    async fn cli_event(self: &Arc<Self>, id: &str, turn_id: &str, event: ReviewEvent) {
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

/// A check's result as the status Settings shows.
fn status<T>(result: &Result<T, String>, version: impl Fn(&T) -> Option<String>) -> CliStatus {
    match result {
        Ok(value) => CliStatus {
            ready: true,
            error: None,
            version: version(value),
        },
        Err(error) => CliStatus {
            ready: false,
            error: Some(error.clone()),
            version: None,
        },
    }
}

/// The conversation a turn runs in, once its start is checked: the model
/// Orbit asked for and, for Antigravity, the conversation it asked to
/// continue and a permission mode that refuses what nobody approved.
fn confirmed(provider: Provider, turn: &Turn, init: &Init) -> Result<String, String> {
    let name = provider.label();
    if init.model != turn.model {
        return Err(format!(
            "{name} did not confirm the requested model '{}' (reported: {}). The run was stopped.",
            turn.model,
            if init.model.is_empty() {
                "none"
            } else {
                &init.model
            }
        ));
    }
    // Claude's conversation is the one Orbit named.
    if provider != Provider::Antigravity {
        return Ok(turn.session.clone());
    }
    let mode = init.permission_mode.as_deref().unwrap_or("");
    if !antigravity::ASKING_MODES.contains(&mode) {
        return Err(format!(
            "Antigravity would run without asking for approval (permission mode: {}), so the run was stopped. Orbit never changes Antigravity's settings: set its tool permission back to request review.",
            if mode.is_empty() { "not reported" } else { mode }
        ));
    }
    match init.session.clone().filter(|s| !s.is_empty()) {
        Some(session) if turn.session.is_empty() || session == turn.session => Ok(session),
        reported => Err(format!(
            "Antigravity did not continue the review's conversation (reported: {}). The run was stopped.",
            reported.as_deref().unwrap_or("none")
        )),
    }
}

/// Orbit's review prompt for the change from `base` to HEAD, with the
/// change computed by Orbit. Diff drivers the clone configures never run. A
/// diff too large for the prompt is written whole to a file in the
/// worktree, so nothing is left out.
async fn change_prompt(worktree: &Path, base: &str) -> Result<String, String> {
    use review_instructions::{Change, CHANGE_FILE, MAX_DIFF_BYTES};
    let range = format!("{base}...HEAD");
    let diff = |extra: Option<&'static str>| {
        let range = range.clone();
        async move {
            let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color"];
            args.extend(extra);
            args.push(&range);
            git(worktree, &args).await
        }
    };
    let stat = diff(Some("--stat")).await?;
    let patch = diff(None).await?;
    if patch.len() <= MAX_DIFF_BYTES {
        return Ok(review_instructions::review_prompt(
            base,
            &stat,
            Change::Inline(&patch),
        ));
    }
    let file = worktree.join(CHANGE_FILE);
    if std::fs::symlink_metadata(&file).is_ok() {
        return Err(format!(
            "The pull request has its own {CHANGE_FILE}, where Orbit puts a large change for the reviewer to read, so the review didn't run."
        ));
    }
    std::fs::write(&file, &patch)
        .map_err(|e| format!("Could not write the change for the reviewer: {e}"))?;
    Ok(review_instructions::review_prompt(
        base,
        &stat,
        Change::File(patch.len()),
    ))
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
    // Orbit's own copy of a large change is not the reviewer's doing.
    let ours = format!("?? {}", review_instructions::CHANGE_FILE);
    let status = status
        .lines()
        .filter(|line| *line != ours)
        .collect::<Vec<_>>()
        .join("\n");
    let status = status.trim();
    (!status.is_empty()).then(|| status.chars().take(4000).collect())
}
