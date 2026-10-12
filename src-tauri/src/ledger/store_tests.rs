use super::*;
use serde_json::json;

fn pr_json() -> Value {
    json!({
        "number": 5,
        "title": "Add login",
        "html_url": "https://github.com/a/b/pull/5",
        "state": "open",
        "user": {"login": "jason"},
        "body": "Before: x.",
        "created_at": "2026-10-11T09:00:00Z",
        "updated_at": "2026-10-11T10:00:00Z",
        "merged_at": null,
        "head": {"ref": "feature", "sha": "abc1234"},
        "base": {"ref": "master"},
        "additions": 3,
        "deletions": 1,
        "changed_files": 2
    })
}
fn issue_comment(body: &str, updated_at: &str) -> Value {
    json!({
        "id": 7,
        "user": {"login": "milo"},
        "body": body,
        "html_url": "https://github.com/a/b/pull/5#issuecomment-7",
        "created_at": "2026-10-11T09:30:00Z",
        "updated_at": updated_at
    })
}
fn stored(comments: &[Value], previous: Option<&StoredPr>) -> StoredPr {
    from_github(
        "a/b",
        &pr_json(),
        comments,
        &[],
        &[],
        &[],
        &[],
        previous,
        "2026-10-12T00:00:00Z",
    )
}

#[test]
fn reads_reviews_and_comments_on_lines_with_their_places() {
    let reviews = [
        json!({"id": 1, "user": {"login": "milo"}, "body": "Looks fine", "html_url": "r", "submitted_at": "2026-10-11T09:40:00Z"}),
        json!({"id": 2, "user": {"login": "milo"}, "body": "draft", "html_url": "r2", "submitted_at": null}),
    ];
    let line_comments = [json!({
        "id": 3, "user": {"login": "milo"}, "body": "here", "html_url": "c",
        "created_at": "2026-10-11T09:41:00Z", "updated_at": "2026-10-11T09:41:00Z",
        "path": "src/a.rs", "line": null, "original_line": 12, "start_line": 10,
        "in_reply_to_id": 9, "pull_request_review_id": 1
    })];
    let commits = [
        json!({"sha": "abc1234", "html_url": "u", "commit": {"message": "Fix", "author": {"name": "Jason", "date": "2026-10-11T09:00:00Z"}}}),
    ];
    let files = [json!({"filename": "src/a.rs"})];
    let s = from_github(
        "a/b",
        &pr_json(),
        &[],
        &reviews,
        &line_comments,
        &commits,
        &files,
        None,
        "now",
    );
    assert_eq!(s.comments.len(), 2, "the unsent review is left out");
    assert_eq!(s.comments[0].kind, CommentKind::Review);
    assert_eq!(s.comments[0].created_at, "2026-10-11T09:40:00Z");
    let c = &s.comments[1];
    assert_eq!(c.kind, CommentKind::ReviewComment);
    assert_eq!(c.path.as_deref(), Some("src/a.rs"));
    assert_eq!((c.start_line, c.line), (Some(10), Some(12)));
    assert_eq!((c.in_reply_to, c.review_id), (Some(9), Some(1)));
    assert_eq!(s.commits[0].author, "Jason");
    assert_eq!(s.files, ["src/a.rs"]);
    assert_eq!(s.pr.head_sha, "abc1234");
    assert!(!s.pr.merged);
}

#[test]
fn an_edited_comment_keeps_its_earlier_text_up_to_the_limit() {
    let first = stored(&[issue_comment("one", "2026-10-11T09:30:00Z")], None);
    assert!(first.comments[0].revisions.is_empty());
    let same = stored(
        &[issue_comment("one", "2026-10-11T09:30:00Z")],
        Some(&first),
    );
    assert!(same.comments[0].revisions.is_empty());
    let second = stored(
        &[issue_comment("two", "2026-10-11T10:30:00Z")],
        Some(&first),
    );
    assert_eq!(
        second.comments[0].revisions,
        [Revision {
            body: "one".into(),
            updated_at: "2026-10-11T09:30:00Z".into()
        }]
    );
    let mut latest = second;
    for i in 0..15 {
        latest = stored(
            &[issue_comment(&format!("v{i}"), "2026-10-11T11:00:00Z")],
            Some(&latest),
        );
    }
    assert_eq!(latest.comments[0].revisions.len(), REVISIONS);
    assert_eq!(latest.comments[0].revisions.last().unwrap().body, "v13");
}

#[test]
fn saves_loads_and_skips_older_forms_and_unreadable_files() {
    let d = tempfile::tempdir().unwrap();
    let s = stored(&[issue_comment("one", "2026-10-11T09:30:00Z")], None);
    save(d.path(), &s).unwrap();
    let path = stored_path(d.path(), "A/B", 5).unwrap();
    assert!(path.ends_with("ledger/a/b/5.json"));
    let mut old = s.clone();
    old.version = 0;
    old.pr.number = 6;
    save(d.path(), &old).unwrap();
    std::fs::write(path.with_file_name("7.json"), "not json").unwrap();
    let (found, problems) = load_repo(d.path(), "a/b").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].pr.title, "Add login");
    assert_eq!(found[0].comments[0].body, "one");
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("7.json"));
    assert_eq!(load_repo(d.path(), "c/d").unwrap().0.len(), 0);
    let one = |n| load(d.path(), "a/b", n).unwrap().map(|s| s.pr.number);
    assert_eq!(
        [one(5), one(6), one(7), one(8)],
        [Some(5), None, None, None]
    );
}

#[test]
fn a_bad_repository_name_never_becomes_a_path() {
    let d = tempfile::tempdir().unwrap();
    assert!(stored_path(d.path(), "../etc", 1).is_err());
    assert!(stored_path(d.path(), "a/../../b", 1).is_err());
    assert!(load_repo(d.path(), "a").is_err());
}

#[test]
fn prune_removes_disconnected_repositories_only() {
    let d = tempfile::tempdir().unwrap();
    let mut s = stored(&[], None);
    save(d.path(), &s).unwrap();
    s.repo = "a/c".into();
    save(d.path(), &s).unwrap();
    s.repo = "x/y".into();
    save(d.path(), &s).unwrap();
    prune(d.path(), &["A/b".to_string()]).unwrap();
    let ledger = d.path().join("ledger");
    assert!(ledger.join("a/b/5.json").exists());
    assert!(!ledger.join("a/c").exists());
    assert!(!ledger.join("x").exists(), "an empty owner folder goes too");
    prune(tempfile::tempdir().unwrap().path(), &[]).unwrap();
}
