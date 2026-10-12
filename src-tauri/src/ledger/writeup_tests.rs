use super::assemble_tests::fixture;
use super::writeup::*;
use super::*;
use serde_json::json;

const REPLY: &str = "Addressed Milo's review of `9aaf81e`";

#[test]
fn the_writer_reads_the_description_rounds_replies_and_where_findings_ended() {
    let h = derive(&fixture(64));
    let (text, hash) = input(&h, &["Settings".into(), "Ask Bit".into()]);
    assert!(text.starts_with(
        "Repository: analogproblems/orbit\nPull request #64: Ask Bit: quick answers, fix buttons and rules (#63, part 1)\nOpened 2026-10-10T23:43:36Z, merged 2026-10-11T00:14:57Z.\nPart of issue #63 (stage 1).\nFiles changed: 28 (+2790 / -221). README.md, "
    ));
    for part in [
        "## Builder's description of the change\nPart of #63",
        "Before: Bit's page was a list of notes.",
        "- 3a235dc Recheck Bit's fixes before acting, and trust on the fresh PR",
        "- F2: fixed; builder said fixed in 3e0a5fa",
        "- F2 (Revalidate queued fixes before performing their side effects) stayed open across 3 rounds.",
        "## Known areas so far\nSettings; Ask Bit",
        "### Reviewer round 1 at 2026-10-10T23:48:07Z: changes requested (3 blocking)",
        "- F3 · P1 · blocking · likely, not verified: Preserve the live trust check for Bit's publications.",
        "Rulings on earlier findings: F1=fixed",
        "### Builder (jasondwhyte) at 2026-10-11T00:00:13Z\nAddressed Milo's review",
    ] {
        assert!(text.contains(part), "missing: {part}");
    }
    // The reviewer's evidence and hidden data stay out.
    assert!(!text.contains("<!--"));
    assert!(!text.contains("### Scope and verification"));
    assert!(!text.contains("### Executed tests and limits"));
    // The rounds come in order, after the summary sections.
    let at = |s: &str| text.find(s).unwrap();
    assert!(at("## Known areas so far") < at("### Reviewer round 1"));
    assert!(at("### Reviewer round 2") < at("### Builder (jasondwhyte) at 2026-10-11T00:00:13Z"));

    // What it was written from: not the known areas or when it was read,
    // but any change to the conversation.
    assert_eq!(input(&h, &[]).1, hash);
    let mut stored = fixture(64);
    stored.fetched_at = "2026-10-13T00:00:00Z".into();
    assert_eq!(input(&derive(&stored), &[]).1, hash);
    let reply = stored
        .comments
        .iter_mut()
        .find(|c| c.body.contains(REPLY))
        .unwrap();
    reply.body.push_str("\n\nOne more thing.");
    assert_ne!(input(&derive(&stored), &[]).1, hash);
}

#[test]
fn a_reviewers_text_loses_commands_folded_details_and_milos_evidence() {
    let findings = "Findings. ".repeat(50);
    let milo = format!(
        "## Milo PR review\n\n<!-- milo-pr-review:x -->\n**CHANGES REQUESTED**\n\n{findings}\n\n```bash\nnpm test\n```\n\n<details><summary>Log</summary>\nlots\n</details>\n\n\n\n### Executed tests and limits\n\nAll passed.\n\n**Claude Code:** consume this review only if the head matches."
    );
    let trimmed = trim_review(&milo, Format::Milo);
    assert!(trimmed.starts_with("## Milo PR review\n\n**CHANGES REQUESTED**"));
    assert!(trimmed.contains("[commands omitted]"));
    assert!(!trimmed.contains("npm test"));
    assert!(!trimmed.contains("lots"));
    assert!(!trimmed.contains("Executed tests"));
    assert!(!trimmed.contains("Claude Code"));
    assert!(!trimmed.contains("\n\n\n"));
    // Orbit's own reviews keep every section.
    let orbit = "## Orbit · round 1\n\nOne finding.\n\n### Verification\n\nRan the tests.";
    assert_eq!(trim_review(orbit, Format::Orbit), orbit);
}

