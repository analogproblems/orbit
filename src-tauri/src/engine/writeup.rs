//! Plain-language write-ups of each connected repository's merged and
//! closed pull requests for the Decisions page, made one at a time in the
//! background by Codex or Claude with no tools, and saved beside the
//! history (see `crate::ledger::writeup`). A pull request is written up
//! again only when what the writer reads of it changes. Off until you turn
//! it on in Settings, as each one uses the writer.
use super::{
    extras::{self, sentence},
    ledger::{blocking, Reading},
    *,
};
use crate::{
    cli::Output,
    ledger::{
        self as store,
        writeup::{self, WriteUp},
        History,
    },
    models::ModelSelection,
};
use serde_json::json;
use std::path::Path;
use tokio::sync::oneshot;

/// How long one write-up may take.
#[cfg(not(test))]
const WRITE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
#[cfg(test)]
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Codex write-ups wait while Codex's usage is this full or more, so
/// reviews keep what is left.
const USAGE_PAUSE: f64 = 90.0;
/// A pull request whose write-up failed this many times from the same
/// input waits until it changes or Orbit starts again.
const TRIES: u32 = 2;

/// The Codex thread writing a pull request up.
pub(super) struct Writing {
    thread: String,
    /// Codex's latest message, its answer once the turn completes.
    text: String,
    done: Option<oneshot::Sender<Result<String, String>>>,
}

/// Each stored pull request that is merged or closed and whose write-up is
/// missing or was made from something else, newest first, with the hash of
/// what the writer would read; and each repository's write-ups so far.
type Due = (
    Vec<(History, String)>,
    HashMap<String, HashMap<u64, WriteUp>>,
);
fn due(root: &Path, repos: &[String]) -> Due {
    let mut out = vec![];
    let mut made = HashMap::new();
    for repo in repos {
        // A repository whose history can't be listed is skipped; its read
        // says why.
        let (Ok((stored, _)), Ok(writeups)) = (
            store::load_repo(root, repo),
            store::load_writeups(root, repo),
        ) else {
            continue;
        };
        for s in stored.iter().filter(|s| s.pr.state != "open") {
            let h = store::derive(s);
            let (_, hash) = writeup::input(&h, &[]);
            if writeups.get(&h.number).is_none_or(|w| w.input_hash != hash) {
                out.push((h, hash));
            }
        }
        made.insert(repo.clone(), writeups);
    }
    out.sort_by(|a, b| b.0.updated_at.cmp(&a.0.updated_at));
    (out, made)
}

/// Who writes and with what, or why write-ups wait; None when they are off.
fn writer(s: &Snapshot) -> Result<Option<(Writer, ModelSelection)>, String> {
    let w = s.settings.write_ups;
    if !w.enabled {
        return Ok(None);
    }
    match w.writer {
        Writer::Codex => {
            if !s.codex_ready {
                return Err("Write-ups are waiting for Codex to connect.".into());
            }
            let selection = ModelSelection::workspace(&s.settings);
            selection
                .validate_execution(s)
                .map_err(|e| format!("Write-ups are waiting: {}", sentence(&e)))?;
            if s.usage.as_ref().is_some_and(nearly_used_up) {
                return Err(format!(
                    "Write-ups are waiting while Codex's usage is over {USAGE_PAUSE}%, so reviews keep what's left."
                ));
            }
            Ok(Some((Writer::Codex, selection)))
        }
        Writer::Claude => {
            if !s.claude.ready {
                return Err("Write-ups are waiting for Claude. Check it in Settings.".into());
            }
            Ok(Some((
                Writer::Claude,
                crate::models::claude_selection(&s.settings),
            )))
        }
    }
}
/// `orbit#64`.
fn short_ref(h: &History) -> String {
    format!(
        "{}#{}",
        h.repo.split('/').nth(1).unwrap_or(&h.repo),
        h.number
    )
}
fn nearly_used_up(u: &crate::usage::Usage) -> bool {
    let l = &u.rate_limits;
    l.rate_limit_reached_type.is_some()
        || [&l.primary, &l.secondary]
            .into_iter()
            .flatten()
            .any(|w| w.used_percent >= USAGE_PAUSE)
}

