use super::{
    findings::{parse, relative_path},
    run::{apply_notification, Next},
    test_support::*,
    Engine,
};
use crate::types::*;
use serde_json::{json, Value};
use std::sync::Arc;

fn answer(findings: Value) -> String {
    json!({
        "verdict": "changes_requested",
        "summary": "One defect.",
        "completed": true,
        "findings": findings,
        "notChecked": ["cargo test"],
        "priorFindings": [],
    })
    .to_string()
}
fn finding(path: &str) -> Value {
    json!({
        "title": "[P2] Keep overrides", "priority": 2, "category": "bug",
        "locations": [{"path": path, "startLine": 4, "endLine": 9}],
        "problem": "Explain.", "failureScenario": null, "evidence": null, "confirmed": true,
        "suggestedFix": null, "doneWhen": null, "reviewerNotes": null, "confidence": 0.8,
    })
}
/// A review whose report is in and whose extraction turn is running.
async fn extracting(e: &Engine, worktree: &str) {
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.report = "[P2] Keep overrides".into();
        r.extracting = true;
        r.worktree = Some(worktree.into());
    })
    .await;
}
async fn deliver(e: &Arc<Engine>, text: &str, status: &str) {
    e.event(json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{"id":"json","type":"agentMessage","text":text}}})).await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":status}}})).await;
}

