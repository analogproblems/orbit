//! What a reviewer reports while it works, in Orbit's own terms. Each
//! provider translates its own stream into these events, and
//! `run::apply_event` applies them to the review the same way whoever sent
//! them.
use serde_json::Value;

/// How a reviewer's turn ended.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum TurnEnd {
    Completed,
    Interrupted,
    /// The provider's error, with a `message` when it gave one. Codex usage
    /// limits are recognized from it.
    Failed(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ReviewEvent {
    /// A turn began, with the provider's id for it when it has one.
    TurnStarted(Option<String>),
    /// More text of the reviewer's message `item`, shown as it streams.
    TextDelta {
        item: String,
        delta: String,
    },
    /// The reviewer's finished message `item`.
    Text {
        item: String,
        text: String,
    },
    CommandStarted(String),
    /// A finished command's output.
    CommandOutput(String),
    /// The review's report, as people will read it.
    Report(String),
    TurnDone(TurnEnd),
    /// The reviewer stopped waiting for the approval with this request id.
    RequestResolved(Value),
    /// A command the reviewer was refused without anyone being asked.
    Declined(String),
    /// Something the developer should know about the run.
    Notice(String),
    /// The reviewer could not do the review, though its turns finished.
    Incomplete(String),
    /// Anything else the reviewer did, which only shows the review is alive.
    Activity,
}

impl ReviewEvent {
    /// Translates a Codex app-server notification.
    pub(super) fn from_codex(method: &str, p: &Value) -> Self {
        let text = |v: &Value| v.as_str().unwrap_or("").to_string();
        match method {
            "turn/started" => Self::TurnStarted(p["turn"]["id"].as_str().map(str::to_owned)),
            "item/agentMessage/delta" => Self::TextDelta {
                item: text(&p["itemId"]),
                delta: text(&p["delta"]),
            },
            "item/started" if p["item"]["type"] == "commandExecution" => Self::CommandStarted(
                p["item"]["command"]
                    .as_str()
                    .unwrap_or("command")
                    .to_string(),
            ),
            "item/started" if in_box(&p["item"]) => Self::CommandStarted(box_command(
                p["item"]["arguments"]["command"]
                    .as_str()
                    .unwrap_or("command"),
            )),
            "item/completed" if in_box(&p["item"]) => {
                let item = &p["item"];
                Self::CommandOutput(match item["result"]["content"].as_array() {
                    Some(content) => content
                        .iter()
                        .filter_map(|c| c["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    None => text(&item["error"]["message"]),
                })
            }
            "item/completed" => {
                let item = &p["item"];
                match item["type"].as_str().unwrap_or("") {
                    "agentMessage" => Self::Text {
                        item: text(&item["id"]),
                        text: text(&item["text"]),
                    },
                    "exitedReviewMode" => Self::Report(text(&item["review"])),
                    "commandExecution" => Self::CommandOutput(text(&item["aggregatedOutput"])),
                    _ => Self::Activity,
                }
            }
            "turn/completed" => Self::TurnDone(match p["turn"]["status"].as_str().unwrap_or("") {
                "completed" => TurnEnd::Completed,
                "interrupted" => TurnEnd::Interrupted,
                _ => TurnEnd::Failed(p["turn"]["error"].clone()),
            }),
            "serverRequest/resolved" => Self::RequestResolved(p["requestId"].clone()),
            _ => Self::Activity,
        }
    }
}

/// Whether a Codex item is a call to Orbit's dev container tool.
fn in_box(item: &Value) -> bool {
    item["type"] == "mcpToolCall"
        && item["server"] == crate::boxtool::SERVER
        && item["tool"] == crate::boxtool::TOOL
}
/// A command the reviewer ran in the dev container, as the chat shows it.
pub(super) fn box_command(command: &str) -> String {
    format!("{command} (in the dev container)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn translates_codex_notifications() {
        let event = |method, p| ReviewEvent::from_codex(method, &p);
        assert_eq!(
            event("turn/started", json!({"turn":{"id":"t"}})),
            ReviewEvent::TurnStarted(Some("t".into()))
        );
        assert_eq!(
            event(
                "item/started",
                json!({"item":{"type":"commandExecution","command":"cargo test"}})
            ),
            ReviewEvent::CommandStarted("cargo test".into())
        );
        assert_eq!(
            event("item/started", json!({"item":{"type":"reasoning"}})),
            ReviewEvent::Activity
        );
        // Calls to Orbit's dev container tool show like commands.
        let call = json!({"type":"mcpToolCall","server":"orbit_box","tool":"run","arguments":{"command":"npm test"}});
        assert_eq!(
            event("item/started", json!({"item": call})),
            ReviewEvent::CommandStarted("npm test (in the dev container)".into())
        );
        let mut done = call.clone();
        done["result"] = json!({"content":[{"type":"text","text":"passed in 3s"}]});
        assert_eq!(
            event("item/completed", json!({"item": done})),
            ReviewEvent::CommandOutput("passed in 3s".into())
        );
        let mut failed = call.clone();
        failed["error"] = json!({"message":"refused"});
        assert_eq!(
            event("item/completed", json!({"item": failed})),
            ReviewEvent::CommandOutput("refused".into())
        );
        let mut other = call;
        other["server"] = json!("someone_else");
        assert_eq!(
            event("item/started", json!({"item": other})),
            ReviewEvent::Activity
        );
        assert_eq!(
            event(
                "item/completed",
                json!({"item":{"type":"exitedReviewMode","review":"Report"}})
            ),
            ReviewEvent::Report("Report".into())
        );
        assert_eq!(
            event(
                "turn/completed",
                json!({"turn":{"status":"failed","error":{"message":"m"}}})
            ),
            ReviewEvent::TurnDone(TurnEnd::Failed(json!({"message":"m"})))
        );
        assert_eq!(
            event("turn/completed", json!({"turn":{"status":"interrupted"}})),
            ReviewEvent::TurnDone(TurnEnd::Interrupted)
        );
        assert_eq!(
            event("serverRequest/resolved", json!({"requestId":7})),
            ReviewEvent::RequestResolved(json!(7))
        );
    }
}
