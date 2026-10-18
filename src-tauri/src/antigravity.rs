//! Runs the Antigravity CLI (`agy`) headless, one process per turn, and reads
//! its `stream-json` output. Orbit never reads or stores Antigravity
//! credentials, and never changes its settings: the CLI uses the sign-in and
//! settings of whoever runs Orbit, and Orbit refuses to review when those
//! settings would let a pull request reach past the review's sandbox.
use crate::cli::{self, Dialect, Init, Launcher, Line, Output, Started, Stderr, Turn};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

const MISSING: &str =
    "Cannot start Antigravity. Install the Antigravity CLI (agy), then run agy once to sign in.";
/// The reasoning efforts `agy --effort` takes.
pub const EFFORTS: [&str; 3] = ["low", "medium", "high"];
/// Permission modes in which tool calls that need approval are refused
/// rather than run, as nobody is there to approve them.
pub const ASKING_MODES: [&str; 2] = ["request-review", "strict"];
/// Allow rules that reach past the sandbox: commands run on the computer
/// itself, the network, the developer's MCP tools, and files. Inside the
/// review's folder reading and writing need no rule, so a file grant only
/// ever reaches outside it, and grants also become sandbox mounts.
const UNSAFE_RULES: [&str; 6] = [
    "unsandboxed",
    "read_url",
    "execute_url",
    "mcp",
    "read_file",
    "write_file",
];

/// How Orbit starts `agy`, and where it reads Antigravity's settings; tests
/// use a fake and their own settings.
#[derive(Clone, Debug)]
pub struct Setup {
    pub launcher: Launcher,
    /// Antigravity's global settings, or None when no home folder is known.
    pub settings: Option<PathBuf>,
}
impl Default for Setup {
    fn default() -> Self {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        Self {
            launcher: Launcher::new("agy"),
            settings: home.map(|h| {
                PathBuf::from(h)
                    .join(".gemini")
                    .join("antigravity-cli")
                    .join("settings.json")
            }),
        }
    }
}

/// Checks Antigravity is installed, signed in and set up so reviews stay in
/// its sandbox, and returns the models it offers, in the shape of Codex's
/// catalog.
pub async fn check(setup: &Setup) -> Result<Vec<Value>, String> {
    let models = setup
        .launcher
        .output(&["models"], "agy", MISSING)
        .await
        .map_err(|e| {
            if e.to_lowercase().contains("auth") {
                "Antigravity is not signed in. Run agy once to sign in, then check again in Settings.".into()
            } else {
                e
            }
        })?;
    let catalog = catalog(&models);
    if catalog.is_empty() {
        return Err("Antigravity listed no models. Run agy models to see why.".into());
    }
    check_settings(setup.settings.as_deref())?;
    Ok(catalog)
}

/// The models `agy models` lists, one per line as `slug (Display name)`.
/// Lines that do not start with a model slug, such as headings, are skipped.
pub fn catalog(listing: &str) -> Vec<Value> {
    let efforts: Vec<Value> = EFFORTS
        .iter()
        .map(|e| json!({"reasoningEffort": e, "description": ""}))
        .collect();
    let mut seen = HashSet::new();
    listing
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', ' ']).trim_start();
            let slug = line.split_whitespace().next()?;
            let is_slug = slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
                && slug.chars().next()?.is_ascii_alphanumeric();
            if !is_slug || !seen.insert(slug.to_string()) {
                return None;
            }
            let rest = line[slug.len()..].trim();
            let name = rest
                .strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(slug);
            Some(json!({
                "id": slug,
                "model": slug,
                "displayName": name,
                "isDefault": false,
                "defaultReasoningEffort": "high",
                "supportedReasoningEfforts": efforts,
            }))
        })
        .collect()
}

