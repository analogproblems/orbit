//! After a review turn, the reviewer restates its findings as JSON in one
//! more turn on the same thread. The Markdown report stays the record of the
//! review; the JSON is stored beside it for the review loop.
use super::Engine;
use crate::{rpc::Rpc, types::*};
use serde_json::{json, Value};
use std::{
    path::{Component, Path},
    sync::{atomic::Ordering, Arc},
};

const EXTRACTION_PROMPT: &str = "Restate the findings from the review you just completed as JSON matching the output schema. \
    Do not run commands, read files, or review the code again. \
    Include every finding you reported, including low-priority ones, with the title without its [P#] prefix and \
    priority 0-3 for P0-P3. ";

/// What each field of the form holds, for every reviewer's extraction turn.
pub(super) const FORM_PROMPT: &str = "For each finding, also give: \
    its category; \
    every location it involves, each with the file path relative to the repository root and the line range, the place to comment on first; \
    the problem, what is wrong in one or two sentences; \
    the failure scenario, a concrete input or state and the wrong result it produces; \
    the evidence, how you established it, such as the files you read and the commands you ran; \
    confirmed, true only when you verified it by reading or running the code and false when it is likely but unverified; \
    a suggested fix; doneWhen, the test or check that would show it is fixed; \
    reviewerNotes, anything your review said about the finding that fits no other field; \
    and how confident you are from 0 to 1. \
    Use null for a field your review has nothing for, and don't invent detail it didn't give. \
    List what you could not read, run or check in notChecked. \
    Set completed to false only when the review could not read or check the change, for example because a command it needed was refused, \
    and then say why in the summary. \
    Use verdict \"clean\" only when you reported no findings and no earlier finding is still_open or declined_disputed. ";

const CODEX_PRIOR_PROMPT: &str = "If your review classified earlier findings, list each in priorFindings with its ID, status and note; otherwise use an empty list.";

/// Codex's extraction prompt.
fn extraction_prompt() -> String {
    format!("{EXTRACTION_PROMPT}{FORM_PROMPT}{CODEX_PRIOR_PROMPT}")
}

const PRIOR_STATUSES: [(&str, PriorStatus); 4] = [
    ("fixed", PriorStatus::Fixed),
    ("still_open", PriorStatus::StillOpen),
    ("declined_accepted", PriorStatus::DeclinedAccepted),
    ("declined_disputed", PriorStatus::DeclinedDisputed),
];

/// The form every reviewer restates its findings in. Every field is
/// required, with null for an empty one, as strict schemas need.
pub(super) fn output_schema() -> Value {
    let text = json!({"type": "string"});
    let optional = json!({"type": ["string", "null"]});
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["verdict", "summary", "completed", "findings", "notChecked", "priorFindings"],
        "properties": {
            "verdict": {"type": "string", "enum": ["clean", "changes_requested"]},
            "summary": text,
            "completed": {"type": "boolean"},
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "title", "priority", "category", "locations", "problem", "failureScenario",
                        "evidence", "confirmed", "suggestedFix", "doneWhen", "reviewerNotes", "confidence",
                    ],
                    "properties": {
                        "title": text,
                        "priority": {"type": "integer", "enum": [0, 1, 2, 3]},
                        "category": {"type": "string", "enum": Category::ALL.map(Category::key)},
                        "locations": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["path", "startLine", "endLine"],
                                "properties": {
                                    "path": text,
                                    "startLine": {"type": "integer"},
                                    "endLine": {"type": "integer"},
                                },
                            },
                        },
                        "problem": text,
                        "failureScenario": optional,
                        "evidence": optional,
                        "confirmed": {"type": "boolean"},
                        "suggestedFix": optional,
                        "doneWhen": optional,
                        "reviewerNotes": optional,
                        "confidence": {"type": "number"},
                    },
                },
            },
            "notChecked": {"type": "array", "items": text},
            "priorFindings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "status", "note"],
                    "properties": {
                        "id": text,
                        "status": {"type": "string", "enum": PRIOR_STATUSES.map(|(name, _)| name)},
                        "note": text,
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
    pub completed: bool,
    pub findings: Vec<Finding>,
    pub not_checked: Vec<String>,
    pub prior_findings: Vec<PriorFinding>,
}
impl Extracted {
    /// Saves the answer on its review, numbering the findings from
    /// `first_number`.
    pub(super) fn store(mut self, r: &mut Review, first_number: u32) {
        for (f, number) in self.findings.iter_mut().zip(first_number..) {
            f.number = Some(number);
        }
        r.verdict = Some(self.verdict);
        r.summary = Some(self.summary);
        r.completed = Some(self.completed);
        r.findings = Some(self.findings);
        r.not_checked = self.not_checked;
        r.prior_findings = self.prior_findings;
        r.report_findings = report_count(r.provider, &r.report);
    }
}

