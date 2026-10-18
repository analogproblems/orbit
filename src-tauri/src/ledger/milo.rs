//! Milo's reviews: conversation comments headed "## Milo PR review" and,
//! earlier, plain "Reviewed `sha`." prose. Their shape drifted over time, so
//! this reader is tolerant: it recovers the verdict, the reviewed head, each
//! finding's ID, priority, title and place, and the rulings on earlier
//! findings, and leaves the prose to the write-up.
use super::*;
use regex::Regex;
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

static MARKS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"<!--\s*milo-pr-review",
        r"(?m)^##\s*Milo PR review",
        r"(?im)^(?:\*\*)?(?:re-?)?reviewed\s+(?:`[0-9a-f]{7,40}`|commit\b|head\b|the (?:latest )?update\b)",
        r"(?m)^\s*(?:\d+\.\s*)?\*\*\[P[0-3]\]",
        r"(?im)^(?:\*\*)?clear to merge\b",
        r"(?im)^(?:\*\*)?no (?:new|outstanding|actionable|remaining) findings",
    ]
    .iter()
    .map(|p| re(p))
    .collect()
});
static HEADS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bFull head:\s*`?([0-9a-f]{7,40})`?",
        r"(?im)^\s*(?:-\s*)?Head:\s*`?([0-9a-f]{7,40})`?",
        r"(?i)milo-pr-review:[^>]*?\bhead=([0-9a-f]{7,40})",
        r"(?i)milo-pr-review:[^:>]+:\d+:([0-9a-f]{7,40}):[0-9a-f]{7,40}",
        r"(?im)^(?:\*\*)?(?:re-?)?reviewed\s+(?:commit\s+|head\s+)?`([0-9a-f]{7,40})`",
        r"(?im)^(?:\*\*)?(?:re-?)?reviewed the (?:latest )?update from `[0-9a-f]{7,40}`(?: through current head| to)\s+`([0-9a-f]{7,40})`",
        r"(?i)clear to merge[^\n]{0,40}?\bat\s+`([0-9a-f]{7,40})`",
        r"/blob/([0-9a-f]{40})/",
    ]
    .iter()
    .map(|p| re(p))
    .collect()
});
static BASES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bFull base:\s*`?([0-9a-f]{7,40})`?",
        r"(?im)^\s*(?:-\s*)?Base:\s*`?([0-9a-f]{7,40})`?",
        r"(?i)milo-pr-review:[^>]*?\bbase=([0-9a-f]{7,40})",
        r"(?i)milo-pr-review:[^:>]+:\d+:[0-9a-f]{7,40}:([0-9a-f]{7,40})",
    ]
    .iter()
    .map(|p| re(p))
    .collect()
});
static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)```.*?```"));
static CLEAR_UPPER: LazyLock<Regex> = LazyLock::new(|| re(r"\bCLEAR TO MERGE\b"));
static CHANGES_UPPER: LazyLock<Regex> = LazyLock::new(|| re(r"\bCHANGES REQUESTED\b"));
/// Milo's review when its required test stage couldn't run: no verdict.
static BLOCKED: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^\**BLOCKED\b"));
static NOT_CLEAR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bnot (?:yet )?clear to merge\b"));
static CLEAR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bclear to merge\b"));
static TO_ADDRESS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bto address before merging\b"));
static ACTIONABLE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:found|there (?:are|is)|two|three|four|five|\d+|one) (?:\w+ )?actionable (?:issues?|findings?)\b",
    )
});
static NO_ACTIONABLE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\bno (?:new |remaining |other )?actionable"));
static REMAINING: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\bremaining (?:P[0-3] )?(?:issue|finding)s?\b"));
static NO_REMAINING: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bno remaining\b"));
static NO_FINDINGS: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\bno (?:new|outstanding|remaining|further) (?:actionable )?(?:code )?(?:issues|findings)\b|\bno (?:remaining )?actionable (?:code )?findings\b|\bno findings\b|\bfindings? (?:is|are) (?:resolved|addressed|fixed)\b",
    )
});
static LOC: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?:\[`?([\w./-]+\.\w+):(\d+)(?:[–-](\d+))?`?\]|`([\w./-]+\.\w+):(\d+)(?:[–-](\d+))?`|\b((?:src|lib|app|test|tests|docs)[\w./-]*\.\w+):(\d+)(?:[–-](\d+))?)",
    )
});
static BLOB: LazyLock<Regex> =
    LazyLock::new(|| re(r"/blob/[0-9a-f]{40}/([\w./-]+)#L(\d+)(?:-L(\d+))?"));
