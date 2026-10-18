use super::inline::{hunks, plan};
use super::Engine;
use crate::ledger::{self, FinalStatus, FlagKind, History, RoundVerdict, StoredPr};
use crate::types::*;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

fn finding(review: &str, number: u32, path: &str, line: u32, title: &str) -> Finding {
    Finding {
        id: format!("{review}-{number}"),
        number: Some(number),
        title: title.into(),
        priority: 1,
        path: Some(path.into()),
        start_line: line,
        end_line: line,
        body: format!("{title}, explained."),
        confidence: 0.8,
        category: Some(Category::Bug),
        failure_scenario: Some("an empty list arrives.".into()),
        ..Default::default()
    }
}
fn review(id: &str, head: &str, findings: Vec<Finding>, blocking: u32) -> Review {
    let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), false);
    r.id = id.into();
    r.head_sha = head.repeat(40);
    r.base_sha = "b".repeat(40);
    let verdict = if blocking > 0 {
        Verdict::ChangesRequested
    } else {
        Verdict::Clean
    };
    r.verdict = Some(verdict);
    r.orbit_verdict = Some(verdict);
    r.blocking = Some(blocking);
    r.blocking_priority = Some(1);
    r.summary = Some("What this round found.".into());
    r.findings = Some(findings);
    r
}
fn claim(
    finding_id: &str,
    kind: ClaimKind,
    commit: Option<&str>,
    reason: Option<&str>,
) -> AuthorClaim {
    AuthorClaim {
        finding_id: finding_id.into(),
        author: "jason".into(),
        created_at: "2026-10-11T10:30:00Z".into(),
        kind,
        commit: commit.map(str::to_owned),
        commit_check: None,
        reason: reason.map(str::to_owned),
    }
}
fn posted_review(id: u64, at: &str, body: &str) -> Value {
    json!({
        "id": id, "user": {"login": "jason"}, "body": body, "submitted_at": at,
        "html_url": format!("https://github.com/a/b/pull/1#pullrequestreview-{id}")
    })
}
fn line_comment(id: u64, review: Option<u64>, at: &str, c: Value) -> Value {
    let mut c = c;
    c["id"] = json!(id);
    c["pull_request_review_id"] = json!(review);
    c["user"] = json!({"login": "jason"});
    c["created_at"] = json!(at);
    c["updated_at"] = json!(at);
    c["html_url"] = json!(format!("https://github.com/a/b/pull/1#discussion_r{id}"));
    c
}
fn issue_comment(id: u64, at: &str, body: &str) -> Value {
    json!({
        "id": id, "user": {"login": "jason"}, "body": body, "created_at": at, "updated_at": at,
        "html_url": format!("https://github.com/a/b/pull/1#issuecomment-{id}")
    })
}
fn pr_json(number: u64, state: &str, updated_at: &str) -> Value {
    json!({
        "number": number, "title": format!("Change {number}"), "state": state,
        "html_url": format!("https://github.com/a/b/pull/{number}"),
        "user": {"login": "jason"}, "body": "Before: one.\n\nAfter: two.",
        "created_at": "2026-10-11T09:00:00Z", "updated_at": updated_at,
        "merged_at": null, "head": {"ref": "fix", "sha": "c".repeat(40)}, "base": {"ref": "master"}
    })
}

