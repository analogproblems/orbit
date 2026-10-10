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
    r.blocking_priority = Some(1);
    r.summary = Some("One defect.".into());
    r
}
/// The hidden data in a review's text.
fn data(body: &str) -> serde_json::Value {
    let rest = body.split("<!-- orbit-data ").nth(1).expect("hidden data");
    serde_json::from_str(rest.split_once(" -->").unwrap().0).unwrap()
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
        number: Some(2),
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
        comment_body(&f, Some(1)),
        "**F2 · P1 · Blocking · Bug** Keep overrides\n\nExplain.\n\n\
         **Breaks when:** A one-item list returns nothing.\n\
         **Suggested fix:** Index 0.\n**Also at:** `src/main.rs:40`\n\n\
         <!-- orbit-finding:rid-11 -->"
    );
    let likely = Finding {
        priority: 2,
        confirmed: Some(false),
        ..f.clone()
    };
    assert!(comment_body(&likely, Some(1))
        .starts_with("**F2 · P2 · Not blocking · Bug · Likely, not verified** Keep overrides\n"));
    // A finding from before short IDs, or a review from before blocking
    // labels, says what it can.
    assert!(comment_body(&finding(Some("src/a.rs"), 11, 12), None)
        .starts_with("**P1** Keep overrides\n\nExplain.\n\n<!--"));
    // A finding that can't go inline carries it all in the review's text.
    let plan = plan(&review(vec![f]), &files(), None, false);
    assert!(
        plan.body.contains(
            "**F2 · P1 · Blocking · Bug** Keep overrides · `src/a.rs:11-12` · not inline: \
             GitHub did not accept a comment on these lines\n\nExplain.\n\n**Breaks when:**"
        ),
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
               "body":"**P1 · Blocking** Keep overrides\n\nExplain.\n\n<!-- orbit-finding:rid-11 -->"})
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
        "### Other findings\n\n**P1 · Blocking** Keep overrides · `src/a.rs:20-21` · not inline: lines not in the diff\n\nExplain."
    ));
    // An inline finding is only named in the hidden data.
    assert!(!plan
        .body
        .split("<!-- orbit-data")
        .next()
        .unwrap()
        .contains("rid-11"));
    assert!(!plan.body.contains("orbit-finding:"));
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
fn the_review_holds_the_reviewers_report_and_data_for_agents() {
    let mut inline = finding(Some("src/a.rs"), 11, 12);
    inline.number = Some(3);
    inline.category = Some(Category::Security);
    inline.title = "Ends <!-- a comment --> early & <b>bold</b>".into();
    inline.other_locations = vec![Location {
        path: None,
        start_line: 2,
        end_line: 2,
    }];
    let mut outside = finding(None, 3, 3);
    outside.number = Some(4);
    outside.priority = 2;
    let mut r = review(vec![inline.clone(), outside]);
    r.report = "[P1] Keep overrides\n[P2] Another".into();
    r.not_checked = vec!["cargo test".into()];
    r.completed = Some(true);
    let mut earlier = finding(Some("src/a.rs"), 30, 30);
    earlier.id = "old-0".into();
    earlier.number = Some(1);
    r.previous_round = Some(PreviousRound {
        review_id: "old".into(),
        head_sha: "c".repeat(40),
        verdict: None,
        findings: vec![earlier],
        decisions: vec![],
        comments_included: 0,
        comments_omitted: 0,
    });
    r.prior_findings = vec![PriorFinding {
        id: "old-0".into(),
        status: PriorStatus::Fixed,
        note: "Done.".into(),
    }];
    let body = plan(&r, &files(), Some(2), true).body;
    assert!(
        body.contains("<details>\n<summary>Codex’s own report</summary>\n\n[P1] Keep overrides\n[P2] Another\n\n</details>"),
        "{body}"
    );
    assert!(
        body.contains("| fixed | F1 · P1 Keep overrides | Done. |"),
        "{body}"
    );
    // The data can't end its HTML comment early, and reads back exactly.
    let hidden = body.split("<!-- orbit-data ").nth(1).unwrap();
    assert!(!hidden.split_once(" -->").unwrap().0.contains(['<', '>']));
    let v = data(&body);
    assert_eq!(v["version"], 1);
    assert_eq!(v["review"], "rid");
    assert_eq!(v["round"], 2);
    assert_eq!(v["reviewer"], "codex");
    assert_eq!(v["verdict"], "changes_requested");
    assert_eq!(
        (v["blocking"].clone(), v["blockingPriority"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(v["notChecked"], json!(["cargo test"]));
    assert_eq!(v["completed"], true);
    let f = &v["findings"][0];
    assert_eq!(f["title"], inline.title.as_str());
    assert_eq!(f["shortId"], "F3");
    assert_eq!(f["blocking"], true);
    assert_eq!(f["category"], "security");
    assert_eq!(f["problem"], "Explain.");
    assert_eq!(f["confirmed"], serde_json::Value::Null);
    assert_eq!(
        f["locations"],
        json!([{"path":"src/a.rs","startLine":11,"endLine":12},{"path":null,"startLine":2,"endLine":2}])
    );
    assert_eq!(
        (f["inline"].clone(), f["notInlineBecause"].clone()),
        (json!(true), json!(null))
    );
    let f = &v["findings"][1];
    assert_eq!(
        (f["shortId"].clone(), f["blocking"].clone()),
        (json!("F4"), json!(false))
    );
    assert_eq!(f["notInlineBecause"], "outside the repository");
    assert_eq!(
        v["earlierFindings"],
        json!([{"id":"old-0","shortId":"F1","title":"Keep overrides","priority":1,"status":"fixed","note":"Done."}])
    );
}
#[test]
fn leaves_out_the_data_then_the_report_to_fit_githubs_limit() {
    let mut r = review(vec![finding(Some("src/a.rs"), 11, 12)]);
    let short = plan(&r, &files(), None, true).body;
    let start = short.find("<!-- orbit-data ").unwrap();
    let data_len = short[start..].find(" -->\n").unwrap() + 5;
    let wrapper = "<details>\n<summary>Codex’s own report</summary>\n\n\n\n</details>\n\n".len();
    // A report that fits only without the data.
    r.report = "y".repeat(MAX_BODY - (short.len() - data_len) - wrapper);
    let body = plan(&r, &files(), None, true).body;
    assert_eq!(body.len(), MAX_BODY);
    assert!(body.contains("<summary>Codex’s own report</summary>") && !body.contains("orbit-data"));
    // A report too long to include at all: the data comes back.
    r.report = "y".repeat(MAX_BODY);
    let body = plan(&r, &files(), None, true).body;
    assert!(body.contains("Codex’s own report is too long to include here; it is kept in Orbit."));
    assert!(!body.contains("<details>"));
    assert_eq!(data(&body)["findings"][0]["id"], "rid-11");
}
#[test]
fn notes_when_the_list_has_fewer_findings_than_the_report() {
    let mut r = review(vec![finding(Some("src/a.rs"), 11, 12)]);
    r.report_findings = Some(1);
    assert_eq!(r.count_note(), None);
    r.report_findings = Some(3);
    let note = "Codex’s report lists 3 findings, but Orbit’s list has 1, so some may have been left out. Check the report.";
    assert_eq!(r.count_note().as_deref(), Some(note));
    assert!(plan(&r, &files(), None, true)
        .body
        .contains(&format!("One defect.\n\n**Note:** {note}\n\n")));
    // Earlier findings the reviewer still holds open count as listed.
    r.prior_findings = ["a", "b"]
        .map(|id| PriorFinding {
            id: id.into(),
            status: PriorStatus::StillOpen,
            note: String::new(),
        })
        .to_vec();
    assert_eq!(r.count_note(), None);
    r.report_findings = None;
    r.prior_findings.clear();
    assert_eq!(r.count_note(), None);
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
