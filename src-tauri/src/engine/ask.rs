//! Questions you type to Bit, answered by the answerer you chose: Codex in
//! a thread of its own (read-only, in an empty folder, with no network, no
//! approvals and none of Codex's other tools), Claude Code with no tools at
//! all, Antigravity in its sandbox, or Ollama or a provider's API with your
//! key, which only ever write an answer. The answer's buttons run only when
//! you tap them.
use super::{extras, publish::By, Engine};
use crate::{
    antigravity,
    api::{self, Api},
    ask::{self, Action},
    buddy::{self, Answerer},
    claude,
    cli::{Init, Output, Running, Turn, TurnKind},
    guide,
    text::sentence,
    types::*,
};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

/// How long Bit waits for an answer.
pub(super) const ANSWER_TIMEOUT: Duration = Duration::from_secs(180);

/// The question being answered.
pub(super) struct Asking {
    /// The question's note.
    note: u64,
    /// Who answers it.
    by: Answerer,
    /// Claude Code or Antigravity answering it; dropping it stops them.
    running: Option<Running>,
    /// Ollama's or an API's request; dropping it stops the request.
    request: Option<Request>,
    thread: Option<String>,
    turn: Option<String>,
    /// Codex's latest message, its answer once the turn completes.
    text: String,
    /// `Engine::memory_generation` when the question was asked, before its
    /// summary was made.
    memory_generation: u64,
    /// The repositories the question's summary offered to connect.
    connectable: Vec<String>,
}

