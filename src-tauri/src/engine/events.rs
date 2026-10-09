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
