use super::{
    claims::{ci_lines, collect, lines, place_commits, Line},
    rounds::{Comment, Comments},
    test_support::*,
    worktrees::git,
};
use crate::ops::{AnswerKind, Said};
use crate::types::*;

fn line(numbers: &[u32], kind: ClaimKind, commit: Option<&str>, reason: Option<&str>) -> Line {
    Line {
        numbers: numbers.to_vec(),
        kind,
        commit: commit.map(str::to_owned),
        reason: reason.map(str::to_owned),
    }
}

#[test]
fn reads_fixed_and_declined_replies() {
    use ClaimKind::*;
    let cases = [
        (
            "Fixed F2 in abc1234",
            line(&[2], Fixed, Some("abc1234"), None),
        ),
        (
            "**Fixed F2, F3 and f4** in `ABC1234`.",
            line(&[2, 3, 4], Fixed, Some("abc1234"), None),
        ),
        (
            "Fixed F2 in commit abc1234 by indexing from 0",
            line(&[2], Fixed, Some("abc1234"), None),
        ),
        (
            "Fixed in abc1234def",
            line(&[], Fixed, Some("abc1234def"), None),
        ),
        ("Fixed.", line(&[], Fixed, None, None)),
        ("Fixed F2", line(&[2], Fixed, None, None)),
        (
            "- Declined F5: intended behaviour",
            line(&[5], Declined, None, Some("intended behaviour")),
        ),
        (
            "> Declined: not worth it",
            line(&[], Declined, None, Some("not worth it")),
        ),
        ("Declined F1 & F2.", line(&[1, 2], Declined, None, None)),
    ];
    for (text, expected) in cases {
        assert_eq!(lines(text), [expected], "{text}");
    }
    // Sentences that only mention fixing or declining are not claims.
    for text in [
        "Fixed the tests",
        "Declined because it's fine",
        "I fixed F2 in abc1234",
        "Fixedly yours",
        "Declined F2 since it is intended",
        "Fixed in xyz",
    ] {
        assert_eq!(lines(text), [], "{text}");
    }
    assert_eq!(
        lines("Thanks!\nFixed F1 in abc1234\nDeclined F2: intended"),
        [
            line(&[1], Fixed, Some("abc1234"), None),
            line(&[2], Declined, None, Some("intended"))
        ]
    );
}
fn said(numbers: &[u32], kind: AnswerKind, commit: Option<&str>, text: Option<&str>) -> Said {
    Said {
        numbers: numbers.to_vec(),
        kind,
        commit: commit.map(str::to_owned),
        text: text.map(str::to_owned),
    }
}

#[test]
fn reads_answers_about_ci_items() {
    use AnswerKind::*;
    let cases = [
        ("Taking I2", said(&[2], Taking, None, None)),
        (
            "taking i1 and I3, back soon",
            said(&[1, 3], Taking, None, None),
        ),
        (
            "- **Fixed I2** in `ABC1234`.",
            said(&[2], Fixed, Some("abc1234"), None),
        ),
        (
            "Fixed I1, I2 in commit abc1234def by pinning the runner",
            said(&[1, 2], Fixed, Some("abc1234def"), None),
        ),
        ("Fixed I2", said(&[2], Fixed, None, None)),
        (
            "> Declined I3: the runner is broken",
            said(&[3], Declined, None, Some("the runner is broken")),
        ),
        ("Declined I3.", said(&[3], Declined, None, None)),
        (
            "Need a person on I4: who can renew the key?",
            said(&[4], Question, None, Some("who can renew the key?")),
        ),
        ("Need a person for I4", said(&[4], Question, None, None)),
    ];
    for (text, expected) in cases {
        assert_eq!(ci_lines(text), [expected], "{text}");
    }
    // Review replies and sentences that only mention items are left alone,
    // and CI answers aren't read as review replies.
    for text in [
        "Fixed F2 in abc1234",
        "Declined F2: intended",
        "Taking a look",
        "I'm taking I2",
        "Fixed in abc1234",
        "Declined I3 since it is flaky",
        "Need a person",
        "Fixed issue 3",
    ] {
        assert_eq!(ci_lines(text), [], "{text}");
    }
    for text in ["Fixed I2 in abc1234", "Declined I3: flaky", "Taking I2"] {
        assert_eq!(lines(text), [], "{text}");
    }
}