static RULING: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(?:[-*]\s*)?\*\*F(\d+)(?: remaining)?(?:\s*\(P[0-3]\))?,?\s*(fixed|partially fixed|still open|not fixed|remains open|declined[^*:]*)\b[^*]*\*\*",
    )
});
static NUMBERED: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?is)^(?:\d+\.\s*)?\*\*F(\d+)(\s+remaining)?\s*[·—:-]\s*P([0-3])\s*(?:[·—:-]\s*)?(.*?)\*\*(?:\s|$)",
    )
});
static BRACKETED: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?s)^(?:(\d+)\.\s*)?\*\*\[P([0-3])\]\s*(.*?)\*\*"));
static PROBLEM: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^#{2,4}\s*Problem\s+(\d+)\s*:\s*(.+)$"));
static LINK: LazyLock<Regex> = LazyLock::new(|| re(r"\[([^\]]+)\]\([^)]*\)"));
static TRAILING_PLACE: LazyLock<Regex> =
    LazyLock::new(|| re(r"\s+[—–-]\s+`?[\w./-]+:\d+(?:[–-]\d+)?`?\s*$"));
static EDGES: LazyLock<Regex> = LazyLock::new(|| re(r"^[\s:.\-—·]+|[\s.]+$"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
static SECTION: LazyLock<Regex> = LazyLock::new(|| re(r"^#{2,4}\s+(.+)$"));
static PROBLEM_SECTION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^#{2,4}\s*Problem\s+\d+"));
static PARAGRAPH: LazyLock<Regex> = LazyLock::new(|| re(r"\n\s*\n"));
static BREAKS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?s)\*\*Breaks when:\*\*\s*(.*?)(?:\n\s*\n|\*\*Needed fix|\*\*Suggested fix|\z)")
});
static FIX: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?s)\*\*(?:Needed|Suggested) fix:\*\*\s*(.*?)(?:\n\s*\n|\z)"));
static REPRODUCED: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\breproduced\b|\bI ran\b|\bverified by (?:running|executing)\b"));
static STATIC_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)static(?:ally)?[^.]*(?:finding|review|established|inspection)|not (?:been )?executed|no [^.]*reproduction",
    )
});
static OPTIONAL_SECTION: LazyLock<Regex> =
    LazyLock::new(|| re(r"suggestion|optional|non-?blocking|nit"));
static NON_BLOCKING: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)non-?blocking"));
static NOT_CHECKED: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:were|was|is|are) not (?:run|executed|rerun|re-run|verified|independently verified|treated as (?:observed|verified))\b|\bnot run locally\b|\bcould not (?:read|run|check|be loaded)\b|\bno (?:rust|native)[^.]*reproduction was executed\b",
    )
});
static SUMMARY_LINE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(?:I found|No actionable|No remaining|Reviewed|Re-reviewed|The three|Both findings|No code changes)",
    )
});
static MARKER: LazyLock<Regex> = LazyLock::new(|| re(r"milo-pr-review[^>]*"));
static INCOMPLETE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\bI could not (?:read|check|review)\b|\bunable to (?:read|check|review) (?:the|all|every)\b",
    )
});

pub(super) fn is_review(body: &str) -> bool {
    MARKS.iter().any(|m| m.is_match(body))
}
fn first(patterns: &[Regex], body: &str) -> Option<String> {
    patterns
        .iter()
        .find_map(|p| p.captures(body))
        .map(|c| c[1].to_ascii_lowercase())
}
/// The head commit a review names, so an edited review can be told apart
/// from a new round.
pub(super) fn head(body: &str) -> Option<String> {
    first(&HEADS, body)
}

pub(super) fn verdict(body: &str) -> RoundVerdict {
    let text = CODE.replace_all(body, "");
    if CLEAR_UPPER.is_match(&text) {
        RoundVerdict::Clean
    } else if CHANGES_UPPER.is_match(&text) {
        RoundVerdict::ChangesRequested
    } else if BLOCKED.is_match(&text) {
        RoundVerdict::Unknown
    } else if NOT_CLEAR.is_match(&text) {
        RoundVerdict::ChangesRequested
    } else if CLEAR.is_match(&text) {
        RoundVerdict::Clean
    } else if TO_ADDRESS.is_match(&text)
        || (ACTIONABLE.is_match(&text) && !NO_ACTIONABLE.is_match(&text))
        || (REMAINING.is_match(&text) && !NO_REMAINING.is_match(&text))
    {
        RoundVerdict::ChangesRequested
    } else if NO_FINDINGS.is_match(&text) {
        RoundVerdict::Clean
    } else {
        RoundVerdict::Unknown
    }
}

