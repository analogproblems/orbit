//! Where each finding goes when a review is published as a GitHub PR review:
//! inline on the lines it names, or in the review body. GitHub rejects the
//! whole review if any inline comment targets a line outside the diff.
use crate::types::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// GitHub's limit is 65,536 characters; Orbit keeps a margin, as it does for
/// issue comments.
pub(crate) const MAX_BODY: usize = 60000;

/// Line ranges of each changed file that GitHub accepts comments on (the
/// RIGHT side of each diff hunk), or None when GitHub sent no patch for the
/// file (binary or too large).
pub(crate) type Hunks = HashMap<String, Option<Vec<(u32, u32)>>>;

/// Reads the RIGHT-side ranges of each file in `GET pulls/{n}/files`.
pub(crate) fn hunks(files: &Value) -> Hunks {
    files
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let name = f["filename"].as_str()?;
            let ranges = f["patch"].as_str().map(|patch| {
                patch
                    .lines()
                    .filter_map(hunk_range)
                    .filter(|(start, end)| start <= end)
                    .collect()
            });
            Some((name.to_string(), ranges))
        })
        .collect()
}
/// The new-file lines a hunk header covers: `@@ -a,b +c,d @@` is lines c to
/// c+d-1, where a missing d means one line and d = 0 means none.
fn hunk_range(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let right = rest.split_once(" +")?.1.split_once(" @@")?.0;
    let (start, count) = match right.split_once(',') {
        Some((start, count)) => (start.parse::<u32>().ok()?, count.parse::<u32>().ok()?),
        None => (right.parse().ok()?, 1),
    };
    // A zero count yields an empty range, dropped by the caller.
    Some((start.max(1), (start + count).saturating_sub(1)))
}

/// What the publish dialog shows: each finding's placement, and the review
/// text as it will be posted.
pub(crate) fn preview(r: &Review, hunks: &Hunks, round: Option<u32>) -> PublishPlan {
    let placements = r
        .findings
        .iter()
        .flatten()
        .map(|f| {
            let reason = placement(f, hunks);
            Placement {
                finding_id: f.id.clone(),
                inline: reason.is_none(),
                reason: reason.map(str::to_owned),
            }
        })
        .collect();
    PublishPlan {
        placements,
        body: plan(r, hunks, round, true).body,
    }
}
/// Why a finding cannot be an inline comment, or None when it can.
pub(crate) fn placement(f: &Finding, hunks: &Hunks) -> Option<&'static str> {
    let Some(path) = &f.path else {
        return Some("outside the repository");
    };
    let Some(ranges) = hunks.get(path) else {
        return Some("file not changed in this pull request");
    };
    let Some(ranges) = ranges else {
        return Some("diff too large to show");
    };
    if f.start_line == 0 || f.end_line < f.start_line {
        return Some("lines not in the diff");
    }
    let inside = |line: u32| ranges.iter().position(|&(a, b)| a <= line && line <= b);
    match (inside(f.start_line), inside(f.end_line)) {
        (Some(a), Some(b)) if a == b => None,
        (Some(_), Some(_)) => Some("spans more than one part of the diff"),
        _ => Some("lines not in the diff"),
    }
}

/// What a PR review for a structured run posts.
#[derive(Debug)]
pub(crate) struct Plan {
    pub body: String,
    /// `comments[]` for `POST pulls/{n}/reviews`.
    pub comments: Vec<Value>,
}