/// The reply skill for coding agents only teaches lines Orbit reads.
#[test]
fn the_reply_skill_teaches_lines_orbit_reads() {
    use ClaimKind::*;
    let skill = include_str!("../../../.claude/skills/answer-orbit-reviews/SKILL.md");
    let examples = skill.split("```text\n").nth(1).unwrap();
    let examples = examples.split("```").next().unwrap();
    assert_eq!(
        lines(examples),
        [
            line(&[2], Fixed, Some("3f9c2a1"), None),
            line(&[1, 3], Fixed, Some("3f9c2a1"), None),
            line(
                &[4],
                Declined,
                None,
                Some("the caller in src/api.rs rejects an empty list, so this path can't be reached.")
            ),
        ]
    );
    let ci = skill.split("```text\n").nth(2).unwrap();
    let ci = ci.split("```").next().unwrap();
    assert_eq!(
        ci_lines(ci),
        [
            said(&[2], AnswerKind::Taking, None, None),
            said(&[1], AnswerKind::Fixed, Some("3f9c2a1"), None),
            said(
                &[3],
                AnswerKind::Declined,
                None,
                Some("the runner image is out of disk space, which this branch can't change.")
            ),
            said(
                &[4],
                AnswerKind::Question,
                None,
                Some("the deploy key expired, and only an owner can renew it.")
            ),
        ]
    );
    assert_eq!(lines(ci), [], "CI answers aren't review replies");
    // Each form it names, such as `Fixed F2 in <commit>`, filled in.
    let code = regex::Regex::new(r"`([^`\n]+)`").unwrap();
    let forms: Vec<&str> = code
        .captures_iter(skill)
        .map(|c| c.get(1).unwrap().as_str())
        .filter(|s| {
            [
                "Fixed ",
                "Declined ",
                "Declined:",
                "Taking ",
                "Need a person ",
            ]
            .iter()
            .any(|p| s.starts_with(p))
        })
        .collect();
    let item = regex::Regex::new(r"\bI\d").unwrap();
    let (ci_forms, forms): (Vec<&str>, Vec<&str>) =
        forms.into_iter().partition(|f| item.is_match(f));
    assert_eq!(forms.len(), 5, "{forms:?}");
    assert_eq!(ci_forms.len(), 6, "{ci_forms:?}");
    for form in ci_forms {
        let text = form
            .replace("<commit>", "3f9c2a1")
            .replace("<reason>", "flaky")
            .replace("<question>", "who?");
        let read = ci_lines(&text);
        assert_eq!(read.len(), 1, "{form}");
        let expected = match read[0].kind {
            AnswerKind::Fixed if form.contains("<commit>") => (Some("3f9c2a1"), None),
            AnswerKind::Fixed | AnswerKind::Taking => (None, None),
            AnswerKind::Declined => (None, Some("flaky")),
            AnswerKind::Question => (None, Some("who?")),
        };
        assert_eq!(
            (read[0].commit.as_deref(), read[0].text.as_deref()),
            expected,
            "{form}"
        );
    }
    for form in forms {
        let text = form
            .replace("<commit>", "3f9c2a1")
            .replace("<reason>", "intended");
        let read = lines(&text);
        assert_eq!(read.len(), 1, "{form}");
        let expected = match read[0].kind {
            Fixed => (Some("3f9c2a1"), None),
            Declined => (None, Some("intended")),
        };
        assert_eq!(
            (read[0].commit.as_deref(), read[0].reason.as_deref()),
            expected,
            "{form}"
        );
    }
}

