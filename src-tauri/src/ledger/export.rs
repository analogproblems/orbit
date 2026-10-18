//! The Decisions page as one Markdown file, to keep or share outside Orbit:
//! what is worth a look, the rulebook, and each pull request's chapter with
//! its write-up when it has one. Made from the stored history only.
use super::{
    writeup::{Decision, DecisionKind, MadeBy},
    *,
};

/// The order a write-up's decisions are listed in, as on the page.
const KINDS: [DecisionKind; 8] = [
    DecisionKind::Intent,
    DecisionKind::Rule,
    DecisionKind::Tradeoff,
    DecisionKind::Interpretation,
    DecisionKind::Boundary,
    DecisionKind::Challenge,
    DecisionKind::Pushback,
    DecisionKind::Verification,
];

/// `orbit-decisions-<owner>-<name>-<day>.md`, or `-all-` for every
/// repository.
pub fn file_name(repo: Option<&str>, day: &str) -> String {
    let scope = repo.map_or("all".into(), |r| {
        r.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
    });
    format!("orbit-decisions-{scope}-{day}.md")
}

/// Every pull request in `prs`, most recently updated first, as Markdown.
/// `repo` names the one repository they come from, if only one.
pub fn markdown(prs: &[History], repo: Option<&str>, at: &str) -> String {
    let mut out = format!(
        "# Decisions: {}\n\nExported from Orbit on {}. {} read from GitHub; the plain-words write-ups are the writer's reading, and their quotes were found in the pull requests.\n",
        repo.unwrap_or("every connected repository"),
        day(at),
        plural(prs.len(), "pull request"),
    );
    let worth = worth_a_look(prs);
    if !worth.is_empty() {
        out.push_str("\n## Worth a look\n\n");
        for (h, glance, label, text) in worth {
            let mark = match glance {
                Glance::Act => "Needs a decision",
                Glance::Look => "Good to know",
            };
            out.push_str(&format!(
                "- **{label}** ({mark}), {}: {}\n",
                link(&reference(h), &h.url),
                line(&text)
            ));
        }
    }
    let rules = rulebook(prs);
    if !rules.is_empty() {
        out.push_str("\n## Rulebook\n\nWhat the app now commits to: the rules of merged pull requests, by the part of the app each governs, oldest first.\n");
        for (area, rules) in rules {
            out.push_str(&format!("\n### {}\n\n", line(area)));
            for (h, d) in rules {
                out.push_str(&format!("- {}", line(&d.statement)));
                if let Some(s) = d.supersedes.as_deref() {
                    out.push_str(&format!(" Changes an earlier rule: {}", line(s)));
                }
                out.push_str(&format!(" ({})\n", link(&reference(h), &d.url)));
            }
        }
    }
    out.push_str("\n## Pull requests\n");
    for h in prs {
        chapter(&mut out, h);
    }
    out
}

