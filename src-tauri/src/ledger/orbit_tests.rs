use super::orbit::*;
use super::*;

fn review(id: u64, body: &str) -> CommentRef {
    CommentRef {
        id,
        kind: CommentKind::Review,
        author: "jason".into(),
        role: Role::Reviewer,
        created_at: "2026-10-11T10:00:00Z".into(),
        url: "u".into(),
        body: body.into(),
        path: None,
        line: None,
        start_line: None,
        in_reply_to: None,
        review_id: None,
        finding_marker: None,
        edit: None,
    }
}

#[test]
fn reads_a_finding_heading_in_full() {
    let f = heading("**F2 · P1 · Not blocking · Test gap · Likely, not verified** Missing test")
        .unwrap();
    assert_eq!(f.short_id.as_deref(), Some("F2"));
    assert_eq!(f.number, Some(2));
    assert_eq!(f.priority, 1);
    assert_eq!(f.blocking, Some(false));
    assert_eq!(f.category, Some(Category::TestGap));
    assert_eq!(f.confirmed, Some(false));
    assert_eq!(f.title, "Missing test");
    let bare = heading("**P3** Tidy").unwrap();
    assert_eq!(
        (bare.short_id, bare.blocking, bare.priority),
        (None, None, 3)
    );
    assert!(heading("**Note:** nothing").is_none());
    assert!(heading("**Fé · P1** odd").unwrap().short_id.is_none());
}

#[test]
fn reads_each_status_label_and_its_stored_key() {
    assert_eq!(status("fixed"), Some(PriorStatus::Fixed));
    assert_eq!(status("still open"), Some(PriorStatus::StillOpen));
    assert_eq!(status("still_open"), Some(PriorStatus::StillOpen));
    assert_eq!(
        status("declined (accepted)"),
        Some(PriorStatus::DeclinedAccepted)
    );
    assert_eq!(
        status("declined_disputed"),
        Some(PriorStatus::DeclinedDisputed)
    );
    assert_eq!(status("not reported"), None);
}

#[test]
fn reads_the_markdown_when_the_hidden_data_was_left_out() {
    let body = "<!-- orbit-review:rev2 --><!-- orbit-verdict: changes_requested round=2 blocking=1 -->\n\
## Orbit · Codex review\n\n\
Reviewed head `1111111` against base `2222222`.\n\n\
Verdict: changes requested (1 blocking finding)\n\n\
One defect remains.\n\n\
### Since the last round (head `0000000`)\n\n\
| Status | Earlier finding | Author said | Note |\n| --- | --- | --- | --- |\n\
| fixed | F1 · P2 Guard the \\| list | fixed in `abc1234` | Guarded. |\n\
| declined (disputed) | F2 · P1 Leaks token | declined: test only | Still reachable. |\n\n\
### Other findings\n\n\
**F3 · P1 · Blocking · Security** Token in log · `src/a.rs:4-9` · not inline: lines not in the diff\n\n\
The token is logged.\n\n\
**Breaks when:** a request fails.\n\
**Also at:** `src/b.rs:2`\n\n\
**F4 · P3 · Not blocking** Rename · outside the repository · not inline: outside the repository\n\n\
Clearer name.\n\n\
_To answer a finding, reply `Fixed F3 in <commit>` or `Declined F3: <reason>`, here or in its thread. The next round reads both._\n\n\
Not checked: the Rust tests; the phone layout\n\n\
---\nCodex";
    let r = parse_review(&review(9, body)).unwrap();
    assert_eq!(r.review_id.as_deref(), Some("rev2"));
    assert_eq!(r.round, Some(2));
    assert_eq!(r.reviewer.as_deref(), Some("Codex"));
    assert_eq!(
        (r.head_sha.as_deref(), r.base_sha.as_deref()),
        (Some("1111111"), Some("2222222"))
    );
    assert_eq!(r.verdict, RoundVerdict::ChangesRequested);
    assert_eq!(r.blocking, Some(1));
    assert_eq!(r.summary.as_deref(), Some("One defect remains."));
    let earlier: Vec<(&str, &str, Option<PriorStatus>)> = r
        .earlier_findings
        .iter()
        .map(|e| (e.id.as_str(), e.title.as_str(), e.status))
        .collect();
    assert_eq!(
        earlier,
        [
            ("F1", "Guard the | list", Some(PriorStatus::Fixed)),
            ("F2", "Leaks token", Some(PriorStatus::DeclinedDisputed)),
        ]
    );
    let said = r.earlier_findings[0].author_said.as_ref().unwrap();
    assert_eq!(said.commit.as_deref(), Some("abc1234"));
    assert_eq!(
        r.earlier_findings[1].author_said.as_ref().unwrap().kind,
        ClaimKind::Declined
    );
    assert_eq!(r.findings.len(), 2);
    let f3 = &r.findings[0];
    assert_eq!(f3.title, "Token in log");
    assert_eq!(f3.blocking, Some(true));
    assert_eq!(f3.problem, "The token is logged.");
    assert_eq!(f3.failure_scenario.as_deref(), Some("a request fails."));
    let places: Vec<(Option<&str>, u32, u32)> = f3
        .locations
        .iter()
        .map(|p| (p.path.as_deref(), p.start_line, p.end_line))
        .collect();
    assert_eq!(places, [(Some("src/a.rs"), 4, 9), (Some("src/b.rs"), 2, 2)]);
    assert_eq!(r.findings[1].title, "Rename");
    assert!(r.findings[1].locations.is_empty());
    assert_eq!(r.not_checked, ["the Rust tests", "the phone layout"]);
}

#[test]
fn a_summary_is_never_a_section() {
    let body = "<!-- orbit-review:r -->\n## Orbit · Claude review\n\nReviewed head `1111111` against base `2222222`.\n\nVerdict: clean\n\n### Since the last round (head `0000000`)\n\n| a | b |";
    let r = parse_review(&review(1, body)).unwrap();
    assert_eq!(r.summary, None);
    assert_eq!(r.verdict, RoundVerdict::Clean);
    assert_eq!(r.blocking, Some(0));
}

#[test]
fn an_inline_comment_is_a_finding_at_its_lines() {
    let mut c = review(
        3,
        "**F1 · P2 · Blocking · Bug** Off by one\n\nThe loop skips the last item.\n\n**Done when:** the last item is read.\n\n<!-- orbit-finding:rev1-a -->",
    );
    c.kind = CommentKind::ReviewComment;
    c.path = Some("src/a.rs".into());
    c.start_line = Some(4);
    c.line = Some(6);
    let f = inline_finding(&c).unwrap();
    assert_eq!(f.id, "rev1-a");
    assert_eq!(f.problem, "The loop skips the last item.");
    assert_eq!(f.done_when.as_deref(), Some("the last item is read."));
    assert_eq!(f.locations[0].path.as_deref(), Some("src/a.rs"));
    assert_eq!((f.locations[0].start_line, f.locations[0].end_line), (4, 6));
    assert_eq!(f.inline, Some(true));
}
