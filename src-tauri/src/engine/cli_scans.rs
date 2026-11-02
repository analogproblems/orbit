//! Security scans by a CLI reviewer, Claude or Antigravity, for when you'd
//! rather not use Codex Security or have no OpenAI sign-in. Nothing here
//! needs Codex. The reviewer runs in the same isolated setup as a review
//! (`crate::cli`, with a review's tools), on the scan's copy of the default
//! branch, following Orbit's scan instructions
//! (`review_instructions::scan_prompt`). Its answer is the scan's report.
//! One more turn on the same conversation restates its findings in the form
//! reviews use (`findings::output_schema`), which become the scan's findings
//! on the Findings page.
use super::{
    cli_review::{changed_files, confirmed, START_TIMEOUT},
    findings, Engine,
};
use crate::{
    claude,
    cli::{Output, Turn, TurnKind},
    review_instructions,
    types::*,
};
use std::{future::Future, path::Path, pin::Pin, sync::Arc};
use tokio::sync::mpsc;

const EXTRACTION_PROMPT: &str = "Restate the findings from the security scan you just completed as JSON matching the output schema. \
    Do not run commands, read files, or scan the code again. \
    Include every weakness you reported, with a short title. \
    Give each a priority from its severity: 0 for critical, 1 for high, 2 for medium and 3 for low, \
    and the category security unless it is another kind of problem. ";

const WHOLE_REPOSITORY: &str = "The scan covered the whole repository rather than a change: \
    set completed to false only when you could not read the repository, and use an empty priorFindings list.";

/// The most refused commands a scan lists among its warnings.
const MAX_DENIALS: usize = 10;

/// The scan's extraction prompt.
fn extraction_prompt() -> String {
    format!(
        "{EXTRACTION_PROMPT}{}{WHOLE_REPOSITORY}",
        findings::FORM_PROMPT
    )
}

/// How far a scan by a CLI reviewer has come: `reading` the repository, or
/// `writing_up` its findings.
fn phase(name: &str) -> ScanProgress {
    ScanProgress {
        phase: Some(name.into()),
        updated_at: now(),
        ..ScanProgress::default()
    }
}

/// How a scan turn ended.
type Ended = Result<(String, Option<serde_json::Value>, Vec<String>), String>;