fn places(text: &str) -> Vec<Place> {
    let mut out = vec![];
    let num = |m: Option<regex::Match>| m.and_then(|m| m.as_str().parse::<u32>().ok());
    for m in BLOB.captures_iter(text) {
        let start = num(m.get(2)).unwrap_or(0);
        out.push(Place {
            path: Some(m[1].to_string()),
            start_line: start,
            end_line: num(m.get(3)).unwrap_or(start),
        });
    }
    for m in LOC.captures_iter(text) {
        let path = m.get(1).or(m.get(4)).or(m.get(7));
        let start = num(m.get(2).or(m.get(5)).or(m.get(8)));
        let end = num(m.get(3).or(m.get(6)).or(m.get(9)));
        if let (Some(path), Some(start)) = (path, start) {
            out.push(Place {
                path: Some(path.as_str().to_string()),
                start_line: start,
                end_line: end.unwrap_or(start),
            });
        }
    }
    out
}

#[derive(Debug, PartialEq)]
pub(super) struct Heading {
    pub number: Option<u32>,
    pub priority: u8,
    pub title: String,
    pub ruling: Option<PriorStatus>,
}

/// A finding heading in any of Milo's shapes, or a ruling on an earlier
/// finding such as "**F1 fixed:**".
pub(super) fn heading(line: &str) -> Option<Heading> {
    let t = line.trim();
    if let Some(r) = RULING.captures(t) {
        let word = r[2].to_lowercase();
        let ruling = if word == "fixed" {
            PriorStatus::Fixed
        } else if word.starts_with("declined") {
            if word.contains("disput") {
                PriorStatus::DeclinedDisputed
            } else {
                PriorStatus::DeclinedAccepted
            }
        } else {
            PriorStatus::StillOpen
        };
        return Some(Heading {
            number: r[1].parse().ok(),
            priority: 3,
            title: String::new(),
            ruling: Some(ruling),
        });
    }
    if let Some(f) = NUMBERED.captures(t) {
        return Some(Heading {
            number: f[1].parse().ok(),
            priority: f[3].parse().unwrap_or(3),
            title: clean_title(f.get(4).map_or("", |m| m.as_str())),
            ruling: f.get(2).map(|_| PriorStatus::StillOpen),
        });
    }
    if let Some(p) = BRACKETED.captures(t) {
        return Some(Heading {
            number: None,
            priority: p[2].parse().unwrap_or(3),
            title: clean_title(p.get(3).map_or("", |m| m.as_str())),
            ruling: None,
        });
    }
    // "### Problem 1: Title", the shape Orbit asks CLI reviewers for.
    PROBLEM.captures(t).map(|h| Heading {
        number: None,
        priority: 2,
        title: clean_title(&h[2]),
        ruling: None,
    })
}

fn clean_title(s: &str) -> String {
    let s = LINK.replace_all(s, "$1");
    let s = TRAILING_PLACE.replace_all(&s, "");
    let s = EDGES.replace_all(&s, "");
    SPACES.replace_all(&s, " ").trim().to_string()
}