/// One pull request's chapter.
fn chapter(out: &mut String, h: &History) {
    out.push_str(&format!(
        "\n### {}\n\n",
        link(&format!("{}: {}", reference(h), line(&h.title)), &h.url)
    ));
    let mut facts = vec![
        state(h).to_string(),
        format!("opened {}", day(&h.created_at)),
    ];
    if let Some(m) = h.merged_at.as_deref().filter(|_| h.merged) {
        facts.push(format!("merged {}", day(m)));
    } else if let Some(c) = h.closed_at.as_deref().filter(|_| h.state != "open") {
        facts.push(format!("closed {}", day(c)));
    }
    if h.rounds.is_empty() {
        facts.push("no review".into());
    } else {
        facts.push(plural(h.rounds.len(), "round"));
        facts.push(plural(h.trajectories.len(), "finding"));
        facts.push(format!("blocking findings by round: {}", round_strip(h)));
    }
    out.push_str(&format!("{}.\n", facts.join(" · ")));
    let n = &h.narrative;
    // Its description's After, else its opening, quoted as written.
    if let Some(gist) = n
        .after
        .as_deref()
        .or(n.lead.as_deref())
        .or(n.before.as_deref())
    {
        out.push_str("\nWhat it set out to do:\n\n");
        for l in gist.trim().lines() {
            out.push_str(&format!("> {}\n", inert(l.trim_end())).replace("> \n", ">\n"));
        }
    }
    if let Some(w) = &h.writeup {
        out.push_str(&format!("\n**In plain words.** {}\n", line(&w.summary)));
        for kind in KINDS {
            let ds: Vec<&Decision> = w.decisions.iter().filter(|d| d.kind == kind).collect();
            if ds.is_empty() {
                continue;
            }
            out.push_str(&format!("\n{}:\n\n", kind_label(kind)));
            for d in ds {
                out.push_str(&format!("- {}", line(&d.statement)));
                if let Some(detail) = d.detail.as_deref() {
                    out.push_str(&format!(" {}", line(detail)));
                }
                if let Some(q) = d.quote.as_deref() {
                    out.push_str(&format!(" “{}”", line(q)));
                }
                out.push_str(&format!(
                    " ({}, {}, {})",
                    line(&d.area),
                    made_by_label(d.made_by),
                    link("source", &d.url)
                ));
                if let (Some(_), Some(why)) = (d.attention, d.attention_reason.as_deref()) {
                    out.push_str(&format!(" Worth a look: {}", line(why)));
                }
                out.push('\n');
            }
        }
        out.push_str(&format!(
            "\nWritten by {} ({}) on {}.\n",
            w.writer,
            w.model,
            day(&w.written_at)
        ));
    }
    if !h.trajectories.is_empty() {
        out.push_str("\nFindings:\n\n");
        for t in &h.trajectories {
            let id = t
                .short_id
                .as_deref()
                .map(|s| format!("{s} · "))
                .unwrap_or_default();
            out.push_str(&format!(
                "- {id}P{} · {}: {}",
                t.priority,
                line(&t.title),
                status_label(t.final_status)
            ));
            let plain = h
                .writeup
                .iter()
                .flat_map(|w| &w.finding_notes)
                .find(|f| f.finding_id == t.finding_id);
            if let Some(p) = plain {
                out.push_str(&format!(". {}", line(&p.plain)));
            }
            out.push('\n');
        }
    }
    if !h.flags.is_empty() {
        out.push_str("\nWorth a look:\n\n");
        for f in &h.flags {
            out.push_str(&format!("- {}: {}\n", flag_label(f.kind), line(&f.text)));
        }
    }
}

type WorthRow<'a> = (&'a History, Glance, &'static str, String);
/// Flags and write-up decisions marked for a glance, those that need a
/// decision first, as on the page.
fn worth_a_look(prs: &[History]) -> Vec<WorthRow<'_>> {
    let mut rows: Vec<WorthRow> = vec![];
    for h in prs {
        for f in &h.flags {
            rows.push((h, f.attention, flag_label(f.kind), f.text.clone()));
        }
        for d in h.writeup.iter().flat_map(|w| &w.decisions) {
            if let Some(glance) = d.attention {
                let text = match d.attention_reason.as_deref() {
                    Some(why) => format!("{} {}", d.statement, why),
                    None => d.statement.clone(),
                };
                rows.push((h, glance, kind_label(d.kind), text));
            }
        }
    }
    // A stable sort keeps the pull requests' order within each.
    rows.sort_by_key(|r| r.1 != Glance::Act);
    rows
}

/// Every rule from a merged pull request by area, the area with most rules
/// first, oldest rule first. One closed without merging, or reopened, only
/// proposed its rules, so they stay in its chapter.
fn rulebook(prs: &[History]) -> Vec<(&str, Vec<(&History, &Decision)>)> {
    let mut areas: Vec<(&str, Vec<(&History, &Decision)>)> = vec![];
    for h in prs.iter().filter(|h| h.merged) {
        for d in h.writeup.iter().flat_map(|w| &w.decisions) {
            if d.kind != DecisionKind::Rule {
                continue;
            }
            match areas
                .iter_mut()
                .find(|(a, _)| a.eq_ignore_ascii_case(&d.area))
            {
                Some((_, rules)) => rules.push((h, d)),
                None => areas.push((&d.area, vec![(h, d)])),
            }
        }
    }
    let when = |h: &History| h.merged_at.clone().unwrap_or_else(|| h.updated_at.clone());
    for (_, rules) in &mut areas {
        rules.sort_by_key(|(h, _)| when(h));
    }
    areas.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
    areas
}

