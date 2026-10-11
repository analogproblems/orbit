//! Runs the `claude` CLI in print mode, one process per turn, and reads its
//! `stream-json` output. Orbit never reads or stores Claude credentials: the
//! CLI uses the sign-in of whoever runs Orbit.
use crate::boxtool;
use crate::cli::{self, Dialect, Init, Launcher, Line, Output, Started, Stderr, Turn, TurnKind};
use serde_json::{json, Value};
use std::collections::HashMap;

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
const MISSING: &str = "Cannot start Claude. Install Claude Code, then run claude once to sign in.";

/// How Orbit starts `claude`.
pub fn launcher() -> Launcher {
    Launcher::new("claude")
}

/// The CLI's version, after checking it is signed in.
pub async fn check(launcher: &Launcher) -> Result<String, String> {
    let version = launcher.output(&["--version"], "claude", MISSING).await?;
    let status = launcher
        .output(&["auth", "status"], "claude", MISSING)
        .await?;
    let status: Value = serde_json::from_str(&status)
        .map_err(|_| "Claude reported its sign-in in an unexpected format.".to_string())?;
    if status["loggedIn"] != true {
        return Err(
            "Claude is not signed in. Run claude once to sign in, then check again in Settings."
                .into(),
        );
    }
    Ok(version
        .split_whitespace()
        .next()
        .unwrap_or(&version)
        .to_string())
}

/// The arguments of a Claude turn.
pub fn args(turn: &Turn) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-p".into(),
        turn.prompt.clone(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--model".into(),
        turn.model.clone(),
        "--effort".into(),
        turn.effort.clone(),
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
            // Sandboxed commands still need the allow list: auto-allowing
            // them would let any command run, writes to the worktree
            // included.
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
        }})
        .to_string(),
    ];
    args.push(
        if turn.resume {
            "--resume"
        } else {
            "--session-id"
        }
        .into(),
    );
    args.push(turn.session.clone());
    args.push("--tools".into());
    args.push(
        match turn.kind {
            TurnKind::Extraction => "",
            _ => REVIEW_TOOLS,
        }
        .into(),
    );
    // The dev container tool runs its commands in the container, outside
    // Claude's sandbox, which is the point of it.
    let tool = turn.tool.as_ref().filter(|_| turn.kind == TurnKind::Review);
    if let Some(tool) = tool {
        args.push("--mcp-config".into());
        args.push(tool.claude());
        args.push("--append-system-prompt".into());
        args.push(tool.notes());
    }
    if turn.kind != TurnKind::Extraction {
        args.push("--allowedTools".into());
        args.extend(ALLOWED.iter().map(|s| s.to_string()));
        if tool.is_some() {
            args.push(boxtool::CLAUDE_TOOL.into());
        }
        args.push("--disallowedTools".into());
        args.extend(DENIED.iter().map(|s| s.to_string()));
    }
    if let Some(schema) = &turn.schema {
        args.push("--json-schema".into());
        args.push(schema.to_string());
    }
    args
}

/// Starts a Claude turn; see `cli::spawn`.
pub fn start(launcher: &Launcher, turn: &Turn) -> Result<Started, String> {
    cli::spawn(
        launcher,
        args(turn),
        &turn.cwd,
        None,
        Reader::default(),
        MISSING,
    )
}