/// The number for the first short ID of review `id`'s findings: one past the
/// highest number any other review of its pull request gave, so a short ID
/// is never reused within a pull request.
pub(super) fn next_number(s: &Snapshot, id: &str) -> u32 {
    let Some(pr_id) = s.reviews.iter().find(|r| r.id == id).map(|r| &r.pr_id) else {
        return 1;
    };
    s.reviews
        .iter()
        .filter(|r| &r.pr_id == pr_id)
        .flat_map(|r| {
            // This review's own earlier findings count, but not the findings
            // being numbered.
            let own = (r.id != id).then_some(r.findings.iter().flatten());
            own.into_iter()
                .flatten()
                .chain(r.previous_round.iter().flat_map(|p| &p.findings))
        })
        .filter_map(|f| f.number)
        .max()
        .unwrap_or(0)
        + 1
}

/// How many findings a reviewer's report lists, when Orbit can tell: Codex
/// titles each one `[P1] …`, Orbit's instructions ask Antigravity for a
/// `### Problem 1: …` heading each, and Orbit writes Claude's list as "Claude's
/// review found 3 issues". A Claude answer in plain words can't be counted.
pub(crate) fn report_count(provider: Provider, report: &str) -> Option<u32> {
    // A line's text after any list, heading or bold markers.
    let text = |line: &str| {
        line.trim_start()
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['-', '*', '#', '.', ')', ' ', '_'])
            .to_string()
    };
    let count =
        |pred: &dyn Fn(&str) -> bool| report.lines().filter(|l| pred(&text(l))).count() as u32;
    match provider {
        Provider::Codex => Some(count(&|t| {
            let b = t.as_bytes();
            b.len() > 3 && t.starts_with("[P") && (b'0'..=b'3').contains(&b[2]) && b[3] == b']'
        })),
        Provider::Antigravity => Some(count(&|t| {
            t.strip_prefix("Problem ").is_some_and(|rest| {
                let digits = rest.chars().take_while(char::is_ascii_digit).count();
                digits > 0 && rest[digits..].starts_with(':')
            })
        })),
        Provider::Claude => {
            let rest = report.trim_start().strip_prefix("Claude's review found ")?;
            if rest.starts_with("no issues") {
                return Some(0);
            }
            rest.split_once(' ')?.0.parse().ok()
        }
    }
}

/// Validates the reviewer's JSON. Anything that does not match the schema is
/// refused as a whole, leaving the run unstructured. Classifications of
/// findings the review was not given (`previous`) are ignored.
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
    // A blank optional field is the same as null.
    let optional = |v: &Value, key: &str| match &v[key] {
        Value::Null => Ok(None),
        Value::String(t) => Ok(Some(t.trim().to_owned()).filter(|t| !t.is_empty())),
        _ => Err(format!("'{key}' must be a string or null.")),
    };
    let boolean = |v: &Value, key: &str| {
        v[key]
            .as_bool()
            .ok_or_else(|| format!("'{key}' must be true or false."))
    };
    let line = |v: &Value, key: &str| {
        v[key]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("'{key}' must be a line number."))
    };
    let location = |l: &Value| -> Result<Location, String> {
        let start_line = line(l, "startLine")?;
        let end_line = line(l, "endLine")?;
        if end_line < start_line {
            return Err("'endLine' must not be before 'startLine'.".into());
        }
        Ok(Location {
            path: relative_path(&string(l, "path")?, worktree),
            start_line,
            end_line,
        })
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
            let category = Category::ALL
                .into_iter()
                .find(|c| f["category"] == c.key())
                .ok_or("'category' must be one of the listed categories.")?;
            let mut locations = f["locations"]
                .as_array()
                .ok_or("'locations' must be a list.")?
                .iter()
                .map(location)
                .collect::<Result<Vec<_>, String>>()?;
            if locations.is_empty() {
                return Err("'locations' must list at least one place.".into());
            }
            let first = locations.remove(0);
            let mut others: Vec<Location> = vec![];
            for l in locations {
                if l != first && !others.contains(&l) {
                    others.push(l);
                }
            }
            let title = string(f, "title")?;
            Ok(Finding {
                id: format!("{review_id}-{index}"),
                title: strip_priority(&title).into(),
                priority,
                path: first.path,
                start_line: first.start_line,
                end_line: first.end_line,
                body: string(f, "problem")?,
                confidence,
                category: Some(category),
                other_locations: others,
                failure_scenario: optional(f, "failureScenario")?,
                evidence: optional(f, "evidence")?,
                confirmed: Some(boolean(f, "confirmed")?),
                suggested_fix: optional(f, "suggestedFix")?,
                done_when: optional(f, "doneWhen")?,
                reviewer_notes: optional(f, "reviewerNotes")?,
                number: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let not_checked = v["notChecked"]
        .as_array()
        .ok_or("'notChecked' must be a list.")?
        .iter()
        .map(|t| {
            t.as_str()
                .map(str::to_owned)
                .ok_or("'notChecked' must list strings.")
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
        completed: boolean(&v, "completed")?,
        findings,
        not_checked,
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
        if r.provider != Provider::Codex {
            return self.start_cli_extraction(id).await;
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
                    "input": [{"type":"text","text":extraction_prompt()}],
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
