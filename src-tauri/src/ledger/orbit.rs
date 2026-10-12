//! Orbit's own published review, read back: a GitHub pull request review (or
//! conversation comment) starting `<!-- orbit-review:ID -->`, with
//! `<!-- orbit-verdict: … -->` and, unless it was left out for size, the whole
//! review as JSON in `<!-- orbit-data {…} -->` (see `engine/inline.rs`).
//! Inline findings are comments on the changed lines ending in
//! `<!-- orbit-finding:ID -->`. The JSON is preferred; the Markdown is the
//! fallback.
use super::*;
use regex::Regex;
use std::sync::LazyLock;

static REVIEW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--\s*orbit-review:([^\s>]+)\s*-->").unwrap());
static FINDING_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--\s*orbit-finding:([^\s>]+)\s*-->").unwrap());
static VERDICT_MARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"<!--\s*orbit-verdict:\s*(clean|changes_requested)(?:\s+round=(\d+))?\s+blocking=(\d+)\s*-->",
    )
    .unwrap()
});
static DATA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--\s*orbit-data\s+(.*?)\s*-->").unwrap());
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Reviewed head `([0-9a-f]{7,40})` against base `([0-9a-f]{7,40})`").unwrap()
});
static REVIEWER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^##\s*Orbit\s*·\s*(.+?)\s+review\s*$").unwrap());
static VERDICT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^Verdict:\s*(clean|changes requested(?:\s*\((\d+) blocking findings?\))?|not reported)",
    )
    .unwrap()
});
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\*\*([^*]+)\*\*\s*(.*)$").unwrap());
static DETAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\*\*(Breaks when|Evidence|Suggested fix|Done when|Also at|Reviewer'?s notes):\*\*\s*(.*)$",
    )
    .unwrap()
});
static TICKED_PLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`:]+):(\d+)(?:-(\d+))?`").unwrap());
static SINCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)### Since the last round[^\n]*\n\n\|[^\n]*\n\|[^\n]*\n(.*?)(?:\n\n|\z)")
        .unwrap()
});
static EARLIER_CELL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(F\d+)\s*·\s*)?P([0-3])\s+(.*)$").unwrap());
static OTHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)### Other findings\n\n(.*?)(?:\n_To answer|\nNot checked:|\n<details>|\n<!--|\n---\n|\z)",
    )
    .unwrap()
});
static OTHER_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\*\*F?\d*[^*]*P[0-3]").unwrap());
static TRAILING_PLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*·\s*(?:`[^`]*`|outside the repository)\s*$").unwrap());
static NOT_CHECKED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^Not checked:\s*(.+)$").unwrap());
static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([0-9a-f]{7,40})\b").unwrap());

pub(super) fn is_review(body: &str) -> bool {
    REVIEW.is_match(body)
}
/// The head commit a review names.
pub(super) fn head(body: &str) -> Option<String> {
    data(body)
        .and_then(|d| opt(&d["head"]))
        .or_else(|| HEADER.captures(body).map(|h| h[1].to_string()))
}
/// The finding ID in one of Orbit's inline comments.
pub(super) fn finding_marker(body: &str) -> Option<String> {
    FINDING_MARK.captures(body).map(|c| c[1].to_string())
}

fn category(v: &Value) -> Option<Category> {
    let key = v.as_str()?.to_lowercase().replace(' ', "_");
    Category::ALL.into_iter().find(|c| c.key() == key)
}
fn category_label(label: &str) -> Option<Category> {
    Category::ALL
        .into_iter()
        .find(|c| c.label().eq_ignore_ascii_case(label.trim()))
}
pub(super) fn status(text: &str) -> Option<PriorStatus> {
    let key = text.trim().to_lowercase();
    if key.starts_with("fixed") {
        Some(PriorStatus::Fixed)
    } else if key.starts_with("still") {
        Some(PriorStatus::StillOpen)
    } else if key.starts_with("declined") && key.contains("disput") {
        Some(PriorStatus::DeclinedDisputed)
    } else if key.starts_with("declined") {
        Some(PriorStatus::DeclinedAccepted)
    } else {
        None
    }
}
fn opt(v: &Value) -> Option<String> {
    v.as_str().and_then(some)
}
fn priority(v: &Value) -> u8 {
    v.as_u64().filter(|p| *p <= 3).unwrap_or(3) as u8
}
fn place(v: &Value) -> Place {
    Place {
        path: opt(&v["path"]),
        start_line: v["startLine"].as_u64().unwrap_or(0) as u32,
        end_line: v["endLine"].as_u64().unwrap_or(0) as u32,
    }
}

/// A round from one of Orbit's reviews, or None if it isn't one.
pub(super) fn parse_review(c: &CommentRef) -> Option<Round> {
    let body = &c.body;
    if !is_review(body) {
        return None;
    }
    let marker = VERDICT_MARK.captures(body);
    let header = HEADER.captures(body);
    let mut round = Round {
        review_id: REVIEW.captures(body).map(|m| m[1].to_string()),
        format: Format::Orbit,
        round: marker
            .as_ref()
            .and_then(|m| m.get(2)?.as_str().parse().ok()),
        head_sha: header.as_ref().map(|h| h[1].to_string()),
        base_sha: header.as_ref().map(|h| h[2].to_string()),
        verdict: match marker.as_ref().map(|m| &m[1]) {
            Some("clean") => RoundVerdict::Clean,
            Some(_) => RoundVerdict::ChangesRequested,
            None => RoundVerdict::Unknown,
        },
        blocking: marker.as_ref().and_then(|m| m[3].parse().ok()),
        blocking_priority: None,
        reviewer: REVIEWER.captures(body).map(|m| m[1].to_string()),
        summary: summary(body),
        findings: vec![],
        earlier_findings: vec![],
        not_checked: vec![],
        completed: None,
        comment_id: c.id,
        url: c.url.clone(),
        created_at: c.created_at.clone(),
        body: body.clone(),
    };
    match data(body) {
        Some(d) => from_data(&mut round, &d),
        None => from_markdown(&mut round, body),
    }
    Some(round)
}

/// The hidden JSON, with Orbit's `<`-style escapes undone by parsing.
fn data(body: &str) -> Option<Value> {
    let text = DATA.captures(body)?.get(1)?.as_str();
    serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_object)
}
fn from_data(round: &mut Round, d: &Value) {
    if let Some(id) = opt(&d["review"]) {
        round.review_id = Some(id);
    }
    if let Some(n) = d["round"].as_u64() {
        round.round = Some(n as u32);
    }
    if let Some(head) = opt(&d["head"]) {
        round.head_sha = Some(head);
    }
    if let Some(base) = opt(&d["base"]) {
        round.base_sha = Some(base);
    }
    match d["verdict"].as_str() {
        Some("clean") => round.verdict = RoundVerdict::Clean,
        Some("changes_requested") => round.verdict = RoundVerdict::ChangesRequested,
        _ => {}
    }
    if let Some(n) = d["blocking"].as_u64() {
        round.blocking = Some(n as u32);
    }
    round.blocking_priority = d["blockingPriority"].as_u64().map(|p| p.min(3) as u8);
    // The data names the reviewer by its key, such as `codex`.
    if let Ok(provider) = serde_json::from_value::<crate::types::Provider>(d["reviewer"].clone()) {
        round.reviewer = Some(provider.label().to_string());
    }
    round.completed = d["completed"].as_bool();
    round.not_checked = d["notChecked"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    round.findings = d["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, f)| finding_from_data(f, i))
        .collect();
    round.earlier_findings = d["earlierFindings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Earlier {
            id: opt(&e["id"]).unwrap_or_default(),
            short_id: opt(&e["shortId"]),
            title: text(e, "title"),
            priority: e["priority"].as_u64().map(|p| p.min(3) as u8),
            status: e["status"].as_str().and_then(status),
            note: opt(&e["note"]),
            author_said: e["authorSaid"].is_object().then(|| AuthorSaid {
                kind: if e["authorSaid"]["kind"] == "declined" {
                    ClaimKind::Declined
                } else {
                    ClaimKind::Fixed
                },
                commit: opt(&e["authorSaid"]["commit"]),
                reason: opt(&e["authorSaid"]["reason"]),
            }),
        })
        .collect();
}
fn finding_from_data(f: &Value, index: usize) -> Finding {
    let short_id = opt(&f["shortId"]);
    let mut finding = Finding::new(
        opt(&f["id"]).unwrap_or_else(|| format!("orbit:{index}")),
        opt(&f["title"]).unwrap_or_else(|| "(untitled)".into()),
        priority(&f["priority"]),
    );
    finding.number = short_id
        .as_deref()
        .and_then(|s| s.get(1..))
        .and_then(|n| n.parse().ok());
    finding.short_id = short_id;
    finding.blocking = f["blocking"].as_bool();
    finding.category = category(&f["category"]);
    finding.locations = f["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(place)
        .collect();
    finding.problem = text(f, "problem").trim().to_string();
    finding.failure_scenario = opt(&f["failureScenario"]);
    finding.evidence = opt(&f["evidence"]);
    finding.confirmed = f["confirmed"].as_bool();
    finding.suggested_fix = opt(&f["suggestedFix"]);
    finding.done_when = opt(&f["doneWhen"]);
    finding.reviewer_notes = opt(&f["reviewerNotes"]);
    finding.confidence = f["confidence"].as_f64();
    finding.inline = f["inline"].as_bool();
    finding
}

/// The paragraph after the `Verdict:` line, when there is one before the
/// review's sections.
fn summary(body: &str) -> Option<String> {
    let start = body
        .find("\nVerdict:")
        .map(|i| i + 1)
        .or_else(|| body.starts_with("Verdict:").then_some(0))?;
    let rest = &body[start..];
    let after = rest.split_once("\n\n")?.1;
    let paragraph = after.split("\n\n").next()?.trim();
    let section = [
        "#",
        "|",
        "**Note:**",
        "_To answer",
        "Not checked:",
        "<details>",
        "<!--",
        "---",
    ];
    if paragraph.is_empty() || section.iter().any(|s| paragraph.starts_with(s)) {
        return None;
    }
    Some(paragraph.to_string())
}

/// A finding's bold first line: `**F2 · P1 · Blocking · Bug · Likely, not
/// verified** Title`.
pub(super) fn heading(line: &str) -> Option<Finding> {
    let m = HEADING.captures(line.trim())?;
    let parts: Vec<&str> = m[1].split('·').map(str::trim).collect();
    let priority = parts.iter().find_map(|p| {
        let p = p.strip_prefix('P').or_else(|| p.strip_prefix('p'))?;
        p.parse::<u8>().ok().filter(|n| *n <= 3 && p.len() == 1)
    })?;
    let mut f = Finding::new(String::new(), m[2].trim().to_string(), priority);
    f.short_id = parts
        .iter()
        .find(|p| {
            p.strip_prefix(['F', 'f'])
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|p| p.to_ascii_uppercase());
    f.number = f.short_id.as_deref().and_then(|s| s[1..].parse().ok());
    f.blocking = if parts.contains(&"Blocking") {
        Some(true)
    } else if parts.contains(&"Not blocking") {
        Some(false)
    } else {
        None
    };
    f.category = parts.iter().find_map(|p| category_label(p));
    if parts
        .iter()
        .any(|p| p.eq_ignore_ascii_case("likely, not verified"))
    {
        f.confirmed = Some(false);
    }
    Some(f)
}

/// The labelled lines under a finding's problem: **Breaks when:**,
/// **Evidence:**, **Suggested fix:**, **Done when:**, **Also at:**,
/// **Reviewer's notes:**. Fills them in, and the problem from the other
/// lines.
fn details(f: &mut Finding, text: &str) {
    let mut problem = vec![];
    for raw in text.lines() {
        let line = raw.trim();
        let Some(m) = DETAIL.captures(line) else {
            // `_Bug · Confirmed_`-style kind lines repeat the heading.
            let kind_line = line.len() > 1 && line.starts_with('_') && line.ends_with('_');
            if !line.is_empty() && !kind_line {
                problem.push(line);
            }
            continue;
        };
        let value = m[2].trim().to_string();
        match &m[1] {
            "Breaks when" => f.failure_scenario = Some(value),
            "Evidence" => f.evidence = Some(value),
            "Suggested fix" => f.suggested_fix = Some(value),
            "Done when" => f.done_when = Some(value),
            "Also at" => f
                .locations
                .extend(TICKED_PLACE.captures_iter(&value).map(|l| {
                    let start = l[2].parse().unwrap_or(0);
                    Place {
                        path: Some(l[1].to_string()),
                        start_line: start,
                        end_line: l
                            .get(3)
                            .and_then(|e| e.as_str().parse().ok())
                            .unwrap_or(start),
                    }
                })),
            _ => f.reviewer_notes = Some(value),
        }
    }
    f.problem = problem.join("\n").trim().to_string();
}

/// One of Orbit's inline finding comments, at the lines it was posted on.
pub(super) fn inline_finding(c: &CommentRef) -> Option<Finding> {
    let id = finding_marker(&c.body)?;
    let text = FINDING_MARK.replace(&c.body, "");
    let text = text.trim();
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let mut f = heading(first)?;
    f.id = id;
    if let Some(path) = &c.path {
        let end = c.line.or(c.start_line).unwrap_or(0);
        f.locations.push(Place {
            path: Some(path.clone()),
            start_line: c.start_line.or(c.line).unwrap_or(0),
            end_line: end,
        });
    }
    details(&mut f, rest);
    f.inline = Some(true);
    Some(f)
}

/// Splits a table row on its unescaped `|`.
fn cells(row: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.peek() == Some(&'|') => {
                out.last_mut().unwrap().push('|');
                chars.next();
            }
            '|' => out.push(String::new()),
            _ => out.last_mut().unwrap().push(ch),
        }
    }
    out.into_iter().map(|c| c.trim().to_string()).collect()
}

