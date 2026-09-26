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
    let mut comments = vec![];
    let mut other = vec![];
    for f in findings {
        let reason = if inline {
            placement(f, hunks)
        } else {
            Some("GitHub did not accept a comment on these lines")
        };
        match reason {
            Some(reason) => other.push((f, reason)),
            None => {
                let mut c = json!({
                    "path": f.path,
                    "line": f.end_line,
                    "side": "RIGHT",
                    "body": comment_body(f),
                });
                if f.start_line != f.end_line {
                    c["start_line"] = json!(f.start_line);
                    c["start_side"] = json!("RIGHT");
                }
                comments.push(c);
            }
        }
    }
    Plan {
        body: review_body(r, round, &other),
        comments,
    }
}
fn title(f: &Finding) -> String {
    format!("**[P{}] {}**", f.priority, f.title.trim())
}
/// An inline comment. The hidden marker maps GitHub's comment back to the
/// finding, so replies in its thread can be matched.
pub(crate) fn comment_body(f: &Finding) -> String {
    format!(
        "{}\n\n{}\n\n<!-- orbit-finding:{} -->",
        title(f),
        f.body.trim(),
        f.id
    )
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
fn review_body(r: &Review, round: Option<u32>, other: &[(&Finding, &str)]) -> String {
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
        "<!-- orbit-review:{} -->{}\n## Orbit · Codex review\n\n\
         Reviewed head `{}` against base `{}`.\n\nVerdict: {verdict}\n\n",
        r.id,
        crate::github::verdict_marker(r, round),
        r.head_sha,
        r.base_sha
    );
    if let Some(summary) = r.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("{}\n\n", summary.trim()));
    }
    if let Some(previous) = &r.previous_round {
        out.push_str(&format!(
            "### Since the last round (head `{}`)\n\n| Status | Earlier finding | Note |\n| --- | --- | --- |\n",
            &previous.head_sha[..previous.head_sha.len().min(7)]
        ));
        for f in &previous.findings {
            let prior = r.prior_findings.iter().find(|p| p.id == f.id);
            out.push_str(&format!(
                "| {} | P{} {} | {} |\n",
                prior.map_or("not reported", |p| p.status.label()),
                f.priority,
                cell(&f.title),
                prior.map_or(String::new(), |p| cell(&p.note))
            ));
        }
        out.push('\n');
    }
    if !other.is_empty() {
        out.push_str("### Other findings\n\n");
        for (f, reason) in other {
            out.push_str(&format!(
                "{} · {} · not inline: {reason}\n\n{}\n\n",
                title(f),
                location(f),
                f.body.trim()
            ));
        }
    }
    if !r.tests_not_run.is_empty() {
        out.push_str(&format!("Not run: {}\n\n", r.tests_not_run.join("; ")));
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