#[test]
fn parses_findings_with_stable_ids_and_without_priority_prefixes() {
    let x = parse(
        &answer(json!([finding("src/a.rs"), finding("./b.rs")])),
        "r1",
        None,
        None,
    )
    .unwrap();
    assert_eq!(x.verdict, Verdict::ChangesRequested);
    assert_eq!(x.summary, "One defect.");
    assert_eq!(x.not_checked, ["cargo test"]);
    assert!(x.completed);
    assert_eq!(x.findings[0].id, "r1-0");
    assert_eq!(x.findings[1].id, "r1-1");
    assert_eq!(x.findings[0].title, "Keep overrides");
    assert_eq!(x.findings[1].path.as_deref(), Some("b.rs"));
    assert_eq!((x.findings[0].start_line, x.findings[0].end_line), (4, 9));
}
#[test]
fn keeps_every_part_of_the_form() {
    let d = tempfile::tempdir().unwrap();
    let wt = d.path().to_str().unwrap();
    let mut f = finding(&format!("{wt}/src/lib.rs"));
    f["category"] = json!("data_loss");
    f["locations"] = json!([
        {"path": format!("{wt}/src/lib.rs"), "startLine": 3, "endLine": 3},
        {"path": "src/main.rs", "startLine": 40, "endLine": 41},
        {"path": "src/main.rs", "startLine": 40, "endLine": 41},
        {"path": "src/lib.rs", "startLine": 3, "endLine": 3},
        {"path": "/etc/passwd", "startLine": 1, "endLine": 1},
        {"path": "src/main.rs", "startLine": 40, "endLine": 41},
    ]);
    f["problem"] = json!("first() returns the second item.");
    f["failureScenario"] = json!("A one-item list returns nothing.");
    f["evidence"] = json!("Read src/lib.rs:3 and its caller.");
    f["confirmed"] = json!(false);
    f["suggestedFix"] = json!("  ");
    f["doneWhen"] = json!("A test covers a one-item list.");
    f["reviewerNotes"] = json!("Rated high by the reviewer.");
    let x = parse(&answer(json!([f])), "r1", Some(wt), None).unwrap();
    let f = &x.findings[0];
    assert_eq!(
        (f.path.as_deref(), f.start_line, f.end_line),
        (Some("src/lib.rs"), 3, 3)
    );
    // Repeats of a place are dropped; a place outside the worktree keeps
    // its lines without a path.
    assert_eq!(
        f.other_locations,
        [
            Location {
                path: Some("src/main.rs".into()),
                start_line: 40,
                end_line: 41
            },
            Location {
                path: None,
                start_line: 1,
                end_line: 1
            },
        ]
    );
    assert_eq!(f.body, "first() returns the second item.");
    assert_eq!(f.category, Some(Category::DataLoss));
    assert_eq!(f.confirmed, Some(false));
    assert_eq!(
        f.failure_scenario.as_deref(),
        Some("A one-item list returns nothing.")
    );
    assert_eq!(
        f.evidence.as_deref(),
        Some("Read src/lib.rs:3 and its caller.")
    );
    // A blank field is the same as null.
    assert_eq!(f.suggested_fix, None);
    assert_eq!(
        f.done_when.as_deref(),
        Some("A test covers a one-item list.")
    );
    assert_eq!(
        f.reviewer_notes.as_deref(),
        Some("Rated high by the reviewer.")
    );
    assert_eq!(
        f.details(),
        "_Data loss · Likely, not verified_\n\
         **Breaks when:** A one-item list returns nothing.\n\
         **Evidence:** Read src/lib.rs:3 and its caller.\n\
         **Done when:** A test covers a one-item list.\n\
         **Also at:** `src/main.rs:40-41`, `(outside the repository)`\n\
         **Reviewer's notes:** Rated high by the reviewer."
    );
    let mut v: Value = serde_json::from_str(&answer(json!([]))).unwrap();
    v["completed"] = json!(false);
    assert!(!parse(&v.to_string(), "r1", None, None).unwrap().completed);
}
#[test]
fn the_app_knows_the_same_categories() {
    let fixture: Vec<String> = serde_json::from_str(include_str!(
        "../../../test-fixtures/finding-categories.json"
    ))
    .unwrap();
    assert_eq!(fixture, Category::ALL.map(Category::key));
    for c in Category::ALL {
        assert_eq!(serde_json::to_value(c).unwrap(), c.key());
    }
}
#[test]
fn findings_and_reviews_saved_before_the_new_form_still_load() {
    let f: Finding = serde_json::from_value(json!({"id":"r0-0","title":"Keep overrides",
        "priority":1,"path":"a.rs","startLine":4,"endLine":9,"body":"Explain and fix.","confidence":0.8}))
    .unwrap();
    assert_eq!(f.body, "Explain and fix.");
    assert!(f.category.is_none() && f.confirmed.is_none() && f.other_locations.is_empty());
    assert_eq!(f.details(), "");
    let mut r = serde_json::to_value(Review::new(
        &crate::types::tests::pr(),
        &Settings::default(),
        true,
    ))
    .unwrap();
    let review = r.as_object_mut().unwrap();
    review.remove("notChecked");
    review.remove("completed");
    review.insert("testsNotRun".into(), json!(["cargo test"]));
    let r: Review = serde_json::from_value(r).unwrap();
    assert_eq!(r.not_checked, ["cargo test"]);
    assert_eq!(r.completed, None);
}
#[test]
fn refuses_output_that_does_not_match_the_schema() {
    let mut bad_priority = finding("a.rs");
    bad_priority["priority"] = json!(4);
    let mut backwards = finding("a.rs");
    backwards["locations"][0]["endLine"] = json!(1);
    let mut confidence = finding("a.rs");
    confidence["confidence"] = json!(1.5);
    let mut category = finding("a.rs");
    category["category"] = json!("style");
    let mut nowhere = finding("a.rs");
    nowhere["locations"] = json!([]);
    let mut unsure = finding("a.rs");
    unsure["confirmed"] = json!(null);
    let mut evidence = finding("a.rs");
    evidence["evidence"] = json!(["read a.rs"]);
    // The form before #56, with one location and a body.
    let old = json!({"title":"Keep overrides","priority":2,"path":"a.rs","startLine":4,"endLine":9,
        "body":"Explain.","confidence":0.8});
    let mut incomplete: Value = serde_json::from_str(&answer(json!([]))).unwrap();
    incomplete.as_object_mut().unwrap().remove("completed");
    for text in [
        String::new(),
        "not json".into(),
        "{}".into(),
        answer(json!("none")),
        answer(json!([bad_priority])),
        answer(json!([backwards])),
        answer(json!([confidence])),
        answer(json!([category])),
        answer(json!([nowhere])),
        answer(json!([unsure])),
        answer(json!([evidence])),
        answer(json!([old])),
        incomplete.to_string(),
        json!({"verdict":"maybe","summary":"","completed":true,"findings":[],"notChecked":[],"priorFindings":[]})
            .to_string(),
        json!({"verdict":"clean","summary":"","completed":true,"findings":[],"notChecked":[]}).to_string(),
        json!({"verdict":"clean","summary":"","completed":true,"findings":[],"notChecked":[],
            "priorFindings":[{"id":"r0-0","status":"ignored","note":""}]})
        .to_string(),
    ] {
        assert!(parse(&text, "r1", None, None).is_err(), "{text}");
    }
}
#[test]
fn makes_worktree_paths_relative_and_rejects_outside_paths() {
    let d = tempfile::tempdir().unwrap();
    let wt = d.path().to_str().unwrap();
    assert_eq!(
        relative_path(&format!("{wt}/src/lib.rs"), Some(wt)).as_deref(),
        Some("src/lib.rs")
    );
    assert_eq!(
        relative_path("src/./lib.rs", Some(wt)).as_deref(),
        Some("src/lib.rs")
    );
    // A canonical spelling of the worktree also counts as inside it.
    let canonical = std::fs::canonicalize(d.path()).unwrap();
    assert_eq!(
        relative_path(&format!("{}/x.rs", canonical.display()), Some(wt)).as_deref(),
        Some("x.rs")
    );
    for outside in ["/etc/passwd", "../secret.rs", "src/../../x.rs", "", wt] {
        assert_eq!(relative_path(outside, Some(wt)), None, "{outside}");
    }
    assert_eq!(relative_path(&format!("{wt}/a.rs"), None), None);
    // The finding is kept; only its path is dropped.
    let x = parse(
        &answer(json!([finding("/etc/passwd")])),
        "r1",
        Some(wt),
        None,
    )
    .unwrap();
    assert_eq!(x.findings.len(), 1);
    assert_eq!(x.findings[0].path, None);
}
fn previous() -> PreviousRound {
    PreviousRound {
        review_id: "r0".into(),
        head_sha: "0".repeat(40),
        verdict: Some(Verdict::ChangesRequested),
        findings: parse(
            &answer(json!([finding("a.rs"), finding("b.rs")])),
            "r0",
            None,
            None,
        )
        .unwrap()
        .findings,
        decisions: vec![],
        comments_included: 0,
        comments_omitted: 0,
    }
}
fn with_prior(prior: Value) -> String {
    let mut v: Value = serde_json::from_str(&answer(json!([]))).unwrap();
    v["priorFindings"] = prior;
    v.to_string()
}
#[test]
fn keeps_classifications_of_known_earlier_findings_only() {
    let text = with_prior(json!([
        {"id":"r0-0","status":"fixed","note":"Guarded now."},
        {"id":"r0-1","status":"declined_disputed","note":"Still loses overrides."},
        {"id":"r0-0","status":"still_open","note":"Duplicate."},
        {"id":"made-up","status":"fixed","note":"Unknown."},
    ]));
    let x = parse(&text, "r1", None, Some(&previous())).unwrap();
    assert_eq!(
        x.prior_findings,
        [
            PriorFinding {
                id: "r0-0".into(),
                status: PriorStatus::Fixed,
                note: "Guarded now.".into()
            },
            PriorFinding {
                id: "r0-1".into(),
                status: PriorStatus::DeclinedDisputed,
                note: "Still loses overrides.".into()
            },
        ]
    );
    // Without a previous round, every classification is unknown.
    assert!(parse(&text, "r1", None, None)
        .unwrap()
        .prior_findings
        .is_empty());
}
#[test]
fn unresolved_earlier_findings_rule_out_a_clean_verdict() {
    let clean = |status: &str| {
        let mut v: Value = serde_json::from_str(&with_prior(json!([
            {"id":"r0-0","status":status,"note":"."},
        ])))
        .unwrap();
        v["verdict"] = json!("clean");
        parse(&v.to_string(), "r1", None, Some(&previous()))
            .unwrap()
            .verdict
    };
    assert_eq!(clean("still_open"), Verdict::ChangesRequested);
    assert_eq!(clean("declined_disputed"), Verdict::ChangesRequested);
    assert_eq!(clean("fixed"), Verdict::Clean);
    assert_eq!(clean("declined_accepted"), Verdict::Clean);
}
#[tokio::test]
async fn stores_prior_findings_and_they_survive_a_restart() {
    let (d, e, _) = running_engine().await;
    extracting(&e, d.path().to_str().unwrap()).await;
    e.update(|s| s.reviews[0].previous_round = Some(previous()))
        .await;
    let text = with_prior(json!([
        {"id":"r0-1","status":"declined_accepted","note":"Intended behaviour."},
    ]));
    deliver(&e, &text, "completed").await;
    e.flush().await.unwrap();
    drop(e);
    let r = Engine::new(d.path().into(), |_| {})
        .unwrap()
        .snapshot()
        .await
        .reviews
        .remove(0);
    assert_eq!(r.status, "ready");
    assert_eq!(r.previous_round, Some(previous()));
    assert_eq!(r.prior_findings[0].status, PriorStatus::DeclinedAccepted);
    assert_eq!(r.prior_findings[0].note, "Intended behaviour.");
}
#[tokio::test]
async fn completed_review_turn_starts_extraction_and_stays_active() {
    let (_d, e, id) = running_engine().await;
    e.update(|s| s.reviews[0].report = "Report".into()).await;
    let mut s = e.snapshot().await;
    let next = apply_notification(
        &mut s,
        &id,
        "turn/completed",
        &json!({"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}),
    );
    assert_eq!(next, Next::Extract);
    assert!(s.reviews[0].extracting);
    assert_eq!(s.reviews[0].status, "reviewing");
}
#[tokio::test]
async fn stores_extracted_findings_without_adding_chat_messages() {
    let (d, e, _) = running_engine().await;
    let wt = d.path().to_str().unwrap().to_string();
    extracting(&e, &wt).await;
    let text = answer(json!([finding(&format!("{wt}/src/a.rs"))]));
    e.event(json!({"method":"item/agentMessage/delta","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"json","delta":text}})).await;
    deliver(&e, &text, "completed").await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "ready");
    assert!(!r.extracting && r.error.is_none());
    assert!(r.messages.is_empty(), "{:?}", r.messages);
    assert_eq!(r.verdict, Some(Verdict::ChangesRequested));
    // A P2 finding does not block at the default blocking priority (P1).
    assert_eq!(
        (r.blocking, r.orbit_verdict),
        (Some(0), Some(Verdict::Clean))
    );
    let findings = r.findings.unwrap();
    assert_eq!(findings[0].path.as_deref(), Some("src/a.rs"));
    assert_eq!(findings[0].id, format!("{}-0", r.id));
}
#[tokio::test]
async fn a_review_that_could_not_check_the_change_fails_without_publishing() {
    let (d, e, _) = running_engine().await;
    automatic_extracting(&e, d.path().to_str().unwrap()).await;
    let mut v: Value = serde_json::from_str(&answer(json!([]))).unwrap();
    v["verdict"] = json!("clean");
    v["completed"] = json!(false);
    v["summary"] = json!("A command it needed was refused.");
    deliver(&e, &v.to_string(), "completed").await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "failed");
    assert!(!r.extracting);
    assert!(r.findings.is_none() && r.verdict.is_none() && r.blocking.is_none());
    assert_eq!(
        r.error.as_deref(),
        Some("Codex could not complete the review, so nothing was published. A command it needed was refused.")
    );
    assert!(r.comment_url.is_none());
}
#[tokio::test]
async fn invalid_or_failed_extraction_keeps_a_publishable_report() {
    for (text, status) in [
        ("", "completed"),
        ("{\"verdict\":1}", "completed"),
        ("", "failed"),
    ] {
        let (d, e, _) = running_engine().await;
        extracting(&e, d.path().to_str().unwrap()).await;
        deliver(&e, text, status).await;
        let r = e.snapshot().await.reviews.remove(0);
        assert_eq!(r.status, "ready");
        assert_eq!(r.report, "[P2] Keep overrides");
        assert!(r.findings.is_none() && r.verdict.is_none());
        assert!(r.error.as_deref().unwrap().contains("unstructured"));
        assert!(publishable(&r, &e.snapshot().await.prs[0]));
    }
}
#[tokio::test]
async fn scans_findings_for_credentials() {
    let (d, e, _) = running_engine().await;
    extracting(&e, d.path().to_str().unwrap()).await;
    let mut leaked = finding("a.rs");
    leaked["failureScenario"] = json!(format!("Found ghp_{} in a fixture.", "a1".repeat(18)));
    deliver(&e, &answer(json!([leaked])), "completed").await;
    let r = e.snapshot().await.reviews.remove(0);
    assert!(r.findings.is_some());
    assert_eq!(r.sensitive, ["GitHub token"]);
}
#[tokio::test]
async fn stopping_extraction_stops_the_run() {
    let (d, e, id) = running_engine().await;
    extracting(&e, d.path().to_str().unwrap()).await;
    e.cancel(&id).await.unwrap();
    deliver(&e, &answer(json!([])), "completed").await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "cancelled");
    assert!(!r.extracting && r.findings.is_none());
}
#[tokio::test]
async fn restart_or_disconnect_during_extraction_keeps_the_report() {
    let (d, e, _) = running_engine().await;
    extracting(&e, d.path().to_str().unwrap()).await;
    e.flush().await.unwrap();
    drop(e);
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "ready");
    assert!(r.error.unwrap().contains("restarted during extraction"));

    let (d, e, _) = running_engine().await;
    extracting(&e, d.path().to_str().unwrap()).await;
    e.event(json!({"method":"orbit/disconnected"})).await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "ready");
    assert!(r.error.unwrap().contains("disconnected during extraction"));
}
#[tokio::test]
async fn older_saved_reviews_load_without_findings() {
    let (d, e, _) = running_engine().await;
    e.flush().await.unwrap();
    drop(e);
    let path = d.path().join("state.json");
    let mut state: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let review = state["reviews"][0].as_object_mut().unwrap();
    for key in [
        "findings",
        "verdict",
        "summary",
        "notChecked",
        "completed",
        "extracting",
        "extractionSelection",
        "previousRound",
        "reviewPrompt",
        "priorFindings",
    ] {
        assert!(review.remove(key).is_some(), "{key}");
    }
    std::fs::write(&path, state.to_string()).unwrap();
    let r = Engine::new(d.path().into(), |_| {})
        .unwrap()
        .snapshot()
        .await
        .reviews
        .remove(0);
    assert!(r.findings.is_none() && r.verdict.is_none() && !r.extracting);
    assert!(r.previous_round.is_none() && r.review_prompt.is_none());
    assert!(r.prior_findings.is_empty());
}
/// An automatic review with a credential-shaped report is held with a
/// message when automatic publication runs, without any GitHub request.
async fn automatic_extracting(e: &Arc<Engine>, worktree: &str) {
    let report = format!("Found ghp_{} in the fixture.", "a1".repeat(18));
    ready_for_autopost(e, &report, true).await;
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.status = "reviewing".into();
        r.extracting = true;
        r.worktree = Some(worktree.into());
    })
    .await;
}
async fn wait_for_publication_attempt(e: &Engine) -> Review {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let r = e.snapshot().await.reviews[0].clone();
            if r.error
                .as_deref()
                .is_some_and(|m| m.contains("Not published automatically"))
            {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("automatic publication was not attempted")
}
#[tokio::test]
async fn automatic_reviews_recovered_from_extraction_are_still_published() {
    let (d, e, _) = running_engine().await;
    automatic_extracting(&e, d.path().to_str().unwrap()).await;
    e.event(json!({"method":"orbit/disconnected"})).await;
    let r = wait_for_publication_attempt(&e).await;
    assert_eq!(r.status, "ready");
    assert!(r.findings.is_none());

    let (d, e, _) = running_engine().await;
    automatic_extracting(&e, d.path().to_str().unwrap()).await;
    e.flush().await.unwrap();
    drop(e);
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    assert!(e.snapshot().await.reviews[0]
        .error
        .as_deref()
        .unwrap()
        .contains("restarted during extraction"));
    e.publish_recovered();
    assert_eq!(wait_for_publication_attempt(&e).await.status, "ready");
}
#[tokio::test]
async fn numbers_findings_after_every_earlier_one_on_the_pull_request() {
    let (d, e, _) = running_engine().await;
    e.update(|s| {
        let numbered = |number| Finding {
            number: Some(number),
            ..Default::default()
        };
        let mut earlier = s.reviews[0].clone();
        earlier.id = "earlier".into();
        earlier.findings = Some(vec![numbered(2)]);
        // A finding carried from a review that is gone still holds its number.
        earlier.previous_round = Some(PreviousRound {
            review_id: "gone".into(),
            head_sha: "c".repeat(40),
            verdict: None,
            findings: vec![numbered(5)],
            decisions: vec![],
            comments_included: 0,
            comments_omitted: 0,
        });
        let mut other_pr = earlier.clone();
        other_pr.id = "other".into();
        other_pr.pr_id = "a/b#2".into();
        other_pr.findings = Some(vec![numbered(9)]);
        s.reviews.extend([earlier, other_pr]);
    })
    .await;
    assert_eq!(
        super::findings::next_number(&e.snapshot().await, "earlier"),
        6
    );
    // The review's own earlier findings count even when their review is gone.
    e.update(|s| {
        let gone = s.reviews.iter().position(|r| r.id == "earlier").unwrap();
        let mut earlier = s.reviews.remove(gone);
        earlier.findings = Some(vec![]);
        let round = earlier.previous_round.take().unwrap();
        s.reviews[0].previous_round = Some(round);
        s.reviews.push(earlier);
    })
    .await;
    extracting(&e, d.path().to_str().unwrap()).await;
    deliver(
        &e,
        &answer(json!([finding("a.rs"), finding("b.rs")])),
        "completed",
    )
    .await;
    let r = e.snapshot().await.reviews.remove(0);
    let numbers: Vec<_> = r.findings.iter().flatten().map(|f| f.short_id()).collect();
    assert_eq!(numbers, [Some("F6".to_string()), Some("F7".to_string())]);
    // The report's one `[P2]` title is counted; the blocking priority used
    // is kept for each finding's label.
    assert_eq!(r.report_findings, Some(1));
    assert_eq!(
        r.blocking_priority,
        Some(Settings::default().blocking_priority)
    );
    assert_eq!(r.count_note(), None);
}
#[test]
fn counts_the_findings_in_each_reviewers_report() {
    use super::findings::report_count;
    let codex = "Two problems.\n\n- [P1] First — src/a.rs:3\n  Why.\n### [P3] Second\nThe [P2] in prose doesn't count.\n**[P0] Third**";
    assert_eq!(report_count(Provider::Codex, codex), Some(3));
    assert_eq!(report_count(Provider::Codex, "Looks good."), Some(0));
    let agy = "Not ready.\n\n### Problem 1: First\n`a.rs:3`\n### Problem 2: Second\nProblem 3 is not a heading.";
    assert_eq!(report_count(Provider::Antigravity, agy), Some(2));
    assert_eq!(
        report_count(
            Provider::Claude,
            &crate::claude::report(r#"[{"summary":"A"},{"summary":"B"}]"#)
        ),
        Some(2)
    );
    assert_eq!(
        report_count(Provider::Claude, &crate::claude::report("[]")),
        Some(0)
    );
    assert_eq!(
        report_count(Provider::Claude, "I looked and it seems fine."),
        None
    );
}