/// Turns `stream-json` messages into outputs, remembering which tool each
/// call used so only command output is shown.
#[derive(Default)]
struct Reader {
    tools: HashMap<String, String>,
}
impl Dialect for Reader {
    const NAME: &'static str = "Claude";
    fn read(&mut self, message: &Value) -> Vec<Line> {
        if message["type"] == "system" && message["subtype"] == "init" {
            let text = |key: &str| message[key].as_str().map(str::to_owned);
            return vec![Line::Init(Init {
                model: text("model").unwrap_or_default(),
                session: text("session_id"),
                permission_mode: text("permissionMode"),
            })];
        }
        self.outputs(message)
            .into_iter()
            .map(Line::Output)
            .collect()
    }
    fn stderr(line: &str) -> Stderr {
        if line
            .to_lowercase()
            .contains("sandbox required but unavailable")
        {
            return Stderr::Fatal(format!(
                "Claude reviews only run inside Claude's sandbox, and it can't start on this computer, so the review didn't run. Install what Claude names, or review this repository with Codex. Claude said: {}",
                line.trim().trim_start_matches("Error: ")
            ));
        }
        Stderr::Plain
    }
}
impl Reader {
    fn outputs(&mut self, message: &Value) -> Vec<Output> {
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
                        && matches!(
                            self.tools.get(id).map(String::as_str),
                            Some("Bash" | boxtool::CLAUDE_TOOL)
                        )
                    {
                        out.push(Output::CommandOutput(cli::result_text(&block["content"])));
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
        boxtool::CLAUDE_TOOL => input["command"]
            .as_str()
            .map(|c| format!("{c} (in the dev container)")),
        other => Some(other.to_string()),
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
            tool: None,
        };
        let args = args(&turn);
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("-p"), "/code-review high abc...HEAD");
        assert_eq!(after("--setting-sources"), "");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--session-id"), "s");
        assert!(args.contains(&"--strict-mcp-config".to_string()));
        let settings: Value = serde_json::from_str(&after("--settings")).unwrap();
        assert_eq!(settings["sandbox"]["enabled"], true);
        assert_eq!(settings["sandbox"]["failIfUnavailable"], true);
        assert_eq!(settings["sandbox"]["autoAllowBashIfSandboxed"], false);
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
        let args = super::args(&extraction);
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--resume"), "s");
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--json-schema"), r#"{"type":"object"}"#);
        assert!(!args.contains(&"--allowedTools".to_string()));
    }

    #[test]
    fn review_turns_get_the_dev_container_tool_when_the_repository_allows_it() {
        let server = crate::boxtool::Server::new(crate::boxtool::Spec {
            docker: Launcher::new("docker"),
            container: "orbit-checks-r1".into(),
            user: None,
            folder: "/workspaces/b".into(),
            network: false,
        })
        .unwrap();
        let turn = Turn {
            kind: TurnKind::Review,
            cwd: "/tmp".into(),
            session: "s".into(),
            resume: false,
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            prompt: review_command("high", "abc"),
            schema: None,
            tool: Some(server.clone()),
        };
        let args = args(&turn);
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--mcp-config"), server.claude());
        assert_eq!(after("--append-system-prompt"), server.notes());
        // Only Orbit's servers, and its tool runs without asking.
        assert!(args.contains(&"--strict-mcp-config".to_string()));
        let allowed = args.iter().position(|a| a == "--allowedTools").unwrap();
        let denied = args.iter().position(|a| a == "--disallowedTools").unwrap();
        assert!(args[allowed..denied].contains(&boxtool::CLAUDE_TOOL.to_string()));
        // Later turns don't use it.
        for kind in [TurnKind::Extraction, TurnKind::FollowUp] {
            let later = super::args(&Turn {
                kind,
                resume: true,
                ..turn.clone()
            });
            assert!(!later.contains(&"--mcp-config".to_string()));
            assert!(!later.contains(&boxtool::CLAUDE_TOOL.to_string()));
        }
        // Its calls show in the chat like commands.
        let mut reader = Reader::default();
        let lines = reader.read(&json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m", "content": [
            {"type": "tool_use", "id": "t1", "name": boxtool::CLAUDE_TOOL, "input": {"command": "npm test"}}
        ]}}));
        assert_eq!(
            lines,
            [Line::Output(Output::CommandStarted {
                tool: boxtool::CLAUDE_TOOL.into(),
                command: "npm test (in the dev container)".into()
            })]
        );
        let lines = reader.read(&json!({"type": "user", "parent_tool_use_id": null, "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "passed in 3s"}]}
        ]}}));
        assert_eq!(
            lines,
            [Line::Output(Output::CommandOutput("passed in 3s".into()))]
        );
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
            reader.outputs(&call),
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
            reader.outputs(&results),
            vec![Output::CommandOutput("diff --git".into())]
        );
        let done = json!({"type":"result","subtype":"success","is_error":false,"result":"Done",
            "permission_denials":[{"tool_name":"Bash","tool_input":{"command":"cargo test"}}]});
        assert_eq!(
            reader.outputs(&done),
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
        assert_eq!(check(&fake("ok")).await.unwrap(), "2.1.295");
        assert!(check(&fake("signed-out"))
            .await
            .unwrap_err()
            .contains("not signed in"));
        assert!(check(&Launcher::new("orbit-no-such-claude"))
            .await
            .unwrap_err()
            .contains("Install Claude Code"));
    }
}