/// Refuses settings that would let a review reach past its sandbox. Orbit
/// only reads them; changing them is the developer's call.
pub fn check_settings(path: Option<&Path>) -> Result<(), String> {
    let path = path.ok_or(
        "Orbit can't find your home folder, so it can't check Antigravity's settings are safe for reviews.",
    )?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "Orbit couldn't read Antigravity's settings at {}, so it can't check they are safe for reviews: {e}",
                path.display()
            ))
        }
    };
    let settings: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "Orbit couldn't read Antigravity's settings at {}, so it can't check they are safe for reviews: {e}",
            path.display()
        )
    })?;
    let mut problems = vec![];
    if let Some(mode) = settings["toolPermission"].as_str() {
        if !ASKING_MODES.contains(&mode) {
            problems.push(format!(
                "\"toolPermission\": \"{mode}\" lets the agent act without asking"
            ));
        }
    }
    if settings["allowNonWorkspaceAccess"] == true {
        problems.push(
            "\"allowNonWorkspaceAccess\" lets the agent read files outside the review's folder"
                .into(),
        );
    }
    let rules: Vec<&str> = settings["permissions"]["allow"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|rule| {
            let action = rule.split('(').next().unwrap_or("").trim();
            UNSAFE_RULES.contains(&action.to_ascii_lowercase().as_str())
        })
        .collect();
    if !rules.is_empty() {
        problems.push(format!(
            "the allow rules {} reach past the sandbox (commands outside it, the network, MCP tools, or files outside the review's folder)",
            rules.join(", ")
        ));
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Antigravity reviews didn't start because Antigravity's settings ({}) would let a pull request reach past the review's sandbox: {}. Orbit never changes Antigravity's settings. Change them in agy (/permissions), or review this repository with another reviewer.",
        path.display(),
        problems.join("; ")
    ))
}

/// The arguments of an Antigravity turn. The prompt goes on standard input.
pub fn args(turn: &Turn) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--output-format".into(),
        "stream-json".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--model".into(),
        turn.model.clone(),
        "--effort".into(),
        turn.effort.clone(),
        // Commands the developer allowed run inside Antigravity's sandbox:
        // the review's folder only, and no network.
        "--sandbox".into(),
        // A thorough review takes longer than the default five minutes.
        "--print-timeout".into(),
        "60m".into(),
    ];
    if turn.resume {
        args.push("--conversation".into());
        args.push(turn.session.clone());
    }
    if let Some(schema) = &turn.schema {
        args.push("--json-schema".into());
        args.push(schema.to_string());
    }
    args
}

/// The turn's prompt, as Antigravity reads it on standard input.
pub fn input(prompt: &str) -> String {
    format!(
        "{}\n",
        json!({"event": "user", "message": {"content": prompt}})
    )
}

/// Starts an Antigravity turn, after checking its settings; see
/// `cli::spawn`.
pub fn start(setup: &Setup, turn: &Turn) -> Result<Started, String> {
    check_settings(setup.settings.as_deref())?;
    cli::spawn(
        &setup.launcher,
        args(turn),
        &turn.cwd,
        Some(input(&turn.prompt)),
        Reader::default(),
        MISSING,
    )
}

