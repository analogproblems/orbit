//! Runs the `claude` CLI in print mode, one process per turn, and reads its
//! `stream-json` output. Orbit never reads or stores Claude credentials: the
//! CLI uses the sign-in of whoever runs Orbit.
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
};

/// The tools a review may use. None of them edits files.
const REVIEW_TOOLS: &str = "Bash,Read,Grep,Glob,Task";
/// Commands that run without asking, always inside Claude's sandbox. They
/// only read the repository, or filter what a git command printed.
const ALLOWED: [&str; 14] = [
    "Read",
    "Grep",
    "Glob",
    "Task",
    "Bash(git diff:*)",
    "Bash(git log:*)",
    "Bash(git show:*)",
    "Bash(git status:*)",
    "Bash(git blame:*)",
    "Bash(git merge-base:*)",
    "Bash(head:*)",
    "Bash(tail:*)",
    "Bash(grep:*)",
    "Bash(wc:*)",
];
/// Options of the allowed git commands that write files or run programs. Git
/// can still run a diff driver the clone configures, which is why reviews
/// never run without the sandbox.
const DENIED: [&str; 3] = [
    "Bash(*--output*)",
    "Bash(*--ext-diff*)",
    "Bash(*--textconv*)",
];
/// Lines of standard error kept to explain a failed run.
const STDERR_LINES: usize = 20;

/// How to start `claude`; tests start a fake.
#[derive(Clone, Debug)]
pub struct Launcher {
    pub program: PathBuf,
    pub prefix: Vec<String>,
}
impl Default for Launcher {
    fn default() -> Self {
        Self {
            program: "claude".into(),
            prefix: vec![],
        }
    }
}
impl Launcher {
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.prefix);
        cmd
    }
    /// Runs a short command, such as `--version`, and returns its output.
    async fn output(&self, args: &[&str]) -> Result<String, String> {
        let mut cmd = self.command();
        cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(30), cmd.output())
            .await
            .map_err(|_| format!("claude {} timed out.", args.join(" ")))?
            .map_err(|e| format!("Cannot start Claude. Install Claude Code, then run claude once to sign in. {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "claude {} failed: {}",
                args.join(" "),
                stderr.trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
    /// The CLI's version, after checking it is signed in.
    pub async fn check(&self) -> Result<String, String> {
        let version = self.output(&["--version"]).await?;
        let status = self.output(&["auth", "status"]).await?;
        let status: Value = serde_json::from_str(&status)
            .map_err(|_| "Claude reported its sign-in in an unexpected format.".to_string())?;
        if status["loggedIn"] != true {
            return Err("Claude is not signed in. Run claude once to sign in, then check again in Settings.".into());
        }
        Ok(version
            .split_whitespace()
            .next()
            .unwrap_or(&version)
            .to_string())
    }
}

/// What a turn is for, which decides its tools and how its output is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnKind {
    /// The review itself; its result is the report.
    Review,
    /// Restating the review as findings data.
    Extraction,
    /// Restating the findings and classifying the previous round's, which
    /// may need a look at the code.
    ExtractionWithTools,
    /// A developer's question.
    FollowUp,
}

