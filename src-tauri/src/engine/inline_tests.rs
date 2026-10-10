use super::{findings::parse, inline::*};
use crate::types::*;
use serde_json::json;

fn finding(path: Option<&str>, start_line: u32, end_line: u32) -> Finding {
    Finding {
        id: format!("rid-{start_line}"),
        title: "Keep overrides".into(),
        priority: 1,
        path: path.map(str::to_owned),
        start_line,
        end_line,
        body: "Explain.".into(),
        confidence: 0.8,
        ..Default::default()
    }
}
/// `src/a.rs` changes new lines 10-14 and 30-31; `new.rs` is added;
/// `gone.rs` is deleted; `big.bin` has no patch.
fn files() -> Hunks {
    hunks(&json!([
        {"filename": "src/a.rs", "patch": "@@ -10,4 +10,5 @@ fn a() {\n ctx\n+add\n ctx\n ctx\n ctx\n@@ -28,2 +30,2 @@\n x\n-y\n+z"},
        {"filename": "new.rs", "patch": "@@ -0,0 +1,3 @@\n+a\n+b\n+c"},
        {"filename": "gone.rs", "patch": "@@ -1,2 +0,0 @@\n-a\n-b"},
        {"filename": "one.rs", "patch": "@@ -4 +4 @@\n-a\n+b"},
        {"filename": "big.bin"}
    ]))
}
fn review(findings: Vec<Finding>) -> Review {
    let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), false);
    r.id = "rid".into();
    r.findings = Some(findings);
    r.verdict = Some(Verdict::ChangesRequested);
    r.orbit_verdict = Some(Verdict::ChangesRequested);
    r.blocking = Some(1);
    r.summary = Some("One defect.".into());
    r
}