/// Two rounds of Orbit's own reviews as Orbit publishes them: the first
/// with one finding inline and one in the text, then a reply in the inline
/// thread, a decline in the conversation, and a second round that rules on
/// both.
fn two_orbit_rounds(with_data: bool) -> History {
    let files = hunks(&json!([
        {"filename": "src/a.rs", "patch": "@@ -10,4 +10,5 @@ fn a() {\n ctx\n+add\n ctx\n ctx\n ctx"}
    ]));
    let first = vec![
        finding("rev1", 1, "src/a.rs", 11, "Guard the empty list"),
        finding("rev1", 2, "src/b.rs", 3, "Token reaches the log"),
    ];
    let r1 = review("rev1", "a", first.clone(), 2);
    let mut r2 = review("rev2", "c", vec![], 1);
    r2.previous_round = Some(PreviousRound {
        review_id: "rev1".into(),
        head_sha: "a".repeat(40),
        verdict: Some(Verdict::ChangesRequested),
        findings: first,
        decisions: vec![],
        claims: vec![
            claim("rev1-1", ClaimKind::Fixed, Some("abc1234"), None),
            claim(
                "rev1-2",
                ClaimKind::Declined,
                None,
                Some("only tests reach it"),
            ),
        ],
        comments_included: 2,
        comments_omitted: 0,
    });
    r2.prior_findings = vec![
        PriorFinding {
            id: "rev1-1".into(),
            status: PriorStatus::Fixed,
            note: "Guarded.".into(),
        },
        PriorFinding {
            id: "rev1-2".into(),
            status: PriorStatus::DeclinedDisputed,
            note: "The CLI reaches it too.".into(),
        },
    ];
    let strip = |body: String| {
        if with_data {
            return body;
        }
        let start = body.find("<!-- orbit-data ").unwrap();
        let end = start + body[start..].find(" -->\n").unwrap() + 5;
        format!("{}{}", &body[..start], &body[end..])
    };
    let p1 = plan(&r1, &files, Some(1), true);
    let p2 = plan(&r2, &files, Some(2), true);
    assert_eq!(p1.comments.len(), 1, "F1 is inline, F2 in the text");
    let reviews = [
        posted_review(100, "2026-10-11T10:00:00Z", &strip(p1.body)),
        posted_review(200, "2026-10-11T11:00:00Z", &strip(p2.body)),
    ];
    let review_comments = [
        line_comment(
            101,
            Some(100),
            "2026-10-11T10:00:00Z",
            p1.comments[0].clone(),
        ),
        line_comment(
            102,
            None,
            "2026-10-11T10:30:00Z",
            json!({"path": "src/a.rs", "line": 11, "in_reply_to_id": 101, "body": "Fixed in abc1234"}),
        ),
    ];
    let issue_comments = [issue_comment(
        300,
        "2026-10-11T10:31:00Z",
        "Declined F2: only tests reach it",
    )];
    let stored = ledger::from_github(
        "a/b",
        &pr_json(1, "open", "2026-10-11T11:00:00Z"),
        &issue_comments,
        &reviews,
        &review_comments,
        &[],
        &[],
        None,
        "2026-10-12T00:00:00Z",
    );
    ledger::derive(&stored)
}

#[test]
fn orbit_reviews_read_back_with_and_without_their_hidden_data() {
    for with_data in [true, false] {
        let h = two_orbit_rounds(with_data);
        let rounds: Vec<(Option<u32>, RoundVerdict, Option<u32>)> = h
            .rounds
            .iter()
            .map(|r| (r.round, r.verdict, r.blocking))
            .collect();
        assert_eq!(
            rounds,
            [
                (Some(1), RoundVerdict::ChangesRequested, Some(2)),
                (Some(2), RoundVerdict::ChangesRequested, Some(1)),
            ],
            "with data: {with_data}"
        );
        let r1 = &h.rounds[0];
        assert_eq!(r1.reviewer.as_deref(), Some("Codex"));
        assert_eq!(r1.summary.as_deref(), Some("What this round found."));
        /// Short ID, title, whether inline, and the first place's file.
        type Seen<'a> = (Option<&'a str>, &'a str, Option<bool>, Option<&'a str>);
        let mut found: Vec<Seen> = r1
            .findings
            .iter()
            .map(|f| {
                (
                    f.short_id.as_deref(),
                    f.title.as_str(),
                    f.inline,
                    f.locations.first().and_then(|l| l.path.as_deref()),
                )
            })
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                (
                    Some("F1"),
                    "Guard the empty list",
                    Some(true),
                    Some("src/a.rs")
                ),
                (
                    Some("F2"),
                    "Token reaches the log",
                    Some(false),
                    Some("src/b.rs")
                ),
            ],
            "with data: {with_data}"
        );
        assert!(r1.findings.iter().all(|f| f.failure_scenario.as_deref()
            == Some("an empty list arrives.")
            && f.category == Some(Category::Bug)
            && f.blocking == Some(true)));
        let earlier: Vec<(Option<&str>, Option<PriorStatus>)> = h.rounds[1]
            .earlier_findings
            .iter()
            .map(|e| (e.short_id.as_deref(), e.status))
            .collect();
        assert_eq!(
            earlier,
            [
                (Some("F1"), Some(PriorStatus::Fixed)),
                (Some("F2"), Some(PriorStatus::DeclinedDisputed)),
            ]
        );
        let status = |id: &str| {
            h.trajectories
                .iter()
                .find(|t| t.short_id.as_deref() == Some(id))
                .unwrap()
        };
        let f1 = status("F1");
        assert_eq!(f1.final_status, FinalStatus::Fixed);
        assert_eq!(
            f1.claim.as_ref().and_then(|c| c.commit.as_deref()),
            Some("abc1234"),
            "the reply in the inline thread answers F1"
        );
        let f2 = status("F2");
        assert_eq!(f2.final_status, FinalStatus::DeclinedDisputed);
        assert_eq!(
            f2.claim.as_ref().and_then(|c| c.reason.as_deref()),
            Some("only tests reach it")
        );
        assert!(h
            .flags
            .iter()
            .any(|f| f.kind == FlagKind::DisputedDecline
                && f.finding_id == Some(f2.finding_id.clone())));
    }
}