/// What the Markdown alone gives when the hidden data was left out.
fn from_markdown(round: &mut Round, body: &str) {
    if let Some(v) = VERDICT_LINE.captures(body) {
        if &v[1] == "clean" {
            round.verdict = RoundVerdict::Clean;
            round.blocking = Some(0);
        } else if v[1].starts_with("changes") {
            round.verdict = RoundVerdict::ChangesRequested;
            if let Some(n) = v.get(2).and_then(|n| n.as_str().parse().ok()) {
                round.blocking = Some(n);
            }
        }
    }
    if let Some(since) = SINCE.captures(body) {
        for row in since[1].lines() {
            let cells = cells(row);
            if cells.len() < 5 {
                continue;
            }
            let Some(m) = EARLIER_CELL.captures(&cells[2]) else {
                continue;
            };
            let said = &cells[3];
            round.earlier_findings.push(Earlier {
                id: m
                    .get(1)
                    .map_or_else(|| cells[2].clone(), |s| s.as_str().into()),
                short_id: m.get(1).map(|s| s.as_str().to_string()),
                title: m[3].to_string(),
                priority: m[2].parse().ok(),
                status: status(&cells[1]),
                note: some(&cells[4]),
                author_said: some(said).map(|said| AuthorSaid {
                    kind: if said.to_lowercase().contains("declin") {
                        ClaimKind::Declined
                    } else {
                        ClaimKind::Fixed
                    },
                    commit: SHA.captures(&said).map(|s| s[1].to_string()),
                    reason: Some(said),
                }),
            });
        }
    }
    if let Some(other) = OTHER.captures(body) {
        let mut blocks: Vec<Vec<&str>> = vec![];
        for line in other[1].lines() {
            if OTHER_START.is_match(line) || blocks.is_empty() {
                blocks.push(vec![]);
            }
            blocks.last_mut().unwrap().push(line);
        }
        for block in blocks {
            let block = block.join("\n");
            let block = block.trim();
            let (first, rest) = block.split_once('\n').unwrap_or((block, ""));
            let head = first.split(" · not inline:").next().unwrap_or(first);
            let head = TRAILING_PLACE.replace(head, "");
            let Some(mut f) = heading(&head) else {
                continue;
            };
            f.id = f
                .short_id
                .clone()
                .unwrap_or_else(|| format!("md:{}", round.findings.len()));
            if let Some(l) = TICKED_PLACE.captures(first) {
                let start = l[2].parse().unwrap_or(0);
                f.locations.push(Place {
                    path: Some(l[1].to_string()),
                    start_line: start,
                    end_line: l
                        .get(3)
                        .and_then(|e| e.as_str().parse().ok())
                        .unwrap_or(start),
                });
            }
            details(&mut f, rest);
            f.inline = Some(false);
            round.findings.push(f);
        }
    }
    round.not_checked = NOT_CHECKED
        .captures(body)
        .map(|m| {
            m[1].split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
}