#[test]
fn an_answer_keeps_real_quotes_with_their_link_and_drops_what_it_cant_use() {
    let h = derive(&fixture(64));
    let reply = h.comments.iter().find(|c| c.body.contains(REPLY)).unwrap();
    let decision = |kind: &str, statement: &str, area: &str, quote: Value, attention: &str| {
        json!({
            "kind": kind, "statement": statement, "detail": null, "madeBy": "builder",
            "area": area, "quote": quote, "attention": attention,
            "attentionReason": if attention == "none" { Value::Null } else { json!("A person should confirm it.") },
            "supersedes": null,
        })
    };
    let answer = json!({
        "summary": "  Bit answers four quick questions and fixes things with a tap. The review took four rounds.  ",
        "decisions": [
            decision(
                "rule",
                "Bit never publishes a report the credential check flagged.",
                "Bit's Fixes",
                // Copied without the bold, as writers do.
                json!("Publish report is never offered for a report the credential check flagged"),
                "none",
            ),
            decision("rule", "bit never publishes a report the credential check flagged.", "Bit's Fixes", Value::Null, "none"),
            decision(
                "challenge",
                "A queued fix could still run after you turned Bit off.",
                "bit's fixes",
                json!("If the fix stops being Bit's while its item is still open, Bit hands it back"),
                "look",
            ),
            decision(
                "interpretation",
                "Turning autopilot back on always waits for a tap.",
                "Autopilot",
                json!("Bit promised it would never do this without asking"),
                "act",
            ),
            decision("vibe", "Not a kind of decision.", "Autopilot", Value::Null, "none"),
            decision("boundary", "   ", "Autopilot", Value::Null, "none"),
        ],
        "findingNotes": [
            {"findingId": "f2", "plain": "A fix you cancelled could still have happened."},
            {"findingId": "F9", "plain": "There is no F9."},
        ],
    });
    let a = read(&answer, &h).unwrap();
    assert_eq!(
        a.summary,
        "Bit answers four quick questions and fixes things with a tap. The review took four rounds."
    );
    let seen: Vec<(DecisionKind, Option<&str>, &str, Option<Glance>)> = a
        .decisions
        .iter()
        .map(|d| (d.kind, d.quote.as_deref(), d.url.as_str(), d.attention))
        .collect();
    assert_eq!(
        seen,
        [
            (
                DecisionKind::Rule,
                Some("Publish report is never offered for a report the credential check flagged"),
                h.url.as_str(),
                None
            ),
            (
                DecisionKind::Challenge,
                Some(
                    "If the fix stops being Bit's while its item is still open, Bit hands it back"
                ),
                reply.url.as_str(),
                Some(Glance::Look)
            ),
            // A quote that isn't in the pull request is dropped.
            (
                DecisionKind::Interpretation,
                None,
                h.url.as_str(),
                Some(Glance::Act)
            ),
        ]
    );
    assert_eq!(
        a.decisions[2].attention_reason.as_deref(),
        Some("A person should confirm it.")
    );
    assert_eq!(a.areas, ["Bit's Fixes", "Autopilot"]);
    let f2 = h
        .trajectories
        .iter()
        .find(|t| t.short_id.as_deref() == Some("F2"))
        .unwrap();
    assert_eq!(
        a.finding_notes,
        [FindingNote {
            finding_id: f2.finding_id.clone(),
            plain: "A fix you cancelled could still have happened.".into(),
        }]
    );

    assert!(read(&json!({"decisions": []}), &h).is_err());
    assert!(read(&json!({"summary": "Hi."}), &h).is_err());
}

fn written(repo: &str, number: u64, areas: &[&str]) -> WriteUp {
    WriteUp {
        version: WRITEUP_VERSION,
        repo: repo.into(),
        number,
        input_hash: "h".into(),
        writer: "Codex".into(),
        model: "model-a".into(),
        written_at: "2026-10-12T00:00:00Z".into(),
        summary: "A change.".into(),
        areas: vec![],
        decisions: areas
            .iter()
            .map(|area| Decision {
                kind: DecisionKind::Rule,
                statement: format!("A rule about {area}."),
                detail: None,
                made_by: MadeBy::Builder,
                area: area.to_string(),
                quote: None,
                url: String::new(),
                attention: None,
                attention_reason: None,
                supersedes: None,
            })
            .collect(),
        finding_notes: vec![],
    }
}

#[test]
fn known_areas_are_the_most_used_first() {
    let a = written("a/b", 1, &["Settings", "Publishing to GitHub"]);
    let b = written("a/b", 2, &["publishing to GitHub", "Ask Bit"]);
    assert_eq!(
        known_areas([&a, &b]),
        ["Publishing to GitHub", "Ask Bit", "Settings"]
    );
}

#[test]
fn write_ups_are_kept_beside_the_history_and_never_read_as_pull_requests() {
    let d = tempfile::tempdir().unwrap();
    save(d.path(), &fixture(64)).unwrap();
    save_writeup(d.path(), &written("analogproblems/orbit", 64, &["Ask Bit"])).unwrap();
    let mut old = written("analogproblems/orbit", 21, &[]);
    old.version = WRITEUP_VERSION + 1;
    save_writeup(d.path(), &old).unwrap();
    assert!(d
        .path()
        .join("ledger/analogproblems/orbit/writeups/64.json")
        .exists());
    let found = load_writeups(d.path(), "AnalogProblems/Orbit").unwrap();
    assert_eq!(found.keys().collect::<Vec<_>>(), [&64]);
    assert_eq!(found[&64].decisions[0].area, "Ask Bit");
    let (prs, problems) = load_repo(d.path(), "analogproblems/orbit").unwrap();
    assert_eq!((prs.len(), problems.len()), (1, 0));
    assert!(load_writeups(d.path(), "a/none").unwrap().is_empty());
    assert!(save_writeup(d.path(), &written("../x", 1, &[])).is_err());
}