#[derive(Clone, Debug)]
pub struct Turn {
    pub kind: TurnKind,
    pub cwd: PathBuf,
    /// The session, created by the first turn and resumed by later ones.
    pub session: String,
    pub resume: bool,
    pub model: String,
    pub effort: String,
    pub prompt: String,
    pub schema: Option<Value>,
}
impl Turn {
    pub fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "-p".into(),
            self.prompt.clone(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--model".into(),
            self.model.clone(),
            "--effort".into(),
            self.effort.clone(),
            // Anything that would ask is refused; nobody is there to answer.
            "--permission-mode".into(),
            "dontAsk".into(),
            // No settings file loads: not the pull request's own, whose hooks
            // and MCP servers would run its code, and not the developer's,
            // whose permission rules were written for their own work.
            "--setting-sources".into(),
            "".into(),
            "--strict-mcp-config".into(),
            "--settings".into(),
            // Without the sandbox, an allowed `git diff` could run a program
            // the pull request controls (a configured textconv driver
            // pointing at a script in the repository), so Claude must refuse
            // to start rather than fall back to running commands unsandboxed.
            json!({"sandbox": {
                "enabled": true,
                "failIfUnavailable": true,
                "autoAllowBashIfSandboxed": true,
                "allowUnsandboxedCommands": false,
            }})
            .to_string(),
        ];
        args.push(
            if self.resume {
                "--resume"
            } else {
                "--session-id"
            }
            .into(),
        );
        args.push(self.session.clone());
        args.push("--tools".into());
        args.push(
            match self.kind {
                TurnKind::Extraction => "",
                _ => REVIEW_TOOLS,
            }
            .into(),
        );
        if self.kind != TurnKind::Extraction {
            args.push("--allowedTools".into());
            args.extend(ALLOWED.iter().map(|s| s.to_string()));
            args.push("--disallowedTools".into());
            args.extend(DENIED.iter().map(|s| s.to_string()));
        }
        if let Some(schema) = &self.schema {
            args.push("--json-schema".into());
            args.push(schema.to_string());
        }
        args
    }
}

/// What one turn's process reported, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// A finished message from the reviewer itself (not a sub-agent).
    Text {
        item: String,
        text: String,
    },
    CommandStarted {
        tool: String,
        command: String,
    },
    CommandOutput(String),
    /// The turn finished. `text` is its final answer, `structured` the
    /// answer to a schema, and `denials` the tool calls that were refused.
    Result {
        ok: bool,
        text: String,
        structured: Option<Value>,
        denials: Vec<String>,
    },
    /// The process ended without a result.
    Exited(String),
}

/// The session's start, as Claude reported it.
#[derive(Clone, Debug, PartialEq)]
pub struct Init {
    pub model: String,
}