impl Engine {
    /// Starts writing up what is due in the background, unless that is
    /// already running. Returns whether it started.
    pub fn write_up_soon(self: &Arc<Self>) -> bool {
        if self.writing_up.swap(true, Ordering::SeqCst) {
            return false;
        }
        let e = self.clone();
        tokio::spawn(async move {
            let _writing = Reading(&e.writing_up);
            e.write_up_due().await;
        });
        true
    }
    /// Writes up what is due, newest first, until it is all done, write-ups
    /// are turned off, or one fails. One that failed before goes last, so it
    /// doesn't hold up the rest.
    pub(super) async fn write_up_due(&self) {
        let (on, repos) = {
            let s = self.state.lock().await;
            let repos: Vec<String> = s
                .settings
                .repositories
                .iter()
                .map(|r| r.name.clone())
                .collect();
            (s.settings.write_ups.enabled, repos)
        };
        if !on {
            self.update_live(|s| {
                s.ledger.writing = None;
                s.ledger.write_error = None;
            })
            .await;
            return;
        }
        let root = self.root.clone();
        let Ok((mut todo, mut made)) = blocking(move || Ok(due(&root, &repos))).await else {
            return;
        };
        let mut set_aside = vec![];
        {
            let failed = self.write_failed.lock().unwrap();
            let tries = |h: &History, hash: &String| {
                failed
                    .get(&(h.repo.to_ascii_lowercase(), h.number))
                    .filter(|(f, _)| f == hash)
                    .map_or(0, |(_, n)| *n)
            };
            todo.retain(|(h, hash)| {
                let keep = tries(h, hash) < TRIES;
                if !keep {
                    set_aside.push(short_ref(h));
                }
                keep
            });
            todo.sort_by_key(|(h, hash)| tries(h, hash) > 0);
        }
        let total = todo.len() as u32;
        let mut error = (!set_aside.is_empty()).then(|| {
            format!(
                "Couldn't write up {} after {TRIES} tries. Orbit tries again once it changes.",
                set_aside.join(", ")
            )
        });
        for (done, (h, hash)) in todo.into_iter().enumerate() {
            let ready = writer(&*self.state.lock().await);
            let (writer, selection) = match ready {
                Ok(Some(ready)) => ready,
                Ok(None) => break,
                Err(why) => {
                    error = Some(why);
                    break;
                }
            };
            self.update_live(|s| {
                s.ledger.writing = Some(LedgerProgress {
                    repo: h.repo.clone(),
                    number: Some(h.number),
                    done: done as u32,
                    total,
                })
            })
            .await;
            let writeups = made.entry(h.repo.clone()).or_default();
            let areas = writeup::known_areas(writeups.values());
            let (input, _) = writeup::input(&h, &areas);
            let answer = match writer {
                Writer::Codex => self.write_with_codex(&input, &selection).await,
                Writer::Claude => self.write_with_claude(&input, &selection).await,
            };
            let made_up = answer.and_then(|(output, model)| {
                let a = writeup::read(&output, &h)?;
                Ok(WriteUp {
                    version: writeup::WRITEUP_VERSION,
                    repo: h.repo.clone(),
                    number: h.number,
                    input_hash: hash.clone(),
                    writer: writer.label().into(),
                    model,
                    written_at: now(),
                    summary: a.summary,
                    areas: a.areas,
                    decisions: a.decisions,
                    finding_notes: a.finding_notes,
                })
            });
            let saved = match made_up {
                Ok(w) => {
                    let root = self.root.clone();
                    let to_save = w.clone();
                    blocking(move || store::save_writeup(&root, &to_save))
                        .await
                        .map(|()| w)
                }
                Err(e) => Err(e),
            };
            let key = (h.repo.to_ascii_lowercase(), h.number);
            match saved {
                Ok(w) => {
                    self.write_failed.lock().unwrap().remove(&key);
                    writeups.insert(h.number, w);
                    self.update(|s| s.ledger.version += 1).await;
                }
                Err(e) => {
                    let mut failed = self.write_failed.lock().unwrap();
                    let tries = failed.entry(key).or_insert_with(|| (hash.clone(), 0));
                    if tries.0 != hash {
                        *tries = (hash.clone(), 0);
                    }
                    tries.1 += 1;
                    error = Some(format!(
                        "Couldn't write up {}: {}",
                        short_ref(&h),
                        sentence(&e)
                    ));
                    break;
                }
            }
        }
        self.update_live(|s| {
            s.ledger.writing = None;
            s.ledger.write_error = error;
        })
        .await;
    }
    /// Asks Codex for a write-up in a thread of its own that can't change
    /// anything: read-only, in an empty folder of Orbit's, with no network,
    /// no approvals and none of Codex's other tools. Returns the answer
    /// and the model.
    async fn write_with_codex(
        &self,
        input: &str,
        selection: &ModelSelection,
    ) -> Result<(Value, String), String> {
        let rpc = self.client().await?;
        let folder = self.root.join("writeups");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let codex_config = extras::codex_config(&rpc, &folder).await?;
        let thread = rpc
            .request(
                "thread/start",
                json!({
                    "cwd": folder,
                    "approvalPolicy": "never",
                    "sandbox": "read-only",
                    "model": selection.model,
                    "config": crate::ask::thread_config(&selection.effort, &codex_config),
                    "developerInstructions": writeup::instructions(),
                }),
            )
            .await?;
        let thread_id = thread["thread"]["id"]
            .as_str()
            .ok_or("Codex didn't start a thread.")?
            .to_string();
        let (done, answer) = oneshot::channel();
        let result: Result<String, String> = async {
            selection.verify_response(&thread)?;
            extras::check(&rpc, &thread_id, None)
                .await
                .map_err(|left| left.message("the write-up didn't start"))?;
            *self.writing_thread.lock().unwrap() = Some(Writing {
                thread: thread_id.clone(),
                text: String::new(),
                done: Some(done),
            });
            let turn = rpc
                .request(
                    "turn/start",
                    json!({
                        "threadId": thread_id,
                        "model": selection.model,
                        "effort": selection.effort,
                        "input": [{"type": "text", "text": input}],
                        "outputSchema": writeup::schema(),
                    }),
                )
                .await?;
            match tokio::time::timeout(WRITE_TIMEOUT, answer).await {
                Ok(Ok(answer)) => answer,
                Ok(Err(_)) => Err("Codex stopped answering.".into()),
                Err(_) => {
                    if let Some(turn) = turn["turn"]["id"].as_str() {
                        let _ = rpc
                            .request_timeout(
                                "turn/interrupt",
                                json!({"threadId": thread_id, "turnId": turn}),
                                10,
                            )
                            .await;
                    }
                    Err("Codex took too long.".into())
                }
            }
        }
        .await;
        *self.writing_thread.lock().unwrap() = None;
        let _ = rpc
            .request_timeout("thread/archive", json!({"threadId": thread_id}), 10)
            .await;
        let output = serde_json::from_str(&result?)
            .map_err(|_| "Codex's answer wasn't in the shape asked for.".to_string())?;
        Ok((output, selection.model.clone()))
    }
    /// Applies a Codex notification from the thread writing a pull request
    /// up. Returns whether it was that thread's.
    pub(super) fn write_up_event(&self, thread: Option<&str>, method: &str, p: &Value) -> bool {
        let mut writing = self.writing_thread.lock().unwrap();
        let Some(w) = writing
            .as_mut()
            .filter(|w| thread == Some(w.thread.as_str()))
        else {
            return false;
        };
        match method {
            "item/completed" if p["item"]["type"] == "agentMessage" => {
                w.text = p["item"]["text"].as_str().unwrap_or_default().into();
            }
            "turn/completed" => {
                let result = match p["turn"]["status"].as_str() {
                    Some("completed") => Ok(std::mem::take(&mut w.text)),
                    Some("interrupted") => Err("Codex stopped before it finished.".into()),
                    _ => Err(match p["turn"]["error"]["message"].as_str() {
                        Some(message) => format!("Codex said: {}", sentence(message)),
                        None => "Codex couldn't finish.".into(),
                    }),
                };
                if let Some(done) = w.done.take() {
                    let _ = done.send(result);
                }
            }
            _ => {}
        }
        true
    }
    /// Asks Claude for a write-up: one answer from the pull request on its
    /// standard input, with no tools and none of your Claude settings.
    /// Returns the answer and the model.
    async fn write_with_claude(
        &self,
        input: &str,
        selection: &ModelSelection,
    ) -> Result<(Value, String), String> {
        let folder = self.root.join("writeups");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let launcher = self.claude_launcher.lock().unwrap().clone();
        let args = crate::claude::write_up_args(
            &selection.model,
            &selection.effort,
            &writeup::schema(),
            &writeup::instructions(),
        );
        let (_running, init, mut outputs) =
            crate::claude::start_write_up(&launcher, args, &folder, input.to_string())?;
        tokio::time::timeout(WRITE_TIMEOUT, async {
            let init = init
                .await
                .map_err(|_| "Claude stopped before it started.".to_string())??;
            while let Some(output) = outputs.recv().await {
                match output {
                    Output::Result {
                        ok: true,
                        structured: Some(answer),
                        ..
                    } => {
                        let model = Some(init.model).filter(|m| !m.is_empty());
                        return Ok((answer, model.unwrap_or_else(|| selection.model.clone())));
                    }
                    Output::Result { ok: true, .. } => {
                        return Err("Claude's answer wasn't in the shape asked for.".into())
                    }
                    Output::Result { text, .. } => {
                        return Err(format!("Claude said: {}", sentence(&text)))
                    }
                    Output::Exited(why) => {
                        return Err(format!("Claude stopped: {}", sentence(&why)))
                    }
                    _ => {}
                }
            }
            Err("Claude stopped without an answer.".into())
        })
        .await
        .map_err(|_| "Claude took too long.".to_string())?
    }
}