fn round() -> PreviousRound {
    let finding = |id: &str, number| Finding {
        id: id.into(),
        number: Some(number),
        ..Default::default()
    };
    PreviousRound {
        review_id: "r1".into(),
        head_sha: "c".repeat(40),
        verdict: None,
        findings: vec![finding("r1-0", 1), finding("r1-1", 2)],
        decisions: vec![],
        claims: vec![],
        comments_included: 0,
        comments_omitted: 0,
    }
}
fn comment(author: &str, at: &str, body: &str, finding: Option<&str>) -> Comment {
    Comment {
        author: author.into(),
        created_at: at.into(),
        body: body.into(),
        finding: finding.map(str::to_owned),
        location: None,
    }
}

#[test]
fn keeps_the_latest_claim_about_each_finding_in_play() {
    let comments = Comments {
        kept: vec![
            comment("alex", "1", "Fixed F1 in abc1234 and F2 in def5678", None),
            comment("bob", "2", "Declined: intended", Some("r1-1")),
            // A finding not in play, a thread no longer carried, and a
            // sentence that isn't a claim are left out.
            comment("alex", "3", "Fixed F9 in abc1234", None),
            comment("alex", "4", "Fixed in abc1234", Some("r0-3")),
            comment("alex", "5", "Fixed the flaky test", Some("r1-0")),
        ],
        ..Default::default()
    };
    let claims = collect(&comments, &round());
    assert_eq!(
        claims,
        [
            AuthorClaim {
                finding_id: "r1-0".into(),
                author: "alex".into(),
                created_at: "1".into(),
                kind: ClaimKind::Fixed,
                commit: Some("abc1234".into()),
                commit_check: None,
                reason: None,
            },
            AuthorClaim {
                finding_id: "r1-1".into(),
                author: "bob".into(),
                created_at: "2".into(),
                kind: ClaimKind::Declined,
                commit: None,
                commit_check: None,
                reason: Some("intended".into()),
            },
        ]
    );
    assert_eq!(claims[0].summary(), "fixed in `abc1234`");
    assert_eq!(claims[1].summary(), "declined: intended");
    // A later claim replaces an earlier one, here or in the thread.
    let mut later = comments;
    later
        .kept
        .push(comment("alex", "6", "**Fixed F2** in `0123abcd`.", None));
    let claims = collect(&later, &round());
    assert_eq!(claims[1].kind, ClaimKind::Fixed);
    assert_eq!(claims[1].commit.as_deref(), Some("0123abcd"));
}

#[tokio::test]
async fn says_where_a_named_commit_is() {
    let (_d, repo) = git_repo().await;
    let previous = commit(&repo, "Reviewed").await;
    let fix = commit(&repo, "Fix").await;
    git(&repo, &["checkout", "-q", "-b", "elsewhere", &previous])
        .await
        .unwrap();
    let elsewhere = commit(&repo, "Elsewhere").await;
    git(&repo, &["checkout", "-q", "-"]).await.unwrap();
    let claim = |sha: &str| AuthorClaim {
        finding_id: "r1-0".into(),
        author: "alex".into(),
        created_at: String::new(),
        kind: ClaimKind::Fixed,
        commit: Some(sha.into()),
        commit_check: None,
        reason: None,
    };
    let mut claims = [
        claim(&fix[..7]),
        claim(&previous),
        claim(&elsewhere[..10]),
        claim("deadbee"),
        AuthorClaim {
            commit: None,
            ..claim("")
        },
    ];
    place_commits(&mut claims, &repo, &previous, &fix).await;
    let places: Vec<_> = claims.iter().map(|c| c.commit_check).collect();
    assert_eq!(
        places,
        [
            Some(CommitPlace::InChange),
            Some(CommitPlace::Earlier),
            Some(CommitPlace::NotInBranch),
            Some(CommitPlace::NotFound),
            None,
        ]
    );
    assert_eq!(
        claims[2].commit_note(),
        Some("the commit is not part of this pull request's head")
    );
}