fn repository(name: &str) -> Repository {
    Repository {
        name: name.into(),
        path: format!("/tmp/{name}"),
        automatic: false,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    }
}
/// A stand-in for `gh` answering each endpoint from a file named after it,
/// such as `repos_a_b_pulls_1.json`, and logging every call.
fn fake_gh(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let gh = dir.join("gh");
    let script = format!(
        r#"#!/bin/sh
dir='{}'
echo "$*" >> "$dir/log"
page=1
for a in "$@"; do
  case "$a" in
    repos/*) ep="$a" ;;
    page=*) page="${{a#page=}}" ;;
  esac
done
if [ "$page" != 1 ]; then echo '[]'; exit 0; fi
f="$dir/$(printf '%s' "$ep" | tr '/' '_').json"
if [ -e "$f" ]; then cat "$f"; else echo "HTTP 404: Not Found" >&2; exit 1; fi
"#,
        dir.display()
    );
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    gh
}
fn answer(dir: &Path, endpoint: &str, v: Value) {
    std::fs::write(
        dir.join(format!("{}.json", endpoint.replace('/', "_"))),
        v.to_string(),
    )
    .unwrap();
}
/// A pull request with one of Milo's reviews in its conversation.
fn serve_pr(dir: &Path, number: u64, state: &str, updated_at: &str, comments: Value) {
    let base = format!("repos/a/b/pulls/{number}");
    answer(dir, &base, pr_json(number, state, updated_at));
    answer(
        dir,
        &format!("repos/a/b/issues/{number}/comments"),
        comments,
    );
    for list in ["reviews", "comments", "commits", "files"] {
        answer(dir, &format!("{base}/{list}"), json!([]));
    }
}
fn calls(dir: &Path, endpoint: &str) -> usize {
    std::fs::read_to_string(dir.join("log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.split(' ').any(|a| a == endpoint))
        .count()
}
async fn refresh(e: &Arc<Engine>, gh: &Path) {
    crate::github::GH
        .scope(gh.to_path_buf(), e.refresh_ledger())
        .await;
}

#[tokio::test]
async fn reads_history_in_the_background_and_only_what_changed() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    e.update(|s| {
        s.settings.repositories = vec![repository("a/b"), repository("c/d")];
    })
    .await;
    assert!(super::ledger::unread(&e.snapshot().await));
    let gh = fake_gh(d.path());
    let milo = "## Milo PR review\n\n**CHANGES REQUESTED**\n\n- Full head: `aaaaaaa`\n\n**F1 · P2 · Guard the empty list**\n\nIt panics.";
    answer(
        d.path(),
        "repos/a/b/pulls",
        json!([
            {"number": 2, "state": "open", "updated_at": "2026-10-11T12:00:00Z"},
            {"number": 1, "state": "closed", "updated_at": "2026-10-11T11:00:00Z"}
        ]),
    );
    serve_pr(
        d.path(),
        1,
        "closed",
        "2026-10-11T11:00:00Z",
        json!([issue_comment(1, "2026-10-11T10:00:00Z", milo)]),
    );
    serve_pr(
        d.path(),
        2,
        "open",
        "2026-10-11T12:00:00Z",
        json!([issue_comment(2, "2026-10-11T10:00:00Z", milo)]),
    );
    refresh(&e, &gh).await;

    let s = e.snapshot().await;
    assert!(!s.ledger.refreshing && s.ledger.progress.is_none());
    assert_eq!(s.ledger.version, 1);
    let ab = &s.ledger.repos[0];
    assert_eq!((ab.repo.as_str(), ab.error.as_deref()), ("a/b", None));
    assert!(ab.refreshed_at.is_some());
    let cd = &s.ledger.repos[1];
    assert!(cd.error.as_deref().unwrap().contains("404"), "{cd:?}");
    assert_eq!(cd.refreshed_at, None);
    assert!(!super::ledger::unread(&s));
    let shown = e.decisions().await.unwrap();
    let numbers: Vec<u64> = shown.prs.iter().map(|h| h.number).collect();
    assert_eq!(numbers, [2, 1]);
    assert_eq!(
        shown.prs[1].rounds[0].findings[0].title,
        "Guard the empty list"
    );

    // Nothing changed: within the hour, nothing is read again.
    refresh(&e, &gh).await;
    assert_eq!(calls(d.path(), "repos/a/b/pulls/1/reviews"), 1);
    assert_eq!(calls(d.path(), "repos/a/b/pulls/2/reviews"), 1);
    // The hourly pass, or Refresh, reads the open one too.
    *e.ledger_full_at.lock().unwrap() = None;
    refresh(&e, &gh).await;
    assert_eq!(calls(d.path(), "repos/a/b/pulls/1/reviews"), 1);
    assert_eq!(calls(d.path(), "repos/a/b/pulls/2/reviews"), 2);
    assert_eq!(e.snapshot().await.ledger.version, 1);

    // Milo edits its review in place for a new head; the earlier text is
    // kept, so both rounds show.
    let mut edited = issue_comment(
        2,
        "2026-10-11T10:00:00Z",
        "## Milo PR review\n\n**CLEAR TO MERGE**\n\n- Full head: `bbbbbbb`\n\n- **F1 fixed:** guarded.",
    );
    edited["updated_at"] = json!("2026-10-11T11:30:00Z");
    answer(d.path(), "repos/a/b/issues/2/comments", json!([edited]));
    refresh(&e, &gh).await;
    assert_eq!(e.snapshot().await.ledger.version, 1);
    *e.ledger_full_at.lock().unwrap() = None;
    refresh(&e, &gh).await;
    assert_eq!(e.snapshot().await.ledger.version, 2);
    let pr2 = e
        .decisions()
        .await
        .unwrap()
        .prs
        .into_iter()
        .find(|h| h.number == 2)
        .unwrap();
    let verdicts: Vec<RoundVerdict> = pr2.rounds.iter().map(|r| r.verdict).collect();
    assert_eq!(
        verdicts,
        [RoundVerdict::ChangesRequested, RoundVerdict::Clean]
    );
    assert_eq!(pr2.trajectories[0].final_status, FinalStatus::Fixed);

    // A disconnected repository's history is removed.
    e.update(|s| s.settings.repositories = vec![repository("c/d")])
        .await;
    refresh(&e, &gh).await;
    let s = e.snapshot().await;
    assert!(!d.path().join("state/ledger/a").exists());
    let repos: Vec<&str> = s.ledger.repos.iter().map(|l| l.repo.as_str()).collect();
    assert_eq!(repos, ["c/d"]);
    assert!(e.decisions().await.unwrap().prs.is_empty());
}

#[tokio::test]
async fn a_pull_request_that_fails_to_read_is_read_again_next_time() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    e.update(|s| s.settings.repositories = vec![repository("a/b")])
        .await;
    let gh = fake_gh(d.path());
    answer(
        d.path(),
        "repos/a/b/pulls",
        json!([
            {"number": 2, "state": "closed", "updated_at": "2026-10-11T12:00:00Z"},
            {"number": 1, "state": "closed", "updated_at": "2026-10-11T11:00:00Z"}
        ]),
    );
    serve_pr(d.path(), 2, "closed", "2026-10-11T12:00:00Z", json!([]));
    serve_pr(d.path(), 1, "closed", "2026-10-11T11:00:00Z", json!([]));
    std::fs::remove_file(d.path().join("repos_a_b_pulls_1_commits.json")).unwrap();
    refresh(&e, &gh).await;
    // #2 was saved before #1 failed, so the history still changed.
    let s = e.snapshot().await;
    assert!(s.ledger.repos[0].error.is_some());
    assert_eq!(s.ledger.version, 1);
    let numbers =
        |d: crate::ledger::Decisions| -> Vec<u64> { d.prs.iter().map(|h| h.number).collect() };
    assert_eq!(numbers(e.decisions().await.unwrap()), [2]);
    answer(d.path(), "repos/a/b/pulls/1/commits", json!([]));
    refresh(&e, &gh).await;
    let s = e.snapshot().await;
    assert_eq!(s.ledger.repos[0].error, None);
    assert_eq!(s.ledger.version, 2);
    assert_eq!(calls(d.path(), "repos/a/b/pulls/2/reviews"), 1);
    assert_eq!(numbers(e.decisions().await.unwrap()), [2, 1]);
    let stored: StoredPr =
        serde_json::from_slice(&std::fs::read(d.path().join("state/ledger/a/b/1.json")).unwrap())
            .unwrap();
    assert_eq!(stored.pr.title, "Change 1");
}

#[tokio::test]
async fn only_one_read_runs_at_a_time() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    assert!(e.refresh_ledger_soon());
    assert!(!e.refresh_ledger_soon());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while e
            .refreshing_ledger
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(e.refresh_ledger_soon());
}

#[tokio::test]
async fn exports_one_repositorys_decisions_or_every_one() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().join("state"), |_| {}).unwrap();
    e.update(|s| s.settings.repositories = vec![repository("a/b")])
        .await;
    assert_eq!(
        e.export_decisions(None).await.unwrap_err(),
        "There are no pull requests to export yet."
    );
    let gh = fake_gh(d.path());
    let milo = "## Milo PR review\n\n**CHANGES REQUESTED**\n\n- Full head: `aaaaaaa`\n\n**F1 · P2 · Guard the empty list**\n\nIt panics.";
    answer(
        d.path(),
        "repos/a/b/pulls",
        json!([{"number": 1, "state": "closed", "updated_at": "2026-10-11T11:00:00Z"}]),
    );
    serve_pr(
        d.path(),
        1,
        "closed",
        "2026-10-11T11:00:00Z",
        json!([issue_comment(1, "2026-10-11T10:00:00Z", milo)]),
    );
    refresh(&e, &gh).await;
    let (name, text) = e.export_decisions(Some("A/B".into())).await.unwrap();
    assert!(
        name.starts_with("orbit-decisions-a-b-20") && name.ends_with(".md"),
        "{name}"
    );
    assert!(text.starts_with("# Decisions: a/b\n"), "{text}");
    assert!(
        text.contains("- F1 · P2 · Guard the empty list: no ruling\n"),
        "{text}"
    );
    let (name, _) = e.export_decisions(Some("all".into())).await.unwrap();
    assert!(name.starts_with("orbit-decisions-all-"), "{name}");
    assert!(e.export_decisions(Some("c/d".into())).await.is_err());
}
