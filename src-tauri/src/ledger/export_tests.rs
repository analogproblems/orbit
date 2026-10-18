use super::assemble_tests::fixture;
use super::export::*;
use super::writeup::*;
use super::*;

fn decision(kind: DecisionKind, statement: &str, area: &str) -> Decision {
    Decision {
        kind,
        statement: statement.into(),
        detail: None,
        made_by: MadeBy::Builder,
        area: area.into(),
        quote: None,
        url: "https://github.com/analogproblems/orbit/pull/64".into(),
        attention: None,
        attention_reason: None,
        supersedes: None,
    }
}
fn written(h: &History, decisions: Vec<Decision>, notes: Vec<FindingNote>) -> WriteUp {
    WriteUp {
        version: WRITEUP_VERSION,
        repo: h.repo.clone(),
        number: h.number,
        input_hash: "h".into(),
        writer: "Codex".into(),
        model: "model-a".into(),
        written_at: "2026-10-12T03:00:00Z".into(),
        summary: "Bit answers four quick questions.\nThe review took four rounds.".into(),
        areas: vec![],
        decisions,
        finding_notes: notes,
    }
}

#[test]
fn the_export_tells_each_story_with_its_write_up_rules_and_what_needs_a_look() {
    let mut bit = derive(&fixture(64));
    let mut status = derive(&fixture(21));
    let f2 = bit
        .trajectories
        .iter()
        .find(|t| t.short_id.as_deref() == Some("F2"))
        .unwrap()
        .finding_id
        .clone();
    bit.writeup = Some(written(
        &bit,
        vec![
            decision(
                DecisionKind::Intent,
                "Bit answers quick questions.",
                "Ask Bit",
            ),
            Decision {
                quote: Some("Publish report is never offered".into()),
                made_by: MadeBy::Both,
                ..decision(
                    DecisionKind::Rule,
                    "Bit never publishes a flagged report.",
                    "Bit's Fixes",
                )
            },
            Decision {
                attention: Some(Glance::Act),
                attention_reason: Some("Decide whether that is right.".into()),
                ..decision(
                    DecisionKind::Pushback,
                    "Turning autopilot back on waits for a tap.",
                    "Autopilot",
                )
            },
        ],
        vec![FindingNote {
            finding_id: f2,
            plain: "A fix you cancelled could still have happened.".into(),
        }],
    ));
    status.writeup = Some(written(
        &status,
        vec![Decision {
            supersedes: Some("Only the comment showed the verdict.".into()),
            url: "https://github.com/analogproblems/orbit/pull/21".into(),
            ..decision(
                DecisionKind::Rule,
                "Each round sets a commit status.",
                "bit's fixes",
            )
        }],
        vec![],
    ));
    // Closed without merging: its rule was only proposed.
    let mut proposed = derive(&fixture(75));
    (proposed.merged, proposed.state) = (false, "closed".into());
    proposed.writeup = Some(written(
        &proposed,
        vec![decision(
            DecisionKind::Rule,
            "Every export waits for approval.",
            "Exports",
        )],
        vec![],
    ));
    let text = markdown(&[bit, status, proposed], None, "2026-10-12T05:00:00Z");
    assert!(text.starts_with(
        "# Decisions: every connected repository\n\nExported from Orbit on 2026-10-12. 3 pull requests read from GitHub;"
    ));
    assert!(!text.contains("### Exports"));
    assert!(
        text.contains("Rules it set:\n\n- Every export waits for approval. (Exports, the builder,")
    );
    for part in [
        // What needs a decision comes first, with the writer's reason.
        "## Worth a look\n\n- **Pushback** (Needs a decision), [orbit#64](https://github.com/analogproblems/orbit/pull/64): Turning autopilot back on waits for a tap. Decide whether that is right.\n- **Still open after two rounds** (Good to know)",
        // Rules by area whatever its case, oldest first.
        "## Rulebook\n\nWhat the app now commits to: the rules of merged pull requests, by the part of the app each governs, oldest first.\n\n### Bit's Fixes\n\n- Each round sets a commit status. Changes an earlier rule: Only the comment showed the verdict. ([orbit#21](https://github.com/analogproblems/orbit/pull/21))\n- Bit never publishes a flagged report. ([orbit#64](https://github.com/analogproblems/orbit/pull/64))\n",
        "### [orbit#64: Ask Bit: quick answers, fix buttons and rules (#63, part 1)](https://github.com/analogproblems/orbit/pull/64)\n\nMerged · opened 2026-10-10 · merged 2026-10-11 · 4 rounds · 4 findings · blocking findings by round: 3 → 3 → 2 → 0.\n\nWhat it set out to do:\n\n> Bit's page is one conversation",
        "**In plain words.** Bit answers four quick questions. The review took four rounds.\n\nWhat it set out to do:\n\n- Bit answers quick questions. (Ask Bit, the builder, [source](https://github.com/analogproblems/orbit/pull/64))\n",
        "Rules it set:\n\n- Bit never publishes a flagged report. “Publish report is never offered” (Bit's Fixes, builder and reviewer, [source](",
        "Pushback:\n\n- Turning autopilot back on waits for a tap. (Autopilot, the builder, [source](https://github.com/analogproblems/orbit/pull/64)) Worth a look: Decide whether that is right.\n",
        "Written by Codex (model-a) on 2026-10-12.",
        "- F2 · P1 · Revalidate queued fixes before performing their side effects: fixed. A fix you cancelled could still have happened.\n",
        "Worth a look:\n\n- Still open after two rounds: F2 (Revalidate queued fixes before performing their side effects) stayed open across 3 rounds.\n",
        "- P2 · Run GitHub status synchronization even when Codex is disconnected: fixed\n",
    ] {
        assert!(text.contains(part), "missing:\n{part}\n\nin:\n{text}");
    }
    let at = |s: &str| text.find(s).unwrap();
    assert!(at("### [orbit#64") < at("### [orbit#21"));
    // A description's list stays a list.
    assert!(text.contains("reviewed commit:\n> - `pending` while the round is in progress;\n"));
}