/// A request for an answer; dropping it stops the request.
struct Request(tokio::task::AbortHandle);
impl Drop for Request {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Engine {
    /// Asks the answerer you chose a question you typed. The question
    /// shows straight away and its answer follows in the same note. When
    /// the one you chose isn't ready, Bit switches to the one sign-in that
    /// is; with none, Bit's answer is how to connect one.
    pub async fn ask_buddy_anything(self: &Arc<Self>, question: &str) -> Result<(), String> {
        let question = question.trim();
        if question.is_empty() || question.chars().count() > ask::MAX_QUESTION {
            return Err(format!(
                "Ask a question of 1 to {} characters.",
                ask::MAX_QUESTION
            ));
        }
        self.pick_bit_answerer().await;
        let (note, by, selection, ollama_url, connected) = {
            let mut s = self.state.lock().await;
            let name = s.settings.buddy.name.clone();
            if !s.settings.buddy.enabled {
                return Err(format!("Turn {name} on in Settings first."));
            }
            if self.asking.lock().unwrap().is_some() {
                return Err(format!("{name} is still answering your last question."));
            }
            let by = s.settings.buddy.answered_by();
            // Nothing to answer with yet, so Bit's answer is how to
            // connect something.
            if guide::bit_ready(&s, by).is_err() {
                buddy::cant_answer(&mut s, question);
                self.dirty.store(true, Ordering::SeqCst);
                drop(s);
                let _ = self.flush().await;
                return Ok(());
            }
            let selection = guide::bit_selection(&s, by);
            let note = buddy::asked(&mut s, question);
            *self.asking.lock().unwrap() = Some(Asking {
                note,
                by,
                running: None,
                request: None,
                thread: None,
                turn: None,
                text: String::new(),
                memory_generation: self.memory_generation.load(Ordering::SeqCst),
                connectable: vec![],
            });
            self.dirty.store(true, Ordering::SeqCst);
            (
                note,
                by,
                selection,
                s.settings.buddy.ollama_url.clone(),
                s.settings.repositories.clone(),
            )
        };
        let _ = self.flush().await;
        // The question shows while Bit looks up your repositories.
        let mut lookups = self.look_up(connected, true).await;
        // What the Decisions page knows, so Bit can answer about it.
        lookups.decisions = self.decisions().await.map(|d| d.prs).unwrap_or_default();
        let names = lookups.connectable_names();
        if self.asking_mut(note, |a| a.connectable = names).is_none() {
            return Ok(());
        }
        let (prompt, instructions) = {
            let s = self.state.lock().await;
            (
                ask::prompt(&s, question, &lookups),
                ask::instructions(&s.settings.buddy),
            )
        };
        let started = match (by, by.provider()) {
            (_, Some(Provider::Codex)) => {
                self.start_answer(note, &prompt, &instructions, &selection)
                    .await
            }
            (_, Some(p)) => self.start_cli_answer(note, p, &prompt, &instructions, &selection),
            (Answerer::Api(api), None) => {
                let setup = self.apis.lock().unwrap().clone();
                self.start_request(note, async move {
                    let key = setup.key(api).await?.ok_or_else(|| {
                        format!(
                            "Your {} API key isn't in your password store any more. Save it again in Settings.",
                            api.company()
                        )
                    })?;
                    api::answer(
                        api,
                        setup.base(api),
                        &key,
                        &selection.model,
                        &instructions,
                        &prompt,
                        &ask::output_schema(),
                        ANSWER_TIMEOUT,
                    )
                    .await
                })
            }
            _ => self.start_request(note, async move {
                crate::ollama::answer(
                    &ollama_url,
                    &selection.model,
                    &instructions,
                    &prompt,
                    &ask::output_schema(),
                    ANSWER_TIMEOUT,
                )
                .await
            }),
        };
        if let Err(error) = started {
            self.answer_done(note, Err(error)).await;
            return Ok(());
        }
        let e = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(ANSWER_TIMEOUT).await;
            e.answer_done(note, Err(format!("{} took too long.", by.label())))
                .await;
        });
        Ok(())
    }
    /// Starts Codex on the question, in a new thread that can't change
    /// anything: read-only, in an empty folder of Orbit's, with no network,
    /// no approvals, and none of Codex's other tools. Codex's tool servers
    /// stay off for the thread; if Codex reports one on, or can't say, the
    /// question isn't asked.
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
        let codex_config = extras::codex_config(&rpc, &folder).await?;
        let thread = rpc
            .request(
                "thread/start",
                json!({
                    "cwd": folder,
                    "approvalPolicy": "never",
                    "sandbox": "read-only",
                    "model": selection.model,
                    "config": ask::thread_config(&selection.effort, &codex_config),
                    "developerInstructions": instructions,
                }),
            )
            .await?;
        let thread_id = thread["thread"]["id"]
            .as_str()
            .ok_or("Codex didn't start a thread.")?
            .to_string();
        let claimed = match selection.verify_response(&thread) {
            Ok(()) => self.tools_off(&rpc, &thread_id).await,
            Err(error) => Err(error),
        }
        .and_then(|()| {
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
    /// Starts Claude Code or Antigravity on the question, in an empty folder
    /// of Orbit's. Claude Code runs with no tools, no settings files and no
    /// tool servers, in Codex's folder. Antigravity can't turn its tools
    /// off, so it runs in its sandbox, with no network, and only with
    /// settings under which it asks before anything that needs approval,
    /// which nobody gives; its folder is emptied first, so nothing it wrote
    /// for one question reaches the next.
    fn start_cli_answer(
        self: &Arc<Self>,
        note: u64,
        by: Provider,
        prompt: &str,
        instructions: &str,
        selection: &ModelSelection,
    ) -> Result<(), String> {
        let folder = match by {
            Provider::Antigravity => {
                let folder = self.root.join("bit-antigravity");
                if folder.exists() {
                    std::fs::remove_dir_all(&folder).map_err(|e| e.to_string())?;
                }
                folder
            }
            _ => self.root.join("bit"),
        };
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let (running, init, outputs) = match by {
            Provider::Antigravity => {
                let setup = self.antigravity.lock().unwrap().clone();
                // Antigravity has no separate instructions, so they lead
                // the prompt.
                let turn = Turn {
                    kind: TurnKind::Extraction,
                    cwd: folder,
                    session: String::new(),
                    resume: false,
                    model: selection.model.clone(),
                    effort: selection.effort.clone(),
                    prompt: format!("{instructions}\n\n{prompt}"),
                    schema: Some(ask::output_schema()),
                    tool: None,
                };
                antigravity::start(&setup, &turn)?
            }
            _ => {
                let launcher = self.claude_launcher.lock().unwrap().clone();
                let args = claude::write_up_args(
                    &selection.model,
                    &selection.effort,
                    &ask::output_schema(),
                    instructions,
                );
                claude::start_write_up(&launcher, args, &folder, prompt.to_string())?
            }
        };
        // A question that was dropped meanwhile drops its process too.
        self.asking_mut(note, |a| a.running = Some(running))
            .ok_or("The question was dropped.")?;
        let e = self.clone();
        let model = selection.model.clone();
        tokio::spawn(async move {
            let reply = cli_answer(by, &model, init, outputs).await;
            e.answer_done(note, reply).await;
        });
        Ok(())
    }
    /// Lists Ollama's models at Bit's address while Ollama answers Bit,
    /// recording whether it can.
    pub async fn check_ollama(&self) {
        let asked = |s: &Snapshot| {
            let buddy = &s.settings.buddy;
            (buddy.answered_by() == Answerer::Ollama).then(|| buddy.ollama_url.clone())
        };
        let url = asked(&*self.state.lock().await);
        let result = match &url {
            Some(url) => Some(crate::ollama::models(url).await.and_then(|models| {
                if models.is_empty() {
                    Err(format!("Ollama at {url} has no models yet. Pull one there, such as with ollama pull, then check again."))
                } else {
                    Ok(models)
                }
            })),
            None => None,
        };
        self.update(|s| {
            // A change since the check started gets a check of its own.
            if asked(s) != url {
                return;
            }
            s.ollama_models = match &result {
                Some(Ok(models)) => models.clone(),
                _ => vec![],
            };
            s.ollama = match result {
                Some(result) => status(&result),
                None => CliStatus::default(),
            };
        })
        .await;
        let _ = self.flush().await;
    }
    /// Checks whoever answers Bit and isn't checked with the reviewers:
    /// Ollama, or your key for a provider's API.
    pub async fn check_bit_answerer(&self) {
        self.check_password_store().await;
        self.check_ollama().await;
        let answerer = self.state.lock().await.settings.buddy.answered_by();
        if let Answerer::Api(api) = answerer {
            let _turn = self.api_lock.lock().await;
            self.check_api(api).await;
        }
    }
    /// Notes whether this computer has a password store to keep API keys
    /// in, so the app and Bit can say so before you paste one.
    async fn check_password_store(&self) {
        let setup = self.apis.lock().unwrap().clone();
        let missing = setup.missing().await;
        if self.state.lock().await.password_store != missing {
            self.update_live(|s| s.password_store = missing).await;
        }
    }
    /// Saves your key for a provider's API in your system's password store,
    /// or removes it when `key` is None, then checks it.
    pub async fn set_api_key(&self, provider: &str, key: Option<String>) -> Result<(), String> {
        let api = Api::from_key(provider)
            .ok_or_else(|| format!("Orbit doesn't use API keys for '{provider}'."))?;
        let key = key.map(|k| k.trim().to_string());
        if let Some(key) = &key {
            api::validate_key(key)?;
        }
        let _turn = self.api_lock.lock().await;
        let setup = self.apis.lock().unwrap().clone();
        if key.is_some() && setup.missing().await.is_some() {
            self.check_password_store().await;
            return Err(api::NO_STORE.into());
        }
        setup.save(api, key.clone()).await?;
        self.update(|s| {
            *s.api_keys.get_mut(api) = ApiKey {
                saved: key.is_some(),
                ..Default::default()
            }
        })
        .await;
        self.check_api(api).await;
        Ok(())
    }
    /// Lists the models your saved key for `api` can use, recording whether
    /// it works. Only called holding `api_lock`, so checks and saves don't
    /// cross.
    async fn check_api(&self, api: Api) {
        let saved = self.state.lock().await.api_keys.get(api).saved;
        let setup = self.apis.lock().unwrap().clone();
        let result = match saved {
            false => None,
            true => match setup.key(api).await {
                Ok(None) => None,
                Ok(Some(key)) => Some(api::models(api, setup.base(api), &key).await.and_then(
                    |models| {
                        if models.is_empty() {
                            Err(format!(
                                "{} didn't offer your key any models that can answer Bit.",
                                api.company()
                            ))
                        } else {
                            Ok(models)
                        }
                    },
                )),
                Err(e) => Some(Err(e)),
            },
        };
        self.update(|s| {
            let key = s.api_keys.get_mut(api);
            *key = match &result {
                // Not saved, or gone from your password store.
                None => ApiKey::default(),
                Some(result) => ApiKey {
                    saved: true,
                    status: status(result),
                    models: result.clone().unwrap_or_default(),
                },
            };
        })
        .await;
        let _ = self.flush().await;
    }
    /// Asks Ollama or a provider's API, which only ever write an answer:
    /// `asking` sends the question with what Orbit knows, and nothing more.
    fn start_request(
        self: &Arc<Self>,
        note: u64,
        asking: impl std::future::Future<Output = Result<String, String>> + Send + 'static,
    ) -> Result<(), String> {
        let asked = tokio::spawn(asking);
        // A question that was dropped meanwhile drops its request too.
        let request = Request(asked.abort_handle());
        self.asking_mut(note, |a| a.request = Some(request))
            .ok_or("The question was dropped.")?;
        let e = self.clone();
        tokio::spawn(async move {
            // Stopped when the question ended without it.
            if let Ok(reply) = asked.await {
                e.answer_done(note, reply).await;
            }
        });
        Ok(())
    }
    /// Checks with Codex that no tool server is on for `thread`.
    async fn tools_off(&self, rpc: &crate::rpc::Rpc, thread: &str) -> Result<(), String> {
        extras::check(rpc, thread, None)
            .await
            .map_err(|left| match left {
                extras::Left::Unsure(e) => format!(
                    "I couldn't check that Codex's other tools are off: {}",
                    sentence(&e)
                ),
                extras::Left::On(names) => format!(
                    "Codex kept these tools on for my question, so I didn't ask it: {}.",
                    names.join(", ")
                ),
                left @ extras::Left::Missing(_) => left.message("I didn't ask it"),
            })
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
                        Some(message) => format!("Codex said: {}", sentence(message)),
                        None => "Codex couldn't finish.".into(),
                    }),
                };
                self.answer_done(note, result).await;
            }
            _ => {}
        }
    }
    /// Ends question `note` with the reply or why there is none, and puts
    /// its Codex thread away or stops its process. Only the first ending
    /// counts.
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
        let connectable = &asking.connectable;
        self.update(
            |s| match reply.and_then(|r| ask::answer(s, &r, connectable)) {
                Ok(a) => {
                    // Something you forgot since the question was asked stays
                    // forgotten: the answer's memory changes are dropped.
                    let fresh =
                        asking.memory_generation == self.memory_generation.load(Ordering::SeqCst);
                    let memory = if fresh { a.memory } else { vec![] };
                    buddy::answered(s, note, a.text, a.proposals, memory)
                }
                Err(error) => buddy::not_answered(s, note, &error),
            },
        )
        .await;
        let _ = self.flush().await;
    }
    /// Ends the question Codex is answering, if any, without an answer.
    pub(super) async fn end_codex_question(self: &Arc<Self>, why: &str) {
        let note = self
            .asking
            .lock()
            .unwrap()
            .as_ref()
            .filter(|a| a.by == Answerer::Codex)
            .map(|a| a.note);
        if let Some(note) = note {
            self.answer_done(note, Err(why.into())).await;
        }
    }
    /// Forgets a line of Bit's memory: one about `repo`, or a general one
    /// when it is none. An answer to a question asked before can't bring it
    /// back (`memory_generation`).
    pub async fn forget_memory(&self, repo: Option<&str>, line: &str) -> Result<(), String> {
        let mut found = false;
        self.update(|s| {
            found = buddy::forget(s, repo, line);
            if found {
                self.memory_generation.fetch_add(1, Ordering::SeqCst);
            }
        })
        .await;
        if !found {
            return Err("That line isn't there any more.".into());
        }
        let _ = self.flush().await;
        Ok(())
    }
    /// Takes the morning brief for `day` out of Needs you, as you read it.
    /// A brief already out of it, or a newer one, is left as it is.
    pub async fn close_brief(&self, day: &str) -> Result<(), String> {
        let mut closed = false;
        self.update(|s| closed = crate::brief::close(s, day)).await;
        if closed {
            let _ = self.flush().await;
        }
        Ok(())
    }
    /// Forgets everything Bit remembers.
    pub async fn clear_memory(&self) -> Result<(), String> {
        self.update(|s| {
            s.buddy_feed.memory = Default::default();
            self.memory_generation.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        let _ = self.flush().await;
        Ok(())
    }
    /// Uses a button from an answer, after checking it against Orbit's
    /// state now, as the same control elsewhere in the app does. A button
    /// is claimed before it runs, so a tap from another window while it
    /// runs is refused; it counts as done only once it worked, and one
    /// that failed can be tapped again.
    pub async fn use_proposal(self: &Arc<Self>, note: u64, proposal: u32) -> Result<(), String> {
        let (p, _claim) = {
            let s = self.state.lock().await;
            let p = buddy::proposal(&s, note, proposal)
                .cloned()
                .ok_or("This button isn't there any more.")?;
            if p.done {
                return Err("That's done already.".into());
            }
            if !self.using.lock().unwrap().insert((note, proposal)) {
                return Err("That's already running.".into());
            }
            (p, Using(self, (note, proposal)))
        };
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
                let settings = self.state.lock().await.settings.clone();
                let patch = p
                    .settings
                    .as_ref()
                    .and_then(|v| crate::ask::complete_patch(&settings, v))
                    .ok_or("This settings change isn't valid.")?;
                if patch.trusted_authors.is_some() {
                    return Err("Change trusted accounts in Settings yourself.".into());
                }
                self.update_settings(patch).await.map(|_| ())
            }
            Action::SetUpRepository => {
                let name = p
                    .repo
                    .as_deref()
                    .ok_or("This button isn't about a repository.")?;
                let patch = p.repository.clone().unwrap_or_default();
                self.set_up_repository(name, &patch, p.connect).await
            }
            Action::CheckConnections => self.check_connections().await,
            // It isn't done until you've entered the code, and Bit says
            // when it is.
            Action::SignIn => {
                let service = p.service.as_deref().ok_or("This button isn't a sign-in.")?;
                return self.sign_in(service).await;
            }
            Action::OpenPage => return Err("The app opens this page itself.".into()),
        }?;
        // Done before the claim ends, so no tap gets in between.
        self.update(|s| buddy::proposal_done(s, note, proposal))
            .await;
        let _ = self.flush().await;
        Ok(())
    }
}