/// A running turn. Dropping it kills the process.
pub struct Running {
    stop: Mutex<Option<oneshot::Sender<()>>>,
}
impl Running {
    /// Kills the process and the commands it started.
    pub async fn kill(&self) {
        if let Some(stop) = self.stop.lock().unwrap().take() {
            let _ = stop.send(());
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.get_mut().unwrap().take() {
            let _ = stop.send(());
        }
    }
}
/// Owns the process until it exits or is told to stop, so the process
/// group is only signalled while the process has not been reaped.
async fn wait(mut child: Child, stop: oneshot::Receiver<()>) -> String {
    let status = tokio::select! {
        status = child.wait() => status,
        _ = stop => {
            #[cfg(unix)]
            if let Some(id) = child.id() {
                unsafe {
                    libc::kill(-(id as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
            child.wait().await
        }
    };
    match status {
        Ok(s) => format!("it exited with {s}"),
        Err(e) => e.to_string(),
    }
}

/// A started turn: its process, the session's start, and the rest.
pub type Started = (
    Running,
    oneshot::Receiver<Result<Init, String>>,
    mpsc::Receiver<Output>,
);

/// Starts a turn. The first value from `init` is the session's start, or
/// the reason it never started; the rest of the turn arrives on the channel.
pub fn start(launcher: &Launcher, turn: &Turn) -> Result<Started, String> {
    let mut cmd = launcher.command();
    cmd.args(turn.args())
        .current_dir(&turn.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| {
        format!("Cannot start Claude. Install Claude Code, then run claude once to sign in. {e}")
    })?;
    let stdout = child.stdout.take().ok_or("Missing Claude stdout")?;
    let stderr = child.stderr.take().ok_or("Missing Claude stderr")?;
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let no_sandbox = Arc::new(Mutex::new(None::<String>));
    let (init_tx, init_rx) = oneshot::channel();
    let (tx, rx) = mpsc::channel(256);
    let (stop, stopped) = oneshot::channel();
    let exited = tokio::spawn(wait(child, stopped));
    {
        let tail = tail.clone();
        let no_sandbox = no_sandbox.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line
                    .to_lowercase()
                    .contains("sandbox required but unavailable")
                {
                    no_sandbox.lock().unwrap().get_or_insert(line.clone());
                }
                let mut tail = tail.lock().unwrap();
                tail.push_back(line);
                if tail.len() > STDERR_LINES {
                    tail.pop_front();
                }
            }
        });
    }
    {
        tokio::spawn(async move {
            let mut init = Some(init_tx);
            let mut reader = Reader::default();
            let mut lines = BufReader::new(stdout).lines();
            let mut finished = false;
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if message["type"] == "system" && message["subtype"] == "init" {
                    if let Some(init) = init.take() {
                        let model = message["model"].as_str().unwrap_or("").to_string();
                        let _ = init.send(Ok(Init { model }));
                    }
                    continue;
                }
                for output in reader.read(&message) {
                    finished |= matches!(output, Output::Result { .. });
                    if tx.send(output).await.is_err() {
                        return;
                    }
                }
            }
            let status = exited
                .await
                .unwrap_or_else(|e| format!("Orbit lost track of it: {e}"));
            // Standard error is read to its end once the process is gone.
            tokio::time::sleep(Duration::from_millis(50)).await;
            let detail = {
                let tail = tail.lock().unwrap();
                tail.iter()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .cloned()
                    .unwrap_or(status)
            };
            let reason = match no_sandbox.lock().unwrap().take() {
                Some(line) => format!(
                    "Claude reviews only run inside Claude's sandbox, and it can't start on this computer, so the review didn't run. Install what Claude names, or review this repository with Codex. Claude said: {}",
                    line.trim().trim_start_matches("Error: ")
                ),
                None => format!("Claude stopped before finishing: {detail}"),
            };
            if let Some(init) = init.take() {
                let _ = init.send(Err(reason.clone()));
            }
            if !finished {
                let _ = tx.send(Output::Exited(reason)).await;
            }
        });
    }
    Ok((
        Running {
            stop: Mutex::new(Some(stop)),
        },
        init_rx,
        rx,
    ))
}

/// Turns `stream-json` messages into outputs, remembering which tool each
/// call used so only command output is shown.
#[derive(Default)]
struct Reader {
    tools: HashMap<String, String>,
}
impl Reader {
    fn read(&mut self, message: &Value) -> Vec<Output> {
        let mut out = vec![];
        // Sub-agents' own words are working notes; their tool calls are shown.
        let top_level = message["parent_tool_use_id"].is_null();
        match message["type"].as_str().unwrap_or("") {
            "assistant" => {
                let id = message["message"]["id"].as_str().unwrap_or("message");
                for (index, block) in content(message).iter().enumerate() {
                    match block["type"].as_str().unwrap_or("") {
                        "text" if top_level => {
                            let text = block["text"].as_str().unwrap_or("");
                            if !text.trim().is_empty() {
                                out.push(Output::Text {
                                    item: format!("{id}-{index}"),
                                    text: text.into(),
                                });
                            }
                        }
                        "tool_use" => {
                            let tool = block["name"].as_str().unwrap_or("tool").to_string();
                            if let Some(id) = block["id"].as_str() {
                                self.tools.insert(id.into(), tool.clone());
                            }
                            if let Some(command) = describe(&tool, &block["input"]) {
                                out.push(Output::CommandStarted { tool, command });
                            }
                        }
                        _ => {}
                    }
                }
            }
            "user" => {
                for block in content(message) {
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    if block["type"] == "tool_result"
                        && self.tools.get(id).map(String::as_str) == Some("Bash")
                    {
                        out.push(Output::CommandOutput(result_text(&block["content"])));
                    }
                }
            }
            "result" => {
                let denials = message["permission_denials"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|d| {
                        let tool = d["tool_name"].as_str().unwrap_or("tool");
                        describe(tool, &d["tool_input"]).unwrap_or_else(|| tool.to_string())
                    })
                    .collect();
                let text = match &message["result"] {
                    Value::String(text) => text.clone(),
                    _ => message["errors"]
                        .as_array()
                        .map(|e| {
                            e.iter()
                                .filter_map(|e| e.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default(),
                };
                let structured =
                    Some(message["structured_output"].clone()).filter(|v| !v.is_null());
                out.push(Output::Result {
                    ok: message["subtype"] == "success" && message["is_error"] != true,
                    text,
                    structured,
                    denials,
                });
            }
            _ => {}
        }
        out
    }
}
fn content(message: &Value) -> Vec<Value> {
    message["message"]["content"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}
/// A tool call in words, for the chat; None for calls not worth a line.
fn describe(tool: &str, input: &Value) -> Option<String> {
    match tool {
        "Bash" => input["command"].as_str().map(str::to_owned),
        "Read" => input["file_path"].as_str().map(|p| format!("read {p}")),
        "Grep" => input["pattern"].as_str().map(|p| format!("search for {p}")),
        "Glob" => input["pattern"].as_str().map(|p| format!("list {p}")),
        "Task" => input["description"]
            .as_str()
            .map(|d| format!("start a helper: {d}")),
        "StructuredOutput" => None,
        other => Some(other.to_string()),
    }
}
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Claude's review result as a Markdown report. At most efforts
/// `/code-review` answers with a JSON list of findings, sometimes in a code
/// fence; a plain-words answer is kept as it is.
pub fn report(result: &str) -> String {
    let trimmed = result.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.trim_end().strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    let Ok(Value::Array(findings)) = serde_json::from_str::<Value>(inner) else {
        return trimmed.to_string();
    };
    if findings.is_empty() {
        return "Claude's review found no issues.".into();
    }
    let field = |f: &Value, keys: &[&str]| {
        keys.iter()
            .find_map(|k| f[*k].as_str().filter(|s| !s.trim().is_empty()))
            .map(str::to_owned)
    };
    let mut out = format!(
        "Claude's review found {} issue{}:\n",
        findings.len(),
        if findings.len() == 1 { "" } else { "s" }
    );
    for (index, f) in findings.iter().enumerate() {
        let summary =
            field(f, &["summary", "title", "description"]).unwrap_or_else(|| f.to_string());
        out.push_str(&format!("\n{}. **{}**", index + 1, summary.trim()));
        if let Some(file) = field(f, &["file", "path"]) {
            match f["line"].as_u64() {
                Some(line) => out.push_str(&format!(" (`{file}:{line}`)")),
                None => out.push_str(&format!(" (`{file}`)")),
            }
        }
        out.push('\n');
        if let Some(detail) = field(f, &["failure_scenario", "body", "details", "explanation"]) {
            out.push_str(&format!("   {}\n", detail.trim().replace('\n', "\n   ")));
        }
    }
    out
}

/// The `/code-review` call for a change: the reviewer reads
/// `git diff <base>...HEAD` itself.
pub fn review_command(effort: &str, base: &str) -> String {
    format!("/code-review {effort} {base}...HEAD")
}

/// Claude models Orbit offers, in the shape of Codex's catalog so the same
/// selection rules apply. Exact model names are checked against the model
/// each run reports.
pub fn catalog() -> Vec<Value> {
    let efforts = |list: &[&str]| -> Vec<Value> {
        list.iter()
            .map(|e| json!({"reasoningEffort": e, "description": ""}))
            .collect()
    };
    let all = ["low", "medium", "high", "xhigh", "max"];
    [
        ("claude-opus-5-5", "Claude Opus 5.5", true, &all[..]),
        ("claude-fable-5-1", "Claude Fable 5.1", false, &all[..]),
        ("claude-sonnet-5-5", "Claude Sonnet 5.5", false, &all[..]),
        ("claude-haiku-5-5", "Claude Haiku 5.5", false, &all[..3]),
    ]
    .into_iter()
    .map(|(model, name, default, supported)| {
        json!({
            "id": model,
            "model": model,
            "displayName": name,
            "isDefault": default,
            "defaultReasoningEffort": "high",
            "supportedReasoningEfforts": efforts(supported),
        })
    })
    .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fake `claude`. Its behavior is chosen by the first prefix argument.
    pub(crate) fn fake(mode: &str) -> Launcher {
        Launcher {
            program: "node".into(),
            prefix: vec![
                concat!(env!("CARGO_MANIFEST_DIR"), "/src/claude_fixture.cjs").into(),
                mode.into(),
            ],
        }
    }

    #[test]
    fn review_turns_never_load_the_pull_requests_settings_or_edit_tools() {
        let turn = Turn {
            kind: TurnKind::Review,
            cwd: "/tmp".into(),
            session: "s".into(),
            resume: false,
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            prompt: review_command("high", "abc"),
            schema: None,
        };
        let args = turn.args();
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("-p"), "/code-review high abc...HEAD");
        assert_eq!(after("--setting-sources"), "");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--session-id"), "s");
        assert!(args.contains(&"--strict-mcp-config".to_string()));
        let settings: Value = serde_json::from_str(&after("--settings")).unwrap();
        assert_eq!(settings["sandbox"]["enabled"], true);
        assert_eq!(settings["sandbox"]["failIfUnavailable"], true);
        assert_eq!(settings["sandbox"]["allowUnsandboxedCommands"], false);
        let tools = after("--tools");
        assert!(
            !tools.contains("Edit") && !tools.contains("Write"),
            "{tools}"
        );
        assert!(!args.contains(&"--json-schema".to_string()));
        let extraction = Turn {
            kind: TurnKind::Extraction,
            resume: true,
            schema: Some(json!({"type":"object"})),
            ..turn
        };
        let args = extraction.args();
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--resume"), "s");
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--json-schema"), r#"{"type":"object"}"#);
        assert!(!args.contains(&"--allowedTools".to_string()));
    }

    #[test]
    fn formats_code_review_findings_as_a_report() {
        let result = "```json\n[{\"file\":\"m.py\",\"line\":6,\"summary\":\"avg() divides by zero.\",\"failure_scenario\":\"avg([]) raises.\"}]\n```";
        assert_eq!(
            report(result),
            "Claude's review found 1 issue:\n\n1. **avg() divides by zero.** (`m.py:6`)\n   avg([]) raises.\n"
        );
        assert_eq!(report("[]"), "Claude's review found no issues.");
        assert_eq!(report(" Plain words. "), "Plain words.");
    }

    #[test]
    fn reads_tool_calls_results_and_denials() {
        let mut reader = Reader::default();
        let call = json!({"type":"assistant","parent_tool_use_id":"forked","message":{"id":"m1","content":[
            {"type":"text","text":"sub-agent notes"},
            {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"git diff a...HEAD"}},
            {"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/w/m.py"}}]}});
        assert_eq!(
            reader.read(&call),
            vec![
                Output::CommandStarted {
                    tool: "Bash".into(),
                    command: "git diff a...HEAD".into()
                },
                Output::CommandStarted {
                    tool: "Read".into(),
                    command: "read /w/m.py".into()
                },
            ]
        );
        let results = json!({"type":"user","message":{"content":[
            {"type":"tool_result","tool_use_id":"t1","content":"diff --git"},
            {"type":"tool_result","tool_use_id":"t2","content":"file contents"}]}});
        assert_eq!(
            reader.read(&results),
            vec![Output::CommandOutput("diff --git".into())]
        );
        let done = json!({"type":"result","subtype":"success","is_error":false,"result":"Done",
            "permission_denials":[{"tool_name":"Bash","tool_input":{"command":"cargo test"}}]});
        assert_eq!(
            reader.read(&done),
            vec![Output::Result {
                ok: true,
                text: "Done".into(),
                structured: None,
                denials: vec!["cargo test".into()],
            }]
        );
    }

    #[test]
    fn offers_one_default_model_with_known_efforts() {
        let catalog = catalog();
        crate::models::validate_catalog(&catalog).unwrap();
        assert_eq!(catalog.iter().filter(|m| m["isDefault"] == true).count(), 1);
    }

    #[tokio::test]
    async fn checks_version_and_sign_in() {
        assert_eq!(fake("ok").check().await.unwrap(), "2.1.295");
        assert!(fake("signed-out")
            .check()
            .await
            .unwrap_err()
            .contains("not signed in"));
        let missing = Launcher {
            program: "orbit-no-such-claude".into(),
            prefix: vec![],
        };
        assert!(missing
            .check()
            .await
            .unwrap_err()
            .contains("Install Claude Code"));
    }
}