/// `orbit#64`.
fn reference(h: &History) -> String {
    format!(
        "{}#{}",
        h.repo.rsplit('/').next().unwrap_or(&h.repo),
        h.number
    )
}
fn link(text: &str, url: &str) -> String {
    if url.is_empty() {
        text.to_string()
    } else {
        format!("[{text}]({url})")
    }
}
/// Text on one line, for a list item or heading, kept as written.
fn line(text: &str) -> String {
    inert(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}
/// Text from a pull request or a writer shown as written: its HTML, images
/// and links come out as plain text. A code span, a run of backticks up to
/// the next run of the same length as CommonMark pairs them, is left alone,
/// since nothing in it renders; a run with no partner is escaped.
pub(super) fn inert(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let run = |at: usize| chars[at..].iter().take_while(|&&c| c == '`').count();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '`' => {
                let n = run(i);
                let mut j = i + n;
                let close = loop {
                    match chars.get(j) {
                        None => break None,
                        Some('`') => match run(j) {
                            m if m == n => break Some(j + n),
                            m => j += m,
                        },
                        Some(_) => j += 1,
                    }
                };
                match close {
                    Some(end) => {
                        out.extend(&chars[i..end]);
                        i = end;
                    }
                    None => {
                        out.push_str(&"\\`".repeat(n));
                        i += n;
                    }
                }
            }
            c @ ('\\' | '[' | ']') => {
                out.push('\\');
                out.push(c);
                i += 1;
            }
            '<' => {
                out.push_str("&lt;");
                i += 1;
            }
            '&' => {
                out.push_str("&amp;");
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}
/// `2026-10-12` from a time.
fn day(at: &str) -> String {
    cut(at, 10)
}
fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}
fn state(h: &History) -> &'static str {
    if h.merged {
        "Merged"
    } else if h.state == "open" {
        if h.draft {
            "Draft"
        } else {
            "Open"
        }
    } else {
        "Closed"
    }
}
/// Blocking findings round by round, as the page shows them.
fn round_strip(h: &History) -> String {
    h.rounds
        .iter()
        .map(|r| match (r.verdict, r.blocking) {
            (RoundVerdict::Unknown, _) => "?".into(),
            (_, Some(n)) => n.to_string(),
            (RoundVerdict::Clean, None) => "0".into(),
            (_, None) => r
                .findings
                .iter()
                .filter(|f| f.blocking != Some(false))
                .count()
                .to_string(),
        })
        .collect::<Vec<_>>()
        .join(" → ")
}
fn flag_label(kind: FlagKind) -> &'static str {
    match kind {
        FlagKind::MergedWithOpenFindings => "Merged with a finding open",
        FlagKind::DisputedDecline => "Disputed decline",
        FlagKind::RepeatBlocking => "Still open after two rounds",
        FlagKind::NoReviewBeforeMerge => "Merged before a review",
        FlagKind::ReviewerCouldNotCheck => "Not checked",
        FlagKind::LikelyNotVerified => "Not verified",
    }
}
fn kind_label(kind: DecisionKind) -> &'static str {
    match kind {
        DecisionKind::Intent => "What it set out to do",
        DecisionKind::Rule => "Rules it set",
        DecisionKind::Tradeoff => "Trade-offs",
        DecisionKind::Interpretation => "How a request was read",
        DecisionKind::Boundary => "What was left out",
        DecisionKind::Challenge => "What the reviewer challenged",
        DecisionKind::Pushback => "Pushback",
        DecisionKind::Verification => "What was checked",
    }
}
fn made_by_label(by: MadeBy) -> &'static str {
    match by {
        MadeBy::Builder => "the builder",
        MadeBy::Reviewer => "the reviewer",
        MadeBy::Both => "builder and reviewer",
        MadeBy::Human => "a person",
    }
}
fn status_label(status: FinalStatus) -> &'static str {
    match status {
        FinalStatus::Fixed => "fixed",
        FinalStatus::StillOpen => "still open",
        FinalStatus::DeclinedAccepted => "declined, and the reviewer accepted it",
        FinalStatus::DeclinedDisputed => "declined, and the reviewer disputes it",
        FinalStatus::Open => "no ruling",
    }
}