/// Turns `stream-json` events into outputs. Antigravity streams a message
/// as text deltas of one step, finished when the step is done.
#[derive(Default)]
struct Reader {
    conversation: String,
    texts: HashMap<u64, String>,
    started: HashSet<u64>,
}
impl Dialect for Reader {
    const NAME: &'static str = "Antigravity";
    fn read(&mut self, message: &Value) -> Vec<Line> {
        if message["event"] == "init" {
            let text = |v: &Value| v.as_str().map(str::to_owned);
            self.conversation = message["conversation_id"]
                .as_str()
                .unwrap_or_default()
                .into();
            return vec![Line::Init(Init {
                model: text(&message["init"]["model"]).unwrap_or_default(),
                session: text(&message["conversation_id"]),
                permission_mode: text(&message["init"]["permission_mode"]),
            })];
        }
        self.outputs(message)
            .into_iter()
            .map(Line::Output)
            .collect()
    }
    fn stderr(line: &str) -> Stderr {
        let lower = line.to_lowercase();
        let failed = [
            "unavailable",
            "not available",
            "unsupported",
            "not supported",
            "could not start",
            "couldn't start",
            "cannot start",
            "can't start",
            "failed to start",
            "failed to initialize",
        ];
        if lower.contains("sandbox") && failed.iter().any(|f| lower.contains(f)) {
            return Stderr::Fatal(format!(
                "Antigravity reviews only run inside Antigravity's sandbox, and it couldn't start on this computer, so the review was stopped. Antigravity said: {}",
                line.trim()
            ));
        }
        if lower.contains("denied") {
            return Stderr::Denied(line.trim().to_string());
        }
        Stderr::Plain
    }
}
impl Reader {
    fn outputs(&mut self, message: &Value) -> Vec<Output> {
        let mut out = vec![];
        match message["event"].as_str().unwrap_or("") {
            "step_update" => {
                let step = &message["step_update"];
                let index = step["step_index"].as_u64().unwrap_or(0);
                let done = step["state"] == "DONE";
                match step["step_type"].as_str().unwrap_or("") {
                    // A sub-agent's words are its working notes.
                    "agent_response" if step["subagent_info"].is_null() => {
                        let text = self.texts.entry(index).or_default();
                        text.push_str(step["text_delta"].as_str().unwrap_or(""));
                        if done {
                            let text = self.texts.remove(&index).unwrap_or_default();
                            if !text.trim().is_empty() {
                                out.push(Output::Text {
                                    item: format!("{}-{index}", self.conversation),
                                    text,
                                });
                            }
                        }
                    }
                    "tool" => {
                        let info = &step["tool_info"];
                        let tool = info["name"]
                            .as_str()
                            .or(step["tool_name"].as_str())
                            .unwrap_or("tool")
                            .to_string();
                        let command = command(&info["parameters"]);
                        if self.started.insert(index) {
                            out.push(Output::CommandStarted {
                                command: command
                                    .clone()
                                    .unwrap_or_else(|| describe(&tool, &info["parameters"])),
                                tool,
                            });
                        }
                        if done && command.is_some() {
                            let text = match info["error"]["message"].as_str() {
                                Some(error) => error.to_string(),
                                None => cli::result_text(&info["output"]),
                            };
                            out.push(Output::CommandOutput(text));
                        }
                    }
                    _ => {}
                }
            }
            "result" => {
                let result = &message["result"];
                let ok = result["status"] == "SUCCESS";
                let text = if ok {
                    result["response"].as_str().unwrap_or("").to_string()
                } else {
                    result["error"]
                        .as_str()
                        .filter(|e| !e.trim().is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            format!(
                                "Antigravity ended the turn with status {}.",
                                result["status"].as_str().unwrap_or("unknown")
                            )
                        })
                };
                out.push(Output::Result {
                    ok,
                    text,
                    structured: Some(result["structured_output"].clone()).filter(|v| !v.is_null()),
                    denials: vec![],
                });
            }
            _ => {}
        }
        out
    }
}
/// The shell command a tool call runs, if it runs one.
fn command(parameters: &Value) -> Option<String> {
    ["command", "CommandLine", "commandLine"]
        .iter()
        .find_map(|k| parameters[*k].as_str())
        .map(str::to_owned)
}
/// Any other tool call in words, for the chat.
fn describe(tool: &str, parameters: &Value) -> String {
    let target = [
        "path",
        "file_path",
        "AbsolutePath",
        "absolute_path",
        "TargetFile",
        "pattern",
        "query",
    ]
    .iter()
    .find_map(|k| parameters[*k].as_str());
    match target {
        Some(target) => format!("{tool} {target}"),
        None => tool.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cli::TurnKind;

    /// A fake `agy`, whose behavior is chosen by `mode`, with settings at
    /// `settings` (none written means Antigravity's defaults).
    pub(crate) fn fake(mode: &str, settings: &Path) -> Setup {
        Setup {
            launcher: Launcher {
                program: "node".into(),
                prefix: vec![
                    concat!(env!("CARGO_MANIFEST_DIR"), "/src/agy_fixture.cjs").into(),
                    mode.into(),
                ],
            },
            settings: Some(settings.to_path_buf()),
        }
    }

    fn turn(kind: TurnKind, resume: bool) -> Turn {
        Turn {
            kind,
            cwd: "/tmp".into(),
            session: if resume { "c1".into() } else { String::new() },
            resume,
            model: "gemini-3.1-pro-high".into(),
            effort: "high".into(),
            prompt: "Review".into(),
            schema: resume.then(|| json!({"type": "object"})),
            tool: None,
        }
    }

    #[test]
    fn turns_run_sandboxed_and_never_skip_permissions() {
        let args = args(&turn(TurnKind::Review, false));
        let after = |args: &[String], flag: &str| {
            args[args.iter().position(|a| a == flag).unwrap() + 1].clone()
        };
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(!args.iter().any(|a| a.contains("dangerously")));
        assert!(
            !args.contains(&"-p".to_string()),
            "the prompt goes on stdin"
        );
        assert_eq!(after(&args, "--input-format"), "stream-json");
        assert_eq!(after(&args, "--model"), "gemini-3.1-pro-high");
        assert!(!args.contains(&"--conversation".to_string()));
        let later = super::args(&turn(TurnKind::Extraction, true));
        assert_eq!(after(&later, "--conversation"), "c1");
        assert_eq!(after(&later, "--json-schema"), r#"{"type":"object"}"#);
        let input: Value = serde_json::from_str(input("a \"b\"\nc").trim()).unwrap();
        assert_eq!(input["event"], "user");
        assert_eq!(input["message"]["content"], "a \"b\"\nc");
    }

    #[test]
    fn reads_models_from_the_listing() {
        let catalog = catalog(
            "Available models:\n  gemini-3.1-pro-high (Gemini 3.1 Pro (High))\n  gemini-3.8-flash-medium (Gemini 3.8 Flash (Medium))\n\ngemini-3.1-pro-high (again)\n",
        );
        crate::models::validate_catalog(&catalog).unwrap();
        let slugs: Vec<_> = catalog.iter().map(|m| m["model"].clone()).collect();
        assert_eq!(slugs, ["gemini-3.1-pro-high", "gemini-3.8-flash-medium"]);
        assert_eq!(catalog[0]["displayName"], "Gemini 3.1 Pro (High)");
        // Other spaces after a bullet don't trip up the reading.
        let spaced = super::catalog("*\u{3000}gemini-x (X)\n");
        assert_eq!(spaced[0]["model"], "gemini-x");
    }

    #[test]
    fn refuses_settings_that_reach_past_the_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(check_settings(Some(&path)).is_ok(), "no settings file");
        assert!(check_settings(None).is_err());
        let write = |v: Value| std::fs::write(&path, v.to_string()).unwrap();
        write(json!({"toolPermission": "request-review",
            "permissions": {"allow": ["command(git)"], "deny": ["unsandboxed(*)", "read_file(/)"]}}));
        assert!(check_settings(Some(&path)).is_ok());
        for unsafe_settings in [
            json!({"toolPermission": "always-proceed"}),
            json!({"toolPermission": "proceed-in-sandbox"}),
            json!({"allowNonWorkspaceAccess": true}),
            json!({"permissions": {"allow": ["unsandboxed(git push)"]}}),
            json!({"permissions": {"allow": ["read_url(example.com)"]}}),
            json!({"permissions": {"allow": ["mcp(*)"]}}),
            json!({"permissions": {"allow": ["read_file(*)"]}}),
            json!({"permissions": {"allow": ["read_file(~/.aws)"]}}),
            json!({"permissions": {"allow": ["write_file(/home/user)"]}}),
            json!({"permissions": {"allow": ["write_file(../other)"]}}),
        ] {
            write(unsafe_settings.clone());
            let error = check_settings(Some(&path)).unwrap_err();
            assert!(
                error.contains("never changes"),
                "{unsafe_settings}: {error}"
            );
        }
        std::fs::write(&path, "{not json").unwrap();
        assert!(check_settings(Some(&path))
            .unwrap_err()
            .contains("couldn't read"));
    }

    #[test]
    fn reads_steps_and_results() {
        let mut reader = Reader::default();
        let init = json!({"event":"init","conversation_id":"c1","init":{"cwd":"/w","model":"m","permission_mode":"request-review"}});
        assert_eq!(
            reader.read(&init),
            vec![Line::Init(Init {
                model: "m".into(),
                session: Some("c1".into()),
                permission_mode: Some("request-review".into()),
            })]
        );
        let step = |v: Value| json!({"event":"step_update","step_update":v});
        assert!(reader
            .outputs(&step(json!({"step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"Looks "})))
            .is_empty());
        assert_eq!(
            reader.outputs(&step(json!({"step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"good."}))),
            vec![Output::Text { item: "c1-1".into(), text: "Looks good.".into() }]
        );
        assert!(reader
            .outputs(&step(json!({"step_index":2,"state":"DONE","step_type":"agent_response","text_delta":"notes","subagent_info":{"name":"x"}})))
            .is_empty());
        assert_eq!(
            reader.outputs(&step(json!({"step_index":3,"state":"ACTIVE","step_type":"tool","tool_info":{"name":"run_command","parameters":{"command":"git log"}}}))),
            vec![Output::CommandStarted { tool: "run_command".into(), command: "git log".into() }]
        );
        assert_eq!(
            reader.outputs(&step(json!({"step_index":3,"state":"DONE","step_type":"tool","tool_info":{"name":"run_command","parameters":{"command":"git log"},"output":"abc"}}))),
            vec![Output::CommandOutput("abc".into())]
        );
        assert_eq!(
            reader.outputs(&step(json!({"step_index":4,"state":"DONE","step_type":"tool","tool_info":{"name":"view_file","parameters":{"AbsolutePath":"/w/m.py"},"output":"..."}}))),
            vec![Output::CommandStarted { tool: "view_file".into(), command: "view_file /w/m.py".into() }]
        );
        assert_eq!(
            reader.outputs(&json!({"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","response":"Done","structured_output":{"a":1}}})),
            vec![Output::Result { ok: true, text: "Done".into(), structured: Some(json!({"a":1})), denials: vec![] }]
        );
        assert_eq!(
            reader.outputs(&json!({"event":"result","result":{"status":"ERROR","error":"invalid model selection"}})),
            vec![Output::Result { ok: false, text: "invalid model selection".into(), structured: None, denials: vec![] }]
        );
    }

    #[test]
    fn stops_when_the_sandbox_cannot_start() {
        assert!(matches!(
            Reader::stderr("Error: terminal sandbox unavailable: user namespaces are disabled"),
            Stderr::Fatal(reason) if reason.contains("only run inside Antigravity's sandbox")
        ));
        assert_eq!(
            Reader::stderr("Permission denied for tool run_command (git status)"),
            Stderr::Denied("Permission denied for tool run_command (git status)".into())
        );
        assert_eq!(Reader::stderr("warning: unknown event"), Stderr::Plain);
    }

    #[tokio::test]
    async fn checks_sign_in_models_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let catalog = check(&fake("ok", &settings)).await.unwrap();
        assert_eq!(catalog[0]["model"], "gemini-3.1-pro-high");
        assert!(check(&fake("signed-out", &settings))
            .await
            .unwrap_err()
            .contains("not signed in"));
        std::fs::write(&settings, r#"{"toolPermission":"always-proceed"}"#).unwrap();
        assert!(check(&fake("ok", &settings))
            .await
            .unwrap_err()
            .contains("never changes"));
        let missing = Setup {
            launcher: Launcher::new("orbit-no-such-agy"),
            settings: Some(settings),
        };
        assert!(check(&missing)
            .await
            .unwrap_err()
            .contains("Install the Antigravity CLI"));
    }
}