impl Engine {
    /// Starts a scan by Claude or Antigravity on its prepared copy. Once the
    /// CLI confirms the model (and, for Antigravity, a permission mode that
    /// refuses what nobody approved), the scan is running; the rest of the
    /// turn arrives as its output.
    pub(super) async fn start_cli_scan(
        self: &Arc<Self>,
        id: &str,
        worktree: &Path,
    ) -> Result<(), String> {
        let scan = self.scan_live(id).await?;
        let provider = scan.provider;
        let selection = ModelSelection {
            model: scan.model.clone(),
            effort: scan.effort.clone(),
        };
        selection.validate_for(provider, &*self.state.lock().await)?;
        let turn = Turn {
            kind: TurnKind::Scan,
            cwd: worktree.to_path_buf(),
            // Claude's conversation is named by Orbit; Antigravity reports
            // the one it started.
            session: match provider {
                Provider::Antigravity => String::new(),
                _ => uuid::Uuid::new_v4().to_string(),
            },
            resume: false,
            model: selection.model,
            effort: selection.effort,
            prompt: review_instructions::scan_prompt(scan.commit.as_deref().unwrap_or("HEAD")),
            schema: None,
            tool: None,
        };
        self.run_scan_turn(id, provider, turn, |x, session| {
            if x.status != "preparing" {
                return Err("The scan was stopped.".into());
            }
            x.thread_id = Some(session.into());
            x.status = "running".into();
            x.progress = Some(phase("reading"));
            Ok(())
        })
        .await
    }
    /// Asks the reviewer to restate the scan's findings as data, on the same
    /// conversation, with no tools. Boxed, as it starts from the end of the
    /// turn before it.
    fn start_cli_scan_extraction<'a>(
        self: &'a Arc<Self>,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(self.extract_cli_scan(id))
    }
    async fn extract_cli_scan(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let scan = self.scan_live(id).await?;
        let provider = scan.provider;
        let session = scan.thread_id.clone().ok_or(format!(
            "The scan has no {} conversation.",
            provider.label()
        ))?;
        let worktree = scan.worktree.clone().ok_or("The scan has no copy.")?;
        let turn = Turn {
            kind: TurnKind::Extraction,
            cwd: worktree.into(),
            session: session.clone(),
            resume: true,
            model: scan.model.clone(),
            effort: scan.effort.clone(),
            prompt: extraction_prompt(),
            schema: Some(findings::output_schema()),
            tool: None,
        };
        self.run_scan_turn(id, provider, turn, move |x, _| {
            if x.status != "running" || x.thread_id.as_deref() != Some(session.as_str()) {
                return Err("The scan was stopped.".into());
            }
            x.progress = Some(phase("writing_up"));
            Ok(())
        })
        .await
    }
    /// Runs one turn of scan `id`. Once the CLI reports the conversation's
    /// start, its model, conversation and permission mode are checked, and
    /// `confirm` records it, which fails when the scan was stopped. The
    /// confirmation is saved before it counts (`Engine::save`); any failure,
    /// one to save included, stops the process.
    async fn run_scan_turn(
        self: &Arc<Self>,
        id: &str,
        provider: Provider,
        turn: Turn,
        confirm: impl FnOnce(&mut SecurityScan, &str) -> Result<(), String>,
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
            // Saved before the turn goes on, as for a review: a run Orbit
            // can't record would carry on with nothing to show for it after
            // a restart.
            let mut state = self.state.lock().await;
            self.save(
                &mut state,
                |next| {
                    let x = next
                        .scans
                        .iter_mut()
                        .find(|x| x.id == id && scan_active(&x.status))
                        .ok_or("The scan was stopped.")?;
                    confirm(x, &session)?;
                    x.turn_id = Some(turn_id.clone());
                    Ok(())
                },
                |e| format!("Orbit couldn't save the confirmed {name} scan, so it stopped it: {e}"),
            )?;
            self.changed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        .await;
        if let Err(error) = started {
            self.forget_cli_turn(id, &running);
            running.kill().await;
            return Err(error);
        }
        let _ = self.flush().await;
        let e = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let ended = e.pump_scan(&id, &turn_id, outputs).await;
            e.forget_cli_turn(&id, &running);
            e.scan_turn_ended(&id, provider, &turn_id, &turn, ended)
                .await;
        });
        Ok(())
    }
    /// Reads a scan turn's output until it ends, keeping the scan's progress
    /// fresh while the reviewer works.
    async fn pump_scan(
        &self,
        id: &str,
        turn_id: &str,
        mut outputs: mpsc::Receiver<Output>,
    ) -> Ended {
        while let Some(output) = outputs.recv().await {
            match output {
                Output::Result {
                    ok: true,
                    text,
                    structured,
                    denials,
                } => return Ok((text, structured, denials)),
                Output::Result {
                    ok: false, text, ..
                } => return Err(text),
                Output::Exited(reason) => return Err(reason),
                Output::CommandStarted { .. } => {
                    self.update_live(|s| {
                        if let Some(p) = s
                            .scans
                            .iter_mut()
                            .find(|x| x.id == id && x.turn_id.as_deref() == Some(turn_id))
                            .and_then(|x| x.progress.as_mut())
                        {
                            p.updated_at = now();
                        }
                    })
                    .await;
                }
                Output::Text { .. } | Output::CommandOutput(_) => {}
            }
        }
        Err("The run ended without a result.".into())
    }
    /// Takes a scan turn's end, unless the scan was stopped or moved on: the
    /// scan's report and the extraction after it, or its findings, or why
    /// it failed.
    async fn scan_turn_ended(
        self: &Arc<Self>,
        id: &str,
        provider: Provider,
        turn_id: &str,
        turn: &Turn,
        ended: Ended,
    ) {
        let name = provider.label();
        let current =
            self.state.lock().await.scans.iter().any(|x| {
                x.id == id && x.status == "running" && x.turn_id.as_deref() == Some(turn_id)
            });
        if !current {
            return;
        }
        let (text, structured, denials) = match ended {
            Ok(done) => done,
            Err(error) => {
                let error = if error.trim().is_empty() {
                    format!("{name} could not finish the scan.")
                } else {
                    format!(
                        "{name} could not finish the scan: {}",
                        crate::text::sentence(&error)
                    )
                };
                return self.end_scan(id, "failed", Some(error)).await;
            }
        };
        let mut warnings: Vec<String> = denials
            .iter()
            .take(MAX_DENIALS)
            .map(|d| {
                format!(
                    "Refused {name}'s command `{}`, as a scan can't ask for approval.",
                    d.replace('`', "'")
                )
            })
            .collect();
        if denials.len() > MAX_DENIALS {
            warnings.push(format!(
                "Refused {} more of {name}'s commands.",
                denials.len() - MAX_DENIALS
            ));
        }
        if turn.kind == TurnKind::Scan {
            if let Some(changed) = changed_files(&turn.cwd).await {
                warnings.push(format!(
                    "{name} left changes in the scan's copy, from a command it ran or an edit it should not have made:\n{changed}"
                ));
            }
            let report = match provider {
                Provider::Claude => claude::report(&text),
                _ => text.trim().to_string(),
            };
            let kept = self.scan_results(id);
            let written = tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(&kept)?;
                std::fs::write(kept.join("report.md"), report)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()));
            self.update(|s| {
                if let Some(x) = s.scans.iter_mut().find(|x| x.id == id) {
                    x.warnings.extend(warnings);
                    x.has_report = written.is_ok();
                }
            })
            .await;
            if let Err(e) = written {
                return self
                    .end_scan(
                        id,
                        "failed",
                        Some(format!("Orbit couldn't keep the scan's report: {e}")),
                    )
                    .await;
            }
            if let Err(error) = self.start_cli_scan_extraction(id).await {
                return self.end_scan(id, "failed", Some(error)).await;
            }
            return;
        }
        // The extraction: the findings, in the form reviews use.
        let worktree = turn.cwd.to_string_lossy().into_owned();
        let parsed = match &structured {
            Some(data) => findings::parse(&data.to_string(), id, Some(&worktree), None),
            None => Err(format!("{name} didn't restate its findings.")),
        };
        let extracted = match parsed {
            Ok(extracted) => extracted,
            Err(error) => {
                self.update(|s| {
                    if let Some(x) = s.scans.iter_mut().find(|x| x.id == id) {
                        x.warnings.extend(warnings);
                    }
                })
                .await;
                return self
                    .end_scan(
                        id,
                        "failed",
                        Some(format!(
                            "{name}'s findings couldn't be read, so none are shown. Its report is kept. {error}"
                        )),
                    )
                    .await;
            }
        };
        self.remove_scan_worktree(id).await;
        let kept = self.scan_results(id);
        let bytes = tokio::task::spawn_blocking(move || crate::storage::bytes(&kept, &mut vec![]))
            .await
            .unwrap_or(0);
        self.update(|s| {
            let Some(x) = s
                .scans
                .iter_mut()
                .find(|x| x.id == id && x.status == "running")
            else {
                return;
            };
            let mut found = extracted.findings;
            found.sort_by_key(|f| f.priority);
            x.findings = found;
            x.warnings.extend(warnings);
            // What it didn't get to, as a review's published text says it.
            if !extracted.not_checked.is_empty() {
                x.warnings
                    .push(format!("Not checked: {}", extracted.not_checked.join("; ")));
            }
            x.finished_at = Some(now());
            x.disk_bytes = Some(bytes);
            x.progress = None;
            if extracted.completed {
                x.status = "completed".into();
            } else {
                x.status = "failed".into();
                x.error = Some(format!(
                    "{name} could not complete the scan. Its findings so far are kept. {}",
                    extracted.summary
                ));
            }
        })
        .await;
        let _ = self.flush().await;
    }
}