/// Builds the review. With `inline` false every finding goes in the body,
/// the fallback when GitHub refuses the inline comments.
pub(crate) fn plan(r: &Review, hunks: &Hunks, round: Option<u32>, inline: bool) -> Plan {
    let findings = r.findings.as_deref().unwrap_or_default();
    let placed: Vec<(&Finding, Option<&str>)> = findings
        .iter()
        .map(|f| {
            let reason = if inline {
                placement(f, hunks)
            } else {
                Some("GitHub did not accept a comment on these lines")
            };
            (f, reason)
        })
        .collect();
    let comments = placed
        .iter()
        .filter(|(_, reason)| reason.is_none())
        .map(|(f, _)| {
            let mut c = json!({
                "path": f.path,
                "line": f.end_line,
                "side": "RIGHT",
                "body": comment_body(f, r.blocking_priority),
            });
            if f.start_line != f.end_line {
                c["start_line"] = json!(f.start_line);
                c["start_side"] = json!("RIGHT");
            }
            c
        })
        .collect();
    Plan {
        body: review_body(r, round, &placed),
        comments,
    }
}
/// An inline comment: the heading, the problem and the rest of the form. The
/// hidden marker maps GitHub's comment back to the finding, so replies in its
/// thread can be matched.
pub(crate) fn comment_body(f: &Finding, blocking_priority: Option<u8>) -> String {
    format!(
        "{}\n\n{}\n\n<!-- orbit-finding:{} -->",
        f.heading(blocking_priority),
        explanation(f),
        f.id
    )
}
/// The problem, then the rest of the finding's form.
fn explanation(f: &Finding) -> String {
    let details = f.detail_lines();
    if details.is_empty() {
        f.body.trim().to_string()
    } else {
        format!("{}\n\n{}", f.body.trim(), details.join("\n"))
    }
}
/// The finding ID in an inline comment Orbit posted.
pub(crate) fn finding_marker(body: &str) -> Option<&str> {
    let rest = body.split("<!-- orbit-finding:").nth(1)?;
    Some(rest.split_once(" -->")?.0)
}
fn location(f: &Finding) -> String {
    let Some(path) = &f.path else {
        return "outside the repository".into();
    };
    if f.start_line == f.end_line {
        format!("`{path}:{}`", f.start_line)
    } else {
        format!("`{path}:{}-{}`", f.start_line, f.end_line)
    }
}
/// What an agent can read instead of the Markdown: the whole form of every
/// finding, as JSON in an HTML comment. `<`, `>` and `&` are escaped so the
/// text can't end the comment.
fn data(r: &Review, round: Option<u32>, placed: &[(&Finding, Option<&str>)]) -> String {
    let findings: Vec<Value> = placed
        .iter()
        .map(|(f, reason)| {
            let locations: Vec<Location> = std::iter::once(f.location())
                .chain(f.other_locations.iter().cloned())
                .collect();
            json!({
                "id": f.id,
                "shortId": f.short_id(),
                "title": f.title.trim(),
                "priority": f.priority,
                "blocking": f.blocking(r.blocking_priority),
                "category": f.category,
                "locations": locations,
                "problem": f.body.trim(),
                "failureScenario": f.failure_scenario,
                "evidence": f.evidence,
                "confirmed": f.confirmed,
                "suggestedFix": f.suggested_fix,
                "doneWhen": f.done_when,
                "reviewerNotes": f.reviewer_notes,
                "confidence": f.confidence,
                "inline": reason.is_none(),
                "notInlineBecause": reason,
            })
        })
        .collect();
    let earlier: Vec<Value> = r
        .previous_round
        .iter()
        .flat_map(|p| &p.findings)
        .map(|f| {
            let prior = r.prior_findings.iter().find(|p| p.id == f.id);
            json!({
                "id": f.id,
                "shortId": f.short_id(),
                "title": f.title.trim(),
                "priority": f.priority,
                "status": prior.map(|p| p.status),
                "note": prior.map(|p| p.note.as_str()),
            })
        })
        .collect();
    let v = json!({
        "version": 1,
        "review": r.id,
        "round": round,
        "reviewer": r.provider,
        "head": r.head_sha,
        "base": r.base_sha,
        "verdict": r.orbit_verdict.or(r.verdict),
        "blocking": r.blocking,
        "blockingPriority": r.blocking_priority,
        "completed": r.completed,
        "notChecked": r.not_checked,
        "reportFindings": r.report_findings,
        "findings": findings,
        "earlierFindings": earlier,
    });
    let text = v
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!("<!-- orbit-data {text} -->\n")
}
/// The review's text, kept within GitHub's limit: the hidden data is left
/// out first, then the reviewer's report, then both.
fn review_body(r: &Review, round: Option<u32>, placed: &[(&Finding, Option<&str>)]) -> String {
    let mut body = String::new();
    for (with_data, with_report) in [(true, true), (false, true), (true, false), (false, false)] {
        body = compose(r, round, placed, with_data, with_report);
        if body.len() <= MAX_BODY {
            break;
        }
    }
    body
}
fn compose(
    r: &Review,
    round: Option<u32>,
    placed: &[(&Finding, Option<&str>)],
    with_data: bool,
    with_report: bool,
) -> String {
    let cell = |text: &str| text.replace('|', "\\|").replace(['\r', '\n'], " ");
    let verdict = match r.orbit_verdict.or(r.verdict) {
        Some(Verdict::Clean) => "clean".to_string(),
        Some(Verdict::ChangesRequested) => match r.blocking {
            Some(n) if n > 0 => format!(
                "changes requested ({n} blocking finding{})",
                if n == 1 { "" } else { "s" }
            ),
            _ => "changes requested".into(),
        },
        None => "not reported".into(),
    };
    let mut out = format!(
        "<!-- orbit-review:{} -->{}\n## Orbit · {} review\n\n\
         Reviewed head `{}` against base `{}`.\n\nVerdict: {verdict}\n\n",
        r.id,
        crate::github::verdict_marker(r, round),
        r.provider.label(),
        r.head_sha,
        r.base_sha
    );
    if let Some(summary) = r.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("{}\n\n", summary.trim()));
    }
    if let Some(note) = r.count_note() {
        out.push_str(&format!("**Note:** {note}\n\n"));
    }
    if let Some(previous) = &r.previous_round {
        out.push_str(&format!(
            "### Since the last round (head `{}`)\n\n| Status | Earlier finding | Note |\n| --- | --- | --- |\n",
            &previous.head_sha[..previous.head_sha.len().min(7)]
        ));
        for f in &previous.findings {
            let prior = r.prior_findings.iter().find(|p| p.id == f.id);
            let id = f.short_id().map(|s| format!("{s} · ")).unwrap_or_default();
            out.push_str(&format!(
                "| {} | {id}P{} {} | {} |\n",
                prior.map_or("not reported", |p| p.status.label()),
                f.priority,
                cell(&f.title),
                prior.map_or(String::new(), |p| cell(&p.note))
            ));
        }
        out.push('\n');
    }
    let other: Vec<(&Finding, &str)> = placed
        .iter()
        .filter_map(|(f, reason)| reason.map(|reason| (*f, reason)))
        .collect();
    if !other.is_empty() {
        out.push_str("### Other findings\n\n");
        for (f, reason) in other {
            out.push_str(&format!(
                "{} · {} · not inline: {reason}\n\n{}\n\n",
                f.heading(r.blocking_priority),
                location(f),
                explanation(f)
            ));
        }
    }
    if !r.not_checked.is_empty() {
        out.push_str(&format!("Not checked: {}\n\n", r.not_checked.join("; ")));
    }
    for note in &r.declined_approvals {
        out.push_str(&format!("{note}\n\n"));
    }
    let report = r.report.trim();
    if !report.is_empty() {
        let summary = format!("{}’s own report", r.provider.label());
        if with_report {
            out.push_str(&format!(
                "<details>\n<summary>{summary}</summary>\n\n{report}\n\n</details>\n\n"
            ));
        } else {
            out.push_str(&format!(
                "{summary} is too long to include here; it is kept in Orbit.\n\n"
            ));
        }
    }
    if with_data {
        out.push_str(&data(r, round, placed));
    }
    out.push_str(&format!(
        "---\n{}",
        crate::models::report_selection_label(r)
    ));
    out
}
/// GitHub's size limits, checked before anything is sent.
pub(crate) fn check_size(plan: &Plan) -> Result<(), String> {
    if plan.body.len() > MAX_BODY {
        return Err(
            "The review body exceeds GitHub’s size limit. Copy the report from the chat instead."
                .into(),
        );
    }
    if plan
        .comments
        .iter()
        .any(|c| c["body"].as_str().is_some_and(|b| b.len() > MAX_BODY))
    {
        return Err(
            "A finding exceeds GitHub’s comment size limit. Copy the report from the chat instead."
                .into(),
        );
    }
    Ok(())
}
/// Each inline comment's GitHub ID, matched to its finding by marker.
pub(crate) fn inline_comments(items: &[Value]) -> Vec<InlineComment> {
    items
        .iter()
        .filter_map(|c| {
            Some(InlineComment {
                finding_id: finding_marker(c["body"].as_str()?)?.to_string(),
                comment_id: c["id"].as_u64()?,
            })
        })
        .collect()
}
