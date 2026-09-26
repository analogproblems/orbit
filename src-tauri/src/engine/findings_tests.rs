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
        "findings": findings,
        "testsNotRun": ["cargo test"],
    })
    .to_string()
}
fn finding(path: &str) -> Value {
    json!({"title":"[P2] Keep overrides","priority":2,"path":path,"startLine":4,"endLine":9,"body":"Explain.","confidence":0.8})
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
    )
    .unwrap();
    assert_eq!(x.verdict, Verdict::ChangesRequested);
    assert_eq!(x.summary, "One defect.");
    assert_eq!(x.tests_not_run, ["cargo test"]);
    assert_eq!(x.findings[0].id, "r1-0");
    assert_eq!(x.findings[1].id, "r1-1");
    assert_eq!(x.findings[0].title, "Keep overrides");
    assert_eq!(x.findings[1].path.as_deref(), Some("b.rs"));
    assert_eq!((x.findings[0].start_line, x.findings[0].end_line), (4, 9));
}
#[test]
fn refuses_output_that_does_not_match_the_schema() {
    let mut bad_priority = finding("a.rs");
    bad_priority["priority"] = json!(4);
    let mut backwards = finding("a.rs");
    backwards["endLine"] = json!(1);
    let mut confidence = finding("a.rs");
    confidence["confidence"] = json!(1.5);
    for text in [
        String::new(),
        "not json".into(),
        "{}".into(),
        answer(json!("none")),
        answer(json!([bad_priority])),
        answer(json!([backwards])),
        answer(json!([confidence])),
        json!({"verdict":"maybe","summary":"","findings":[],"testsNotRun":[]}).to_string(),
    ] {
        assert!(parse(&text, "r1", None).is_err(), "{text}");
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
    let x = parse(&answer(json!([finding("/etc/passwd")])), "r1", Some(wt)).unwrap();
    assert_eq!(x.findings.len(), 1);
    assert_eq!(x.findings[0].path, None);
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
    let findings = r.findings.unwrap();
    assert_eq!(findings[0].path.as_deref(), Some("src/a.rs"));
    assert_eq!(findings[0].id, format!("{}-0", r.id));
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
    leaked["body"] = json!(format!("Found ghp_{} in a fixture.", "a1".repeat(18)));
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
        "testsNotRun",
        "extracting",
        "extractionSelection",
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
}
