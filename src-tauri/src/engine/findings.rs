//! After a review turn, Codex restates its findings as JSON in one more turn
//! on the same thread. The Markdown report stays the record of the review;
//! the JSON is stored beside it for the review loop.
use super::Engine;
use crate::{rpc::Rpc, types::*};
use serde_json::{json, Value};
use std::{
    path::{Component, Path},
    sync::{atomic::Ordering, Arc},
};

const EXTRACTION_PROMPT: &str = "Restate the findings from the review you just completed as JSON matching the output schema. \
    Do not run commands, read files, or review the code again. \
    Include every finding you reported, including low-priority ones, with the title without its [P#] prefix, \
    priority 0-3 for P0-P3, the file path relative to the repository root, and the line range. \
    Use verdict \"clean\" only when you reported no findings and no earlier finding is still_open or declined_disputed. \
    If your review classified earlier findings, list each in priorFindings with its ID, status and note; otherwise use an empty list.";

const PRIOR_STATUSES: [(&str, PriorStatus); 4] = [
    ("fixed", PriorStatus::Fixed),
    ("still_open", PriorStatus::StillOpen),
    ("declined_accepted", PriorStatus::DeclinedAccepted),
    ("declined_disputed", PriorStatus::DeclinedDisputed),
];

pub(super) fn output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["verdict", "summary", "findings", "testsNotRun", "priorFindings"],
        "properties": {
            "verdict": {"type": "string", "enum": ["clean", "changes_requested"]},
            "summary": {"type": "string"},
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "priority", "path", "startLine", "endLine", "body", "confidence"],
                    "properties": {
                        "title": {"type": "string"},
                        "priority": {"type": "integer", "enum": [0, 1, 2, 3]},
                        "path": {"type": "string"},
                        "startLine": {"type": "integer"},
                        "endLine": {"type": "integer"},
                        "body": {"type": "string"},
                        "confidence": {"type": "number"},
                    },
                },
            },
            "testsNotRun": {"type": "array", "items": {"type": "string"}},
            "priorFindings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "status", "note"],
                    "properties": {
                        "id": {"type": "string"},
                        "status": {"type": "string", "enum": PRIOR_STATUSES.map(|(name, _)| name)},
                        "note": {"type": "string"},
                    },
                },
            },
        },
    })
}

#[derive(Debug, PartialEq)]
pub(super) struct Extracted {
    pub verdict: Verdict,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub tests_not_run: Vec<String>,
    pub prior_findings: Vec<PriorFinding>,
}
impl Extracted {
    pub(super) fn store(self, r: &mut Review) {
        r.verdict = Some(self.verdict);
        r.summary = Some(self.summary);
        r.findings = Some(self.findings);
        r.tests_not_run = self.tests_not_run;
        r.prior_findings = self.prior_findings;
    }
}

