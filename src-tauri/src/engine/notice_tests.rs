use super::{inline, Engine};
use crate::{attention::Notice, rpc::Rpc, types::*};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

type Seen = Arc<Mutex<Vec<Notice>>>;
fn engine(root: &Path) -> (Arc<Engine>, Seen) {
    let e = Engine::new(root.into(), |_| {}).unwrap();
    let seen: Seen = Arc::default();
    let sink = seen.clone();
    e.on_notice(move |n| sink.lock().unwrap().push(n.clone()));
    (e, seen)
}
fn titles(seen: &Seen) -> Vec<String> {
    seen.lock()
        .unwrap()
        .iter()
        .map(|n| n.title.clone())
        .collect()
}
fn repository(approval_minutes: u32) -> Repository {
    Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes,
        keep_updated: false,
        provider: Default::default(),
    }
}
fn run(automatic: bool, status: &str) -> Review {
    let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), automatic);
    r.status = status.into();
    r
}
/// A failed automatic review of the PR, which needs the developer.
async fn fail_a_review(e: &Engine) {
    e.update(|s| {
        s.settings.repositories = vec![repository(0)];
        s.prs = vec![crate::types::tests::pr()];
        s.reviews.push(run(true, "failed"));
    })
    .await;
}

#[tokio::test]
async fn announces_each_new_item_once_even_after_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let (e, seen) = engine(d.path());
    fail_a_review(&e).await;
    e.flush().await.unwrap();
    e.flush().await.unwrap();
    assert_eq!(titles(&seen), ["An automatic review stopped"]);
    let notice = seen.lock().unwrap()[0].clone();
    assert!(notice.body.starts_with("a/b#1 · Fix\n"));
    assert_eq!(notice.item.kind, AttentionKind::Failed);
    drop(e);
    let (e, seen) = engine(d.path());
    e.flush().await.unwrap();
    assert!(titles(&seen).is_empty());
    // Once resolved, the same kind of problem is new again.
    e.update(|s| s.reviews.push(run(true, "reviewing"))).await;
    e.flush().await.unwrap();
    assert!(e.snapshot().await.announced.is_empty());
    e.update(|s| s.reviews[1].status = "failed".into()).await;
    e.flush().await.unwrap();
    assert_eq!(titles(&seen), ["An automatic review stopped"]);
}
#[tokio::test]
async fn with_notifications_off_items_still_show_in_the_app() {
    let d = tempfile::tempdir().unwrap();
    let (e, seen) = engine(d.path());
    e.update(|s| s.settings.notify = false).await;
    fail_a_review(&e).await;
    e.flush().await.unwrap();
    assert!(titles(&seen).is_empty());
    assert_eq!(e.shown_snapshot().await.attention.len(), 1);
    // Turning them back on does not announce what is already open.
    e.update(|s| s.settings.notify = true).await;
    e.flush().await.unwrap();
    assert!(titles(&seen).is_empty());
}