/// A button being used. Dropping it ends the claim, however its use ended.
struct Using<'a>(&'a Engine, (u64, u32));
impl Drop for Using<'_> {
    fn drop(&mut self) {
        self.0
            .using
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.1);
    }
}

/// Claude Code's or Antigravity's answer, as the text Codex's would be.
/// As for reviews, it must say it runs `model`, and Antigravity that it
/// asks before anything that needs approval.
async fn cli_answer(
    by: Provider,
    model: &str,
    init: tokio::sync::oneshot::Receiver<Result<Init, String>>,
    mut outputs: tokio::sync::mpsc::Receiver<Output>,
) -> Result<String, String> {
    let name = by.label();
    let init = init
        .await
        .map_err(|_| format!("{name} stopped before it started."))??;
    if init.model != model {
        return Err(format!(
            "{name} didn't confirm the model '{model}' (reported: {}), so I stopped it.",
            if init.model.is_empty() {
                "none"
            } else {
                &init.model
            }
        ));
    }
    if by == Provider::Antigravity {
        let mode = init.permission_mode.as_deref().unwrap_or("");
        if !antigravity::ASKING_MODES.contains(&mode) {
            return Err(format!(
                "Antigravity would run without asking for approval (permission mode: {}), so I stopped it. Orbit never changes Antigravity's settings: set its tool permission back to request review.",
                if mode.is_empty() { "not reported" } else { mode }
            ));
        }
    }
    while let Some(output) = outputs.recv().await {
        match output {
            Output::Result {
                ok: true,
                structured: Some(answer),
                ..
            } => return Ok(answer.to_string()),
            Output::Result { ok: true, .. } => {
                return Err(format!("{name}'s answer wasn't in the shape asked for."))
            }
            Output::Result { text, .. } => return Err(format!("{name} said: {}", sentence(&text))),
            Output::Exited(why) => return Err(format!("{name} stopped: {}", sentence(&why))),
            _ => {}
        }
    }
    Err(format!("{name} stopped without an answer."))
}

/// A check's result as a connection's status.
fn status<T>(result: &Result<T, String>) -> CliStatus {
    super::cli_review::status(result, |_| None)
}