/// Validates Codex's JSON. Anything that does not match the schema is refused
/// as a whole, leaving the run unstructured. Classifications of findings the
/// review was not given (`previous`) are ignored.
pub(super) fn parse(
    text: &str,
    review_id: &str,
    worktree: Option<&str>,
    previous: Option<&PreviousRound>,
) -> Result<Extracted, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("Invalid JSON: {e}"))?;
    let string = |v: &Value, key: &str| {
        v[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("'{key}' must be a string."))
    };
    let line = |v: &Value, key: &str| {
        v[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("'{key}' must be a line number."))
    };
    let verdict = match v["verdict"].as_str() {
        Some("clean") => Verdict::Clean,
        Some("changes_requested") => Verdict::ChangesRequested,
        _ => return Err("'verdict' must be clean or changes_requested.".into()),
    };
    let findings = v["findings"]
        .as_array()
        .ok_or("'findings' must be a list.")?
        .iter()
        .enumerate()
        .map(|(index, f)| {
            let priority = f["priority"]
                .as_u64()
                .filter(|p| *p <= 3)
                .ok_or("'priority' must be 0 to 3.")? as u8;
            let confidence = f["confidence"]
                .as_f64()
                .filter(|c| (0.0..=1.0).contains(c))
                .ok_or("'confidence' must be between 0 and 1.")?;
            let start_line = line(f, "startLine")?;
            let end_line = line(f, "endLine")?;
            if end_line < start_line {
                return Err("'endLine' must not be before 'startLine'.".into());
            }
            let title = string(f, "title")?;
            Ok(Finding {
                id: format!("{review_id}-{index}"),
                title: strip_priority(&title).into(),
                priority,
                path: relative_path(&string(f, "path")?, worktree),
                start_line,
                end_line,
                body: string(f, "body")?,
                confidence,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let tests_not_run = v["testsNotRun"]
        .as_array()
        .ok_or("'testsNotRun' must be a list.")?
        .iter()
        .map(|t| {
            t.as_str()
                .map(str::to_owned)
                .ok_or("'testsNotRun' must list strings.")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut prior_findings: Vec<PriorFinding> = vec![];
    for p in v["priorFindings"]
        .as_array()
        .ok_or("'priorFindings' must be a list.")?
    {
        let status = PRIOR_STATUSES
            .iter()
            .find(|(name, _)| p["status"] == *name)
            .map(|(_, status)| *status)
            .ok_or("'status' must be fixed, still_open, declined_accepted or declined_disputed.")?;
        let id = string(p, "id")?;
        let note = string(p, "note")?;
        let known = previous.is_some_and(|round| round.findings.iter().any(|f| f.id == id));
        if known && !prior_findings.iter().any(|p| p.id == id) {
            prior_findings.push(PriorFinding { id, status, note });
        }
    }
    // An unresolved earlier finding is kept out of `findings`, but the change
    // is not clean while it stands.
    let unresolved = prior_findings.iter().any(|p| {
        matches!(
            p.status,
            PriorStatus::StillOpen | PriorStatus::DeclinedDisputed
        )
    });
    let verdict = if unresolved {
        Verdict::ChangesRequested
    } else {
        verdict
    };
    Ok(Extracted {
        verdict,
        summary: string(&v, "summary")?,
        findings,
        tests_not_run,
        prior_findings,
    })
}

/// `[P2] Title` → `Title`, in case Codex keeps its report's prefix.
fn strip_priority(title: &str) -> &str {
    let trimmed = title.trim();
    match trimmed.strip_prefix("[P") {
        Some(rest)
            if rest.len() > 2
                && rest.as_bytes()[0].is_ascii_digit()
                && rest.as_bytes()[1] == b']' =>
        {
            rest[2..].trim_start()
        }
        _ => trimmed,
    }
}

/// A repository-relative path with `/` separators, or None for a path that
/// leaves the worktree. Codex may report absolute paths inside the worktree.
pub(super) fn relative_path(path: &str, worktree: Option<&str>) -> Option<String> {
    let path = Path::new(path.trim());
    let relative = if path.is_absolute() {
        let worktree = Path::new(worktree?);
        let canonical = std::fs::canonicalize(worktree).ok();
        path.strip_prefix(worktree)
            .ok()
            .or_else(|| canonical.as_deref().and_then(|c| path.strip_prefix(c).ok()))?
    } else {
        path
    };
    let mut parts = vec![];
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl Engine {
    async fn extracting(&self, id: &str) -> Result<(), String> {
        if self
            .state
            .lock()
            .await
            .reviews
            .iter()
            .any(|r| r.id == id && r.extracting && active(&r.status))
        {
            Ok(())
        } else {
            Err("Review was stopped.".into())
        }
    }
    /// Starts the extraction turn. Like a follow-up, it resumes the thread with
    /// the review's pinned model and effort, verifies Codex's response and saves
    /// the confirmation before any inference. A failure to start leaves the run
    /// unstructured with its report kept.
    pub(super) async fn extract_findings(self: &Arc<Self>, id: &str) {
        let result = self.start_extraction(id).await;
        if let Err(error) = result {
            let mut autopost = false;
            self.update(|s| {
                let blocking_priority = s.settings.blocking_priority;
                if let Some(r) = s.reviews.iter_mut().find(|r| r.id == id) {
                    autopost = r.end_extraction(Some(error)) && r.automatic;
                    crate::review_loop::decide(r, blocking_priority);
                }
            })
            .await;
            let _ = self.flush().await;
            if autopost {
                self.autopost(id);
            }
        }
    }
    async fn start_extraction(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let s = self.snapshot().await;
        let r = s
            .reviews
            .iter()
            .find(|r| r.id == id)
            .ok_or("Unknown review")?;
        if r.provider == Provider::Claude {
            return self.start_claude_extraction(id).await;
        }
        let selection = ModelSelection::requested(r);
        selection.validate_execution(&s)?;
        let thread = r
            .thread_id
            .clone()
            .ok_or("The review has no Codex thread.")?;
        let rpc: Arc<Rpc> = self.client().await?;
        self.extracting(id).await?;
        let resume = json!({
            "threadId": thread,
            "cwd": r.worktree,
            "approvalPolicy": "on-request",
            "approvalsReviewer": "user",
            "sandbox": "workspace-write",
            "model": selection.model,
            "config": {"review_model":selection.model, "model_reasoning_effort":selection.effort},
        });
        let response = rpc.request("thread/resume", resume).await?;
        selection.verify_response(&response)?;
        {
            let mut state = self.state.lock().await;
            let mut next = state.clone();
            let run = next
                .reviews
                .iter_mut()
                .find(|r| r.id == id && r.extracting && active(&r.status))
                .ok_or("Review was stopped.")?;
            run.extraction_selection = Some(selection.clone());
            self.persist(&next).map_err(|e| {
                format!(
                    "Could not save confirmed extraction settings; no extraction was started: {e}"
                )
            })?;
            *state = next;
            self.changed.store(true, Ordering::SeqCst);
        }
        self.extracting(id).await?;
        let turn = rpc
            .request(
                "turn/start",
                json!({
                    "threadId": thread, "model": selection.model, "effort": selection.effort,
                    "input": [{"type":"text","text":EXTRACTION_PROMPT}],
                    "outputSchema": output_schema(),
                }),
            )
            .await?;
        let mut accepted = false;
        self.update(|s| {
            if let Some(r) = s
                .reviews
                .iter_mut()
                .find(|r| r.id == id && r.extracting && active(&r.status))
            {
                accepted = true;
                r.turn_id = turn["turn"]["id"].as_str().map(str::to_owned);
            }
        })
        .await;
        if !accepted {
            if let Some(turn_id) = turn["turn"]["id"].as_str() {
                let _ = rpc
                    .request(
                        "turn/interrupt",
                        json!({"threadId": thread, "turnId": turn_id}),
                    )
                    .await;
            }
        }
        Ok(())
    }
}