/// A stand-in for Codex that saves what Orbit sends it.
async fn recording_codex(dir: &Path) -> Arc<Rpc> {
    let file = dir.join("sent");
    let mut cmd = tokio::process::Command::new("node");
    cmd.args([
        "-e",
        "const fs = require('fs');\
         require('readline').createInterface({ input: process.stdin }).on('line', (l) => {\
           fs.appendFileSync(process.argv[1], l + '\\n');\
           const m = JSON.parse(l);\
           if (m.method === 'initialize') console.log(JSON.stringify({ id: m.id, result: {} }));\
         });",
    ])
    .arg(&file);
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    Rpc::from_command(cmd, tx).await.unwrap()
}
fn approval(id: u64, review_id: &str, minutes_ago: i64) -> Approval {
    Approval {
        id: id.into(),
        review_id: review_id.into(),
        method: "item/commandExecution/requestApproval".into(),
        params: json!({"command": "cargo test"}),
        requested_at: (chrono::Utc::now() - chrono::Duration::minutes(minutes_ago))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }
}
#[tokio::test]
async fn declines_only_automatic_approvals_past_their_repositorys_limit() {
    let d = tempfile::tempdir().unwrap();
    let (e, _) = engine(&d.path().join("state"));
    *e.rpc.lock().await = Some(recording_codex(d.path()).await);
    let automatic = run(true, "awaitingApproval");
    let manual = run(false, "awaitingApproval");
    let (a, m) = (automatic.id.clone(), manual.id.clone());
    e.update(|s| {
        s.settings.repositories = vec![repository(5)];
        s.prs = vec![crate::types::tests::pr()];
        s.reviews = vec![automatic, manual];
        s.approvals = vec![approval(1, &a, 6), approval(2, &a, 4), approval(3, &m, 60)];
    })
    .await;
    e.expire_approvals().await;
    let s = e.snapshot().await;
    let left: Vec<_> = s.approvals.iter().map(|a| a.id.clone()).collect();
    assert_eq!(left, [json!(2), json!(3)]);
    let note =
        "Declined the reviewer's command `cargo test` because no one approved it within 5 minutes.";
    assert_eq!(s.reviews[0].declined_approvals, [note]);
    assert_eq!(s.reviews[0].messages.last().unwrap().text, note);
    assert!(s.reviews[1].declined_approvals.is_empty());
    let sent = d.path().join("sent");
    tokio::time::timeout(Duration::from_secs(3), async {
        while !std::fs::read_to_string(&sent).is_ok_and(|t| t.contains("decline")) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let sent = std::fs::read_to_string(&sent).unwrap();
    let line = sent.lines().find(|l| l.contains("decline")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(line).unwrap(),
        json!({"id": 1, "result": {"decision": "decline"}})
    );
    // The note is published with the review.
    let plan = inline::plan(&s.reviews[0], &Default::default(), None, true);
    assert!(plan.body.contains(note));
    // Without a limit, approvals wait however long it takes.
    e.update(|s| s.settings.repositories = vec![repository(0)])
        .await;
    e.update(|s| s.approvals.push(approval(4, &a, 600))).await;
    e.expire_approvals().await;
    assert_eq!(e.snapshot().await.approvals.len(), 3);
}
#[test]
fn approval_time_limits_are_at_most_a_day() {
    let mut settings = Settings {
        repositories: vec![repository(MAX_APPROVAL_MINUTES)],
        ..Settings::default()
    };
    assert!(settings.validate().is_ok());
    settings.repositories[0].approval_minutes = MAX_APPROVAL_MINUTES + 1;
    assert!(settings.validate().unwrap_err().contains("24 hours"));
}
/// An automatic review in `a/b` with a five-minute limit and one request
/// that has waited six minutes; returns its review ID.
async fn expired_request(e: &Engine, dir: &Path) -> String {
    *e.rpc.lock().await = Some(recording_codex(dir).await);
    let r = run(true, "awaitingApproval");
    let id = r.id.clone();
    e.update(|s| {
        s.settings.repositories = vec![repository(5)];
        s.prs = vec![crate::types::tests::pr()];
        s.reviews = vec![r];
        s.approvals = vec![approval(1, &id, 6)];
    })
    .await;
    id
}
async fn decisions(dir: &Path) -> Vec<String> {
    // Writes reach the stand-in asynchronously; give them a moment.
    tokio::time::sleep(Duration::from_millis(200)).await;
    std::fs::read_to_string(dir.join("sent"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("decision"))
        .map(str::to_owned)
        .collect()
}
#[tokio::test]
async fn the_time_limit_never_answers_a_request_already_being_answered() {
    let d = tempfile::tempdir().unwrap();
    let (e, _) = engine(&d.path().join("state"));
    expired_request(&e, d.path()).await;
    // The developer's answer is on its way.
    e.answering.lock().unwrap().insert(json!(1).to_string());
    e.expire_approvals().await;
    assert!(e.snapshot().await.reviews[0].declined_approvals.is_empty());
    assert!(decisions(d.path()).await.is_empty());
    e.answering.lock().unwrap().clear();
    // Racing for real, exactly one answer reaches Codex, and the note is
    // written only when the time limit's decline is that answer.
    let (accepted, ()) = tokio::join!(e.decide(json!(1), true), e.expire_approvals());
    let sent = decisions(d.path()).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    let declined = e.snapshot().await.reviews[0].declined_approvals.len();
    if accepted.is_ok() {
        assert!(sent[0].contains("accept"));
        assert_eq!(declined, 0);
    } else {
        assert!(sent[0].contains("decline"));
        assert_eq!(declined, 1);
    }
    assert!(e.snapshot().await.approvals.is_empty());
}
#[tokio::test]
async fn a_follow_up_on_an_automatic_review_always_waits() {
    let d = tempfile::tempdir().unwrap();
    let (e, _) = engine(&d.path().join("state"));
    expired_request(&e, d.path()).await;
    e.update(|s| {
        s.reviews[0].is_review = false;
        s.reviews[0].before_follow_up = Some(Outcome {
            status: "posted".into(),
            error: None,
        });
    })
    .await;
    e.expire_approvals().await;
    let s = e.snapshot().await;
    assert_eq!(s.approvals.len(), 1);
    assert!(s.reviews[0].declined_approvals.is_empty());
}
#[test]
fn an_unstructured_export_keeps_its_declined_requests() {
    let mut r = run(true, "ready");
    r.report = "Report".into();
    r.declined_approvals = vec!["Declined the reviewer's command.".into()];
    let s = Snapshot {
        prs: vec![crate::types::tests::pr()],
        reviews: vec![r.clone()],
        ..Default::default()
    };
    let (_, body) = crate::storage::report(&s, &r.id).unwrap();
    assert!(body.contains("unstructured run"));
    assert!(body.contains("Declined the reviewer's command."));
}
