//! Signing in to GitHub and Codex from Bit's setup help: Orbit starts the
//! sign-in, Bit shows the code to enter on the website, and Orbit checks
//! the connection again once you have.
use super::Engine;
use crate::{buddy, github, guide, rpc::Rpc};
use serde_json::{json, Value};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

/// How long a sign-in waits for you to enter its code, as long as the
/// code lasts.
pub(super) const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The Codex sign-in Bit started: its id, where to say how it ended, and
/// the connection it started on.
pub(super) struct Waiting {
    pub(super) login: String,
    pub(super) finished: tokio::sync::oneshot::Sender<Result<(), String>>,
    pub(super) rpc: std::sync::Weak<Rpc>,
}
/// How long Orbit waits for GitHub's code.
const CODE_TIMEOUT: Duration = Duration::from_secs(30);

/// The code and page in what `gh auth login --web` says, once it has said
/// both. gh words the code one of two ways, depending on whether it also
/// copied it to the clipboard, which its own settings decide.
pub fn github_prompt(text: &str) -> Option<(String, String)> {
    // Lowercasing ASCII keeps every byte where it was.
    let lower = text.to_ascii_lowercase();
    let at = |marker: &str| lower.find(marker).map(|at| at + marker.len());
    let word = |from: usize| text[from..].split_whitespace().next();
    let code = match at("one-time code:") {
        Some(from) => word(from)?,
        None => {
            let from = at("one-time code (")?;
            &text[from..from + text[from..].find(')')?]
        }
    };
    let page = word(at("web browser:")?)?;
    let valid = !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    (valid && guide::may_open(page)).then(|| (code.to_owned(), page.to_owned()))
}