/// Sentences, split at blank lines and after `.`, `!` or `?` and a space.
fn sentences(text: &str) -> Vec<String> {
    let mut out = vec![];
    for paragraph in PARAGRAPH.split(text) {
        let mut current = String::new();
        let mut chars = paragraph.chars().peekable();
        while let Some(ch) = chars.next() {
            current.push(ch);
            if matches!(ch, '.' | '!' | '?') && chars.peek().is_some_and(|c| c.is_whitespace()) {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                out.push(std::mem::take(&mut current));
            }
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
    }
    out
}

struct Open {
    f: Finding,
    lines: Vec<String>,
}

/// The finding being read is complete: its problem, scenario, fix,
/// whether it was reproduced, and its places.
fn finish(open: Open, findings: &mut Vec<Finding>) {
    let Open { mut f, lines } = open;
    let text = lines.join("\n");
    let paragraph = PARAGRAPH.split(&text).next().unwrap_or("").trim();
    f.problem = cut(&SPACES.replace_all(paragraph, " "), 1200);
    if let Some(b) = BREAKS.captures(&text) {
        f.failure_scenario = Some(SPACES.replace_all(&b[1], " ").trim().to_string());
    }
    if let Some(fix) = FIX.captures(&text) {
        f.suggested_fix = Some(SPACES.replace_all(&fix[1], " ").trim().to_string());
    }
    if REPRODUCED.is_match(&text) {
        f.confirmed = Some(true);
    } else if STATIC_ONLY.is_match(&text) {
        f.confirmed = Some(false);
    }
    let mut found = places(lines.first().map_or("", String::as_str));
    found.extend(places(&cut(&text, 600)));
    let mut seen = std::collections::HashSet::new();
    f.locations = found
        .into_iter()
        .filter(|l| seen.insert((l.path.clone(), l.start_line)))
        .collect();
    findings.push(f);
}

pub(super) fn parse_review(c: &CommentRef) -> Option<Round> {
    let body = &c.body;
    if !is_review(body) {
        return None;
    }
    let mut findings: Vec<Finding> = vec![];
    let mut earlier: Vec<Earlier> = vec![];
    let mut section = String::new();
    let mut open: Option<Open> = None;
    let mut ordinal = 0;
    let lead_non_blocking = NON_BLOCKING.is_match(&cut(body, 400));
    for raw in body.split('\n') {
        let line = raw.trim_end();
        if let Some(sec) = SECTION.captures(line) {
            if !PROBLEM_SECTION.is_match(line) {
                if let Some(o) = open.take() {
                    finish(o, &mut findings);
                }
                section = sec[1].to_lowercase();
                continue;
            }
        }
        if let Some(h) = heading(line) {
            if let Some(o) = open.take() {
                finish(o, &mut findings);
            }
            // "**F1 fixed:**" and "**F2 remaining · P2 — Title**" rule on
            // an earlier finding rather than raise a new one.
            if let Some(ruling) = h.ruling {
                let id = format!("F{}", h.number.unwrap_or(0));
                earlier.push(Earlier {
                    id: id.clone(),
                    short_id: Some(id),
                    title: h.title,
                    priority: None,
                    status: Some(ruling),
                    note: some(&clean_title(&line.replace("**", ""))),
                    author_said: None,
                });
                continue;
            }
            ordinal += 1;
            let optional =
                OPTIONAL_SECTION.is_match(&section) || (lead_non_blocking && h.priority == 3);
            let short_id = h.number.map(|n| format!("F{n}"));
            let mut f = Finding::new(
                short_id.clone().unwrap_or_else(|| match c.edit {
                    Some(edit) => format!("r{}v{edit}.{ordinal}", c.id),
                    None => format!("r{}.{ordinal}", c.id),
                }),
                if h.title.is_empty() {
                    "(untitled)".into()
                } else {
                    h.title.clone()
                },
                h.priority,
            );
            f.short_id = short_id;
            f.number = h.number;
            f.blocking = Some(!optional && h.priority <= 2);
            f.inline = Some(false);
            open = Some(Open {
                f,
                lines: vec![line.to_string()],
            });
            continue;
        }
        if let Some(o) = open.as_mut() {
            if line.starts_with("```")
                || line.starts_with("**Claude Code")
                || line.starts_with("Claude Code:")
            {
                if let Some(o) = open.take() {
                    finish(o, &mut findings);
                }
                continue;
            }
            o.lines.push(line.to_string());
        }
    }
    if let Some(o) = open.take() {
        finish(o, &mut findings);
    }
    let mut verdict = verdict(body);
    if verdict == RoundVerdict::Unknown && !findings.is_empty() && !BLOCKED.is_match(body) {
        verdict = if findings.iter().any(|f| f.blocking == Some(true)) {
            RoundVerdict::ChangesRequested
        } else {
            RoundVerdict::Clean
        };
    }
    let mut seen = std::collections::HashSet::new();
    earlier.retain(|e| seen.insert(e.id.to_ascii_uppercase()));
    let without_code = CODE.replace_all(body, "");
    let not_checked: Vec<String> = sentences(&without_code)
        .iter()
        .filter(|s| NOT_CHECKED.is_match(s))
        .take(6)
        .map(|s| cut(SPACES.replace_all(s.trim(), " ").as_ref(), 220))
        .collect();
    let summary = body
        .lines()
        .find(|l| SUMMARY_LINE.is_match(l.trim()))
        .map(|l| cut(l.replace("**", "").trim(), 400));
    let blocking = match verdict {
        RoundVerdict::ChangesRequested => Some(
            findings
                .iter()
                .filter(|f| f.blocking != Some(false))
                .count() as u32,
        ),
        RoundVerdict::Clean => Some(0),
        RoundVerdict::Unknown => None,
    };
    Some(Round {
        review_id: MARKER.find(body).map(|m| cut(m.as_str(), 120)),
        format: Format::Milo,
        round: None,
        head_sha: first(&HEADS, body),
        base_sha: first(&BASES, body),
        verdict,
        blocking,
        blocking_priority: None,
        reviewer: Some("Milo".into()),
        summary,
        findings,
        earlier_findings: earlier,
        not_checked,
        completed: INCOMPLETE.is_match(body).then_some(false),
        comment_id: c.id,
        url: c.url.clone(),
        created_at: c.created_at.clone(),
        body: body.clone(),
    })
}