#[test]
fn reads_the_right_side_of_each_hunk() {
    let h = files();
    assert_eq!(h["src/a.rs"], Some(vec![(10, 14), (30, 31)]));
    assert_eq!(h["new.rs"], Some(vec![(1, 3)]));
    assert_eq!(h["gone.rs"], Some(vec![]));
    assert_eq!(h["one.rs"], Some(vec![(4, 4)]));
    assert_eq!(h["big.bin"], None);
}
#[test]
fn places_findings_inside_one_hunk_inline() {
    let h = files();
    assert_eq!(placement(&finding(Some("src/a.rs"), 10, 14), &h), None);
    assert_eq!(placement(&finding(Some("src/a.rs"), 31, 31), &h), None);
    assert_eq!(placement(&finding(Some("new.rs"), 2, 3), &h), None);
    assert_eq!(placement(&finding(Some("one.rs"), 4, 4), &h), None);
}
#[test]
fn explains_why_other_findings_go_in_the_body() {
    let h = files();
    let why = |path: Option<&str>, a, b| placement(&finding(path, a, b), &h);
    assert_eq!(why(None, 1, 1), Some("outside the repository"));
    assert_eq!(
        why(Some("src/b.rs"), 1, 1),
        Some("file not changed in this pull request")
    );
    assert_eq!(why(Some("big.bin"), 1, 1), Some("diff too large to show"));
    assert_eq!(why(Some("src/a.rs"), 20, 20), Some("lines not in the diff"));
    assert_eq!(why(Some("src/a.rs"), 8, 11), Some("lines not in the diff"));
    assert_eq!(why(Some("gone.rs"), 1, 1), Some("lines not in the diff"));
    assert_eq!(
        why(Some("src/a.rs"), 12, 30),
        Some("spans more than one part of the diff")
    );
}
#[test]
fn makes_absolute_paths_relative_before_placing_them() {
    let d = tempfile::tempdir().unwrap();
    let worktree = d.path().to_str().unwrap();
    let answer = json!({
        "verdict": "changes_requested", "summary": "", "completed": true, "notChecked": [], "priorFindings": [],
        "findings": [{"title":"Keep overrides","priority":1,"category":"bug",
                      "locations":[{"path":format!("{worktree}/src/a.rs"),"startLine":11,"endLine":12}],
                      "problem":"Explain.","failureScenario":null,"evidence":null,"confirmed":true,
                      "suggestedFix":null,"doneWhen":null,"reviewerNotes":null,"confidence":0.8}]
    });
    let x = parse(&answer.to_string(), "rid", Some(worktree), None).unwrap();
    assert_eq!(x.findings[0].path.as_deref(), Some("src/a.rs"));
    assert_eq!(placement(&x.findings[0], &files()), None);
}
#[test]
fn comments_carry_the_whole_finding() {
    let f = Finding {
        category: Some(Category::Bug),
        confirmed: Some(true),
        failure_scenario: Some("A one-item list returns nothing.".into()),
        suggested_fix: Some("Index 0.".into()),
        other_locations: vec![Location {
            path: Some("src/main.rs".into()),
            start_line: 40,
            end_line: 40,
        }],
        ..finding(Some("src/a.rs"), 11, 12)
    };
    assert_eq!(
        comment_body(&f),
        "**[P1] Keep overrides**\n\nExplain.\n\n_Bug · Confirmed_\n\
         **Breaks when:** A one-item list returns nothing.\n\
         **Suggested fix:** Index 0.\n**Also at:** `src/main.rs:40`\n\n\
         <!-- orbit-finding:rid-11 -->"
    );
    // A finding that can't go inline carries it all in the review's text.
    let plan = plan(&review(vec![f]), &files(), None, false);
    assert!(
        plan.body
            .contains("Explain.\n\n_Bug · Confirmed_\n**Breaks when:**"),
        "{}",
        plan.body
    );
}
#[test]
fn builds_the_review_with_inline_comments_and_other_findings() {
    let r = review(vec![
        finding(Some("src/a.rs"), 11, 12),
        finding(Some("new.rs"), 2, 2),
        finding(Some("src/a.rs"), 20, 21),
    ]);
    let plan = plan(&r, &files(), Some(2), true);
    assert_eq!(plan.comments.len(), 2);
    assert_eq!(
        plan.comments[0],
        json!({"path":"src/a.rs","line":12,"start_line":11,"start_side":"RIGHT","side":"RIGHT",
               "body":"**[P1] Keep overrides**\n\nExplain.\n\n<!-- orbit-finding:rid-11 -->"})
    );
    assert_eq!(plan.comments[1]["line"], 2);
    assert!(plan.comments[1].get("start_line").is_none());
    assert!(plan.body.starts_with(
        "<!-- orbit-review:rid -->\n<!-- orbit-verdict: changes_requested round=2 blocking=1 -->\n## Orbit · Codex review"
    ));
    assert!(plan
        .body
        .contains("Verdict: changes requested (1 blocking finding)\n\nOne defect."));
    assert!(plan.body.contains(
        "### Other findings\n\n**[P1] Keep overrides** · `src/a.rs:20-21` · not inline: lines not in the diff\n\nExplain."
    ));
    assert!(!plan.body.contains("rid-11"));
    // The fallback when GitHub refuses a line puts every finding in the body.
    let all = super::inline::plan(&r, &files(), Some(2), false);
    assert!(all.comments.is_empty());
    assert_eq!(all.body.matches("not inline").count(), 3);
    let mut claude = r.clone();
    claude.provider = Provider::Claude;
    let body = super::inline::plan(&claude, &files(), Some(2), true).body;
    assert!(body.contains("\n## Orbit · Claude review\n") && !body.contains("Codex"));
}
#[test]
fn previews_where_each_finding_goes() {
    let r = review(vec![finding(Some("src/a.rs"), 11, 12), finding(None, 3, 3)]);
    let preview = preview(&r, &files(), None);
    assert_eq!(
        preview.placements,
        [
            Placement {
                finding_id: "rid-11".into(),
                inline: true,
                reason: None
            },
            Placement {
                finding_id: "rid-3".into(),
                inline: false,
                reason: Some("outside the repository".into())
            }
        ]
    );
    assert_eq!(preview.body, plan(&r, &files(), None, true).body);
}
#[test]
fn a_clean_review_has_no_other_findings_section() {
    let mut r = review(vec![]);
    r.orbit_verdict = Some(Verdict::Clean);
    let plan = plan(&r, &files(), None, true);
    assert!(plan.comments.is_empty());
    assert!(plan.body.contains("Verdict: clean"));
    assert!(!plan.body.contains("Other findings"));
}
#[test]
fn enforces_githubs_size_limits() {
    let mut big = finding(Some("src/a.rs"), 11, 11);
    big.body = "x".repeat(MAX_BODY);
    let r = review(vec![big]);
    let inline = plan(&r, &files(), None, true);
    assert!(inline.body.len() < MAX_BODY);
    assert!(check_size(&inline)
        .unwrap_err()
        .contains("A finding exceeds"));
    let body = plan(&r, &files(), None, false);
    assert!(check_size(&body)
        .unwrap_err()
        .contains("review body exceeds"));
    assert!(check_size(&plan(&review(vec![]), &files(), None, true)).is_ok());
}
#[test]
fn maps_inline_comment_ids_to_findings() {
    let items = json!([
        {"id": 11, "body": "**[P1] A**\n\nx\n\n<!-- orbit-finding:rid-0 -->"},
        {"id": 12, "body": "A reply without a marker"},
        {"id": 13, "body": "**[P2] B**\n\ny\n\n<!-- orbit-finding:rid-2 -->"}
    ]);
    assert_eq!(
        inline_comments(items.as_array().unwrap()),
        [
            InlineComment {
                finding_id: "rid-0".into(),
                comment_id: 11
            },
            InlineComment {
                finding_id: "rid-2".into(),
                comment_id: 13
            }
        ]
    );
}
#[test]
fn scans_every_published_part_for_credentials() {
    let mut r = review(vec![]);
    r.previous_round = Some(PreviousRound {
        review_id: "old".into(),
        head_sha: "c".repeat(40),
        verdict: None,
        findings: vec![Finding {
            title: format!("Leaks ghp_{}", "a1".repeat(18)),
            ..finding(Some("src/a.rs"), 1, 1)
        }],
        decisions: vec![],
        comments_included: 0,
        comments_omitted: 0,
    });
    assert!(r
        .credential_scan()
        .iter()
        .any(|k| k.contains("GitHub token")));
}