impl Engine {
    /// Starts signing in to `service` (`github` or `codex`), unless a
    /// sign-in to it is waiting for you already.
    pub(super) async fn sign_in(self: &Arc<Self>, service: &str) -> Result<(), String> {
        let service: &'static str = match service {
            "github" => "github",
            "codex" => "codex",
            _ => return Err("Orbit can only sign in to GitHub and Codex.".into()),
        };
        if !self.signing_in.lock().unwrap().insert(service) {
            return Err(
                "That sign-in is waiting for you already: enter the code I gave you.".into(),
            );
        }
        let started = match service {
            "github" => self.sign_in_github().await,
            _ => self.sign_in_codex().await,
        };
        if started.is_err() {
            self.signing_in.lock().unwrap().remove(service);
        }
        started
    }
    /// Runs `gh auth login` in the web flow, shows its code, and waits for
    /// it to finish in the background.
    async fn sign_in_github(self: &Arc<Self>) -> Result<(), String> {
        let mut child = Command::new(github::gh())
            .args([
                "auth",
                "login",
                "--web",
                "--hostname",
                "github.com",
                "--git-protocol",
                "https",
                "--skip-ssh-key",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Cannot start gh: {e}"))?;
        let mut lines = BufReader::new(child.stderr.take().ok_or("Missing gh output")?).lines();
        let mut said = String::new();
        let prompt = tokio::time::timeout(CODE_TIMEOUT, async {
            while let Ok(Some(line)) = lines.next_line().await {
                said.push_str(&line);
                said.push('\n');
                if let Some(prompt) = github_prompt(&said) {
                    return Some(prompt);
                }
            }
            None
        })
        .await;
        let Ok(Some((code, page))) = prompt else {
            let _ = child.kill().await;
            let said = said.trim();
            return Err(if said.is_empty() {
                "GitHub didn't give a code to sign in with.".into()
            } else {
                format!("GitHub didn't give a code to sign in with: {said}")
            });
        };
        let mut note = 0;
        self.update(|s| note = buddy::signing_in(s, "GitHub", &code, &page))
            .await;
        let _ = self.flush().await;
        let e = self.clone();
        tokio::spawn(async move {
            // The rest of what gh says, for when it fails.
            let rest = tokio::spawn(async move {
                let mut rest = String::new();
                while let Ok(Some(line)) = lines.next_line().await {
                    rest.push_str(&line);
                    rest.push('\n');
                }
                rest
            });
            let result = match tokio::time::timeout(SIGN_IN_TIMEOUT, child.wait()).await {
                Ok(Ok(status)) if status.success() => Ok(()),
                Ok(Ok(_)) => Err(rest
                    .await
                    .ok()
                    .filter(|said| !said.trim().is_empty())
                    .unwrap_or_else(|| "gh stopped before it finished.".into())),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => {
                    let _ = child.kill().await;
                    Err("The code expired.".into())
                }
            };
            let result = match result {
                Ok(()) => {
                    // As gh's own sign-in does, Git uses it for GitHub too.
                    let _ = github::run(
                        &github::gh(),
                        &["auth", "setup-git", "--hostname", "github.com"].map(String::from),
                        None,
                        None,
                    )
                    .await;
                    let _ = e.check_connections().await;
                    let s = e.state.lock().await;
                    match (&s.github_user, &s.github_error) {
                        (Some(_), None) => Ok(()),
                        (_, error) => Err(error
                            .clone()
                            .unwrap_or_else(|| "GitHub isn't answering yet.".into())),
                    }
                }
                Err(error) => Err(error),
            };
            e.signing_in.lock().unwrap().remove("github");
            e.update(|s| buddy::signed_in(s, "GitHub", Some(note), result))
                .await;
            let _ = e.flush().await;
        });
        Ok(())
    }
    /// Asks Codex for a code to sign in with. Codex says when the sign-in
    /// finishes (`codex_signed_in`).
    async fn sign_in_codex(self: &Arc<Self>) -> Result<(), String> {
        if self.connect_signed_in().await.is_ok() {
            self.signing_in.lock().unwrap().remove("codex");
            self.update(|s| buddy::signed_in(s, "Codex", None, Ok(())))
                .await;
            let _ = self.flush().await;
            return Ok(());
        }
        let rpc = self.client().await.map_err(|_| {
            "Codex isn't running, so it can't sign in. Install the Codex CLI first.".to_string()
        })?;
        let started = rpc
            .request("account/login/start", json!({"type": "chatgptDeviceCode"}))
            .await?;
        let text = |key: &str| started[key].as_str().map(str::to_owned);
        let (Some(login), Some(code), Some(page)) =
            (text("loginId"), text("userCode"), text("verificationUrl"))
        else {
            return Err("Codex didn't give a code to sign in with.".into());
        };
        if !guide::may_open(&page) {
            let _ = rpc
                .request("account/login/cancel", json!({"loginId": login}))
                .await;
            return Err(format!(
                "Codex gave a sign-in page Orbit doesn't know: {page}"
            ));
        }
        let (finished, ended) = tokio::sync::oneshot::channel();
        *self.codex_login.lock().unwrap() = Some(Waiting {
            login: login.clone(),
            finished,
            rpc: Arc::downgrade(&rpc),
        });
        let mut note = 0;
        self.update(|s| note = buddy::signing_in(s, "Codex", &code, &page))
            .await;
        let _ = self.flush().await;
        tokio::spawn(self.clone().await_codex_sign_in(login, note, ended));
        Ok(())
    }
    /// Waits for the Codex sign-in `login` to finish, for as long as its
    /// code lasts, then says how it went.
    pub(super) async fn await_codex_sign_in(
        self: Arc<Self>,
        login: String,
        note: u64,
        ended: tokio::sync::oneshot::Receiver<Result<(), String>>,
    ) {
        let result = match tokio::time::timeout(SIGN_IN_TIMEOUT, ended).await {
            Ok(Ok(Ok(()))) => self.connect_signed_in().await,
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err("Codex stopped before the sign-in finished.".into()),
            Err(_) => {
                {
                    let mut waiting = self.codex_login.lock().unwrap();
                    if waiting.as_ref().is_some_and(|w| w.login == login) {
                        waiting.take();
                    }
                }
                if let Ok(rpc) = self.client().await {
                    let _ = rpc
                        .request("account/login/cancel", json!({"loginId": login}))
                        .await;
                }
                Err("The code expired.".into())
            }
        };
        self.signing_in.lock().unwrap().remove("codex");
        self.update(|s| buddy::signed_in(s, "Codex", Some(note), result))
            .await;
        let _ = self.flush().await;
    }
    /// Ends the Codex sign-in waiting on `rpc`, which has gone away, so a
    /// new one can start as soon as Codex is back.
    pub(super) fn codex_login_lost(&self, rpc: Option<&Arc<Rpc>>) {
        let mut waiting = self.codex_login.lock().unwrap();
        let lost = waiting.as_ref().is_some_and(|w| match w.rpc.upgrade() {
            Some(started) => rpc.is_some_and(|rpc| Arc::ptr_eq(rpc, &started)),
            None => true,
        });
        if lost {
            // Dropping its sender tells the waiting task.
            waiting.take();
        }
    }
    /// Connects to Codex again, fine once it's signed in, even when
    /// something after that, such as reading its models, didn't work.
    async fn connect_signed_in(self: &Arc<Self>) -> Result<(), String> {
        let connected = self.connect().await;
        if self.state.lock().await.codex_ready {
            Ok(())
        } else {
            connected
        }
    }
    /// Codex says a sign-in finished: when it's the one Bit started, its
    /// task connects again and Bit says how it went. Connecting here would
    /// wait on the events this is one of.
    pub(super) fn codex_signed_in(&self, p: &Value) {
        let mine = {
            let mut waiting = self.codex_login.lock().unwrap();
            // Codex may leave the id out; the one waiting is Bit's.
            match (waiting.as_ref(), p["loginId"].as_str()) {
                (Some(w), Some(id)) if w.login != id => None,
                _ => waiting.take(),
            }
        };
        if let Some(Waiting { finished, .. }) = mine {
            let _ = finished.send(if p["success"] == true {
                Ok(())
            } else {
                Err(p["error"]
                    .as_str()
                    .unwrap_or("Codex said the sign-in didn't work.")
                    .to_string())
            });
        }
    }
}