#[test]
fn an_export_names_its_repository_and_a_safe_file() {
    let h = derive(&fixture(75));
    let text = markdown(
        std::slice::from_ref(&h),
        Some("analogproblems/orbit"),
        "2026-10-12",
    );
    assert!(text.starts_with("# Decisions: analogproblems/orbit\n\nExported from Orbit on 2026-10-12. 1 pull request read"));
    assert!(!text.contains("## Rulebook"));
    assert_eq!(
        file_name(Some("AnalogProblems/Orbit"), "2026-10-12"),
        "orbit-decisions-AnalogProblems-Orbit-2026-10-12.md"
    );
    assert_eq!(
        file_name(None, "2026-10-12"),
        "orbit-decisions-all-2026-10-12.md"
    );
    assert!(!file_name(Some("../../x"), "d").contains('/'));
}

#[test]
fn text_from_a_pull_request_is_exported_as_written() {
    let mut h = derive(&fixture(75));
    h.title = "Show [a](b) and <img src=x> in `Vec<T>`".into();
    h.narrative.after =
        Some("![x](http://example.com/x.png) & \\[c\\]\n<script>no</script>".into());
    let text = markdown(std::slice::from_ref(&h), None, "2026-10-12");
    assert!(text.contains(r"Show \[a\](b) and &lt;img src=x> in `Vec<T>`]("));
    assert!(text.contains(
        "> !\\[x\\](http://example.com/x.png) &amp; \\\\\\[c\\\\\\]\n> &lt;script>no&lt;/script>\n"
    ));
    assert!(!text.contains("<img") && !text.contains("<script"));
    assert_eq!(inert("one ` tick"), "one \\` tick");
}
