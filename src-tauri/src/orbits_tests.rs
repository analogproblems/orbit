use super::*;
use serde_json::json;

const ME: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "fedcba9876543210fedcba9876543210";

fn me() -> Instance {
    named(ME)
}

fn named(name: &str) -> Instance {
    Instance {
        name: name.into(),
        ..Instance::default()
    }
}

fn comment(by: Option<&str>, at: &str, url: &str) -> Value {
    let body = match by {
        Some(name) => signed("## Orbit · CI\n\nEvery check passes.", name),
        None => "## Orbit · CI\n\nEvery check passes.".to_string(),
    };
    json!({ "body": body, "updated_at": at, "html_url": url })
}

fn post(by: &str, at: &str, url: &str) -> Post {
    Post {
        url: url.into(),
        at: at.into(),
        by: by.into(),
    }
}

fn at(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(time)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn signs_a_post_where_it_can_be_read_back() {
    let name = new_name();
    assert_eq!(name.len(), 32);
    assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(name, new_name());
    let body = "<!-- orbit-review:r1 -->\n## Orbit · Codex review\n\nNo findings.";
    let post = signed(body, &name);
    // The other markers stay where they were, at the start.
    assert!(post.starts_with(body));
    assert!(post.ends_with(&format!("\n\n<!-- orbit-from {name} -->")));
    assert_eq!(signer(&post), Some(name.as_str()));
    assert_eq!(signer(&format!("{post}\n\n  \n")), Some(name.as_str()));
    assert_eq!(signer(body), None);
    assert_eq!(signer(""), None);
    // Only a whole name counts, so text quoting part of one doesn't.
    assert_eq!(signer("<!-- orbit-from 0123 -->"), None);
    assert_eq!(
        signer("<!-- orbit-from XYZ3456789abcdef0123456789abcdef -->"),
        None
    );
}

#[test]
fn only_a_signature_on_the_last_line_counts() {
    let sig = signature(OTHER);
    // Quoted in a reply, or in a code block, above the reply's own text.
    for body in [
        format!("> {sig}\n\nThanks, fixed."),
        format!("Orbit added this:\n\n```\n{sig}\n```"),
        format!("{sig}\n\nAnd something after it."),
        format!("Quoted: {sig}"),
        format!("> {sig}"),
        format!("`{sig}`"),
    ] {
        assert_eq!(signer(&body), None, "{body}");
    }
    // A quoted signature above, its own below: only its own counts.
    let reply = signed(&format!("> {sig}\n\nEvery check passes."), ME);
    assert_eq!(signer(&reply), Some(ME));
    let mut p = Posting::default();
    p.read(
        &[json!({
            "body": format!("Copied from the other Orbit:\n\n{sig}\n\nok"),
            "updated_at": "2026-10-31T10:00:00Z",
        })],
        &me(),
    );
    assert_eq!(p, Posting::default());
}

#[test]
fn only_trusted_accounts_signatures_count() {
    let by = |login: &str, association: &str| json!({ "user": { "login": login }, "author_association": association });
    let trusted_authors = ["Helper-Bot".to_string()];
    // Orbit's own sign-in, whatever GitHub says about it.
    assert!(trusted(&by("Me", "NONE"), "me", &trusted_authors));
    // The repository's owners, members and collaborators.
    for association in ["OWNER", "MEMBER", "COLLABORATOR"] {
        assert!(trusted(&by("x", association), "me", &[]));
    }
    // Accounts you trust.
    assert!(trusted(&by("helper-bot", "NONE"), "me", &trusted_authors));
    // Anyone else could copy a signature.
    for association in ["NONE", "CONTRIBUTOR", "FIRST_TIME_CONTRIBUTOR", ""] {
        assert!(!trusted(&by("x", association), "me", &trusted_authors));
    }
    assert!(!trusted(&json!({}), "me", &trusted_authors));
    // With no sign-in or name, an empty name matches nothing.
    assert!(!trusted(&by("", "NONE"), "", &[String::new()]));
}

#[test]
fn steps_back_only_for_another_orbits_later_post() {
    let mut p = Posting::default();
    // Nothing signed: posts from before Orbits signed theirs, or by people.
    p.read(
        &[
            comment(None, "2026-10-30T10:00:00Z", "u0"),
            json!({ "body": "Fixed F2 in abc123", "updated_at": "2026-10-30T11:00:00Z" }),
        ],
        &me(),
    );
    assert_eq!(p, Posting::default());
    assert!(!p.stepped_back());
    assert_eq!(p.since(), "");

    // Its own post, then another Orbit's earlier one.
    p.read(
        &[
            comment(Some(ME), "2026-10-31T09:00:00Z", "u1"),
            comment(Some(OTHER), "2026-10-31T08:00:00Z", "u2"),
        ],
        &me(),
    );
    assert_eq!(p.mine, Some(post(ME, "2026-10-31T09:00:00Z", "u1")));
    assert_eq!(p.since(), "2026-10-31T09:00:00Z");
    assert!(!p.stepped_back());

    // The other Orbit posts later: Orbit steps back.
    p.read(&[comment(Some(OTHER), "2026-10-31T09:30:00Z", "u3")], &me());
    assert!(p.stepped_back());
    assert_eq!(p.other, Some(post(OTHER, "2026-10-31T09:30:00Z", "u3")));
    // An older post read afterwards doesn't take its place.
    p.read(&[comment(Some(OTHER), "2026-10-31T07:00:00Z", "u4")], &me());
    assert_eq!(p.other.as_ref().unwrap().url, "u3");

    // You say to post here again: it does, until the other posts again.
    assert!(p.resume());
    assert_eq!(p.resumed, p.other);
    assert!(!p.stepped_back());
    assert!(!p.resume());
    p.read(&[comment(Some(OTHER), "2026-10-31T09:30:00Z", "u3")], &me());
    assert!(!p.stepped_back());
    p.read(&[comment(Some(OTHER), "2026-10-31T10:00:00Z", "u5")], &me());
    assert!(p.stepped_back());

    // A post of its own after that, such as one you published, is newer.
    p.read(&[comment(Some(ME), "2026-10-31T10:05:00Z", "u6")], &me());
    assert!(!p.stepped_back());
}

#[test]
fn an_orbit_that_never_posted_here_steps_back_for_one_that_did() {
    let mut p = Posting::default();
    p.read(&[comment(Some(OTHER), "2026-10-01T00:00:00Z", "u1")], &me());
    assert!(p.stepped_back());
    assert!(stepped_back(
        &[Posting {
            pr_id: "a/b#1".into(),
            ..p.clone()
        }],
        "a/b#1"
    ));
    assert!(!stepped_back(
        &[Posting {
            pr_id: "a/b#1".into(),
            ..p
        }],
        "a/b#2"
    ));
}

#[test]
fn reads_reviews_by_when_they_were_submitted_and_times_in_any_zone() {
    let mut p = Posting::default();
    p.read(
        &[
            json!({
                "body": signed("<!-- orbit-review:r1 -->", ME),
                "submitted_at": "2026-10-31T09:00:00Z",
                "html_url": "r1",
            }),
            // An hour before, written with an offset.
            json!({
                "body": signed("<!-- orbit-review:r2 -->", OTHER),
                "submitted_at": "2026-10-31T09:30:00+01:00",
                "html_url": "r2",
            }),
            // No time: it can't be placed, so it's left out.
            json!({ "body": signed("x", OTHER), "html_url": "r3" }),
            json!({ "body": signed("x", OTHER), "submitted_at": "", "html_url": "r4" }),
        ],
        &me(),
    );
    assert_eq!(p.mine, Some(post(ME, "2026-10-31T09:00:00Z", "r1")));
    assert_eq!(p.other.as_ref().unwrap().url, "r2");
    assert!(!p.stepped_back());
    // Its own older post, read after, doesn't take the newer one's place.
    p.read(&[comment(Some(ME), "2026-10-31T08:00:00Z", "u")], &me());
    assert_eq!(p.since(), "2026-10-31T09:00:00Z");
    let mut q = Posting::default();
    q.read(&[comment(Some(OTHER), "", "u")], &me());
    assert_eq!(q, Posting::default());
}

/// Two Orbits that post in the same second, which GitHub's times can't
/// tell apart, agree on which came last, whichever order they read them
/// in, so exactly one of them carries on.
#[test]
fn posts_in_the_same_second_leave_one_orbit_posting() {
    let (a, b) = (named(ME), named(OTHER));
    let same = "2026-10-31T09:00:00Z";
    let both = [
        comment(Some(ME), same, "ua"),
        comment(Some(OTHER), same, "ub"),
    ];
    let reversed = [both[1].clone(), both[0].clone()];
    for posts in [&both, &reversed] {
        let (mut pa, mut pb) = (Posting::default(), Posting::default());
        pa.read(posts, &a);
        pb.read(posts, &b);
        assert_ne!(pa.stepped_back(), pb.stepped_back());
        // The name later in order is the one that carries on.
        assert!(pa.stepped_back());
        // And the same written in another zone.
        let mut pc = Posting::default();
        pc.read(
            &[comment(Some(OTHER), "2026-10-31T10:00:00+01:00", "ub")],
            &a,
        );
        pc.read(posts, &a);
        assert!(pc.stepped_back());
    }

    // After you hand a pull request back to the one that stepped back, the
    // two posting in the same second again still leave exactly one.
    let (mut pa, mut pb) = (Posting::default(), Posting::default());
    pa.read(&both, &a);
    pb.read(&both, &b);
    assert!(pa.resume());
    assert!(!pa.stepped_back());
    let later = "2026-10-31T09:05:00Z";
    let again = [
        comment(Some(ME), later, "ua2"),
        comment(Some(OTHER), later, "ub2"),
    ];
    pa.read(&again, &a);
    pb.read(&[again[1].clone(), again[0].clone()], &b);
    assert_ne!(pa.stepped_back(), pb.stepped_back());
}

#[test]
fn a_copy_of_the_data_folder_is_another_orbit_that_keeps_its_posts() {
    let copied = at("2026-10-31T09:10:00Z");
    let mut me = Instance::default();
    assert!(me.settle("here", copied));
    let first = me.name.clone();
    assert_eq!(first.len(), 32);
    assert!(me.former.is_empty());
    // The same folder on the same computer: the same Orbit.
    assert!(!me.settle("here", copied));
    assert_eq!(me.name, first);
    // Copied, or moved: a new name, and the old one is still its own for
    // what it posted until then.
    let mut copy = me.clone();
    assert!(copy.settle("there", copied));
    assert_ne!(copy.name, first);
    assert_eq!(copy.home, "there");
    assert_eq!(
        copy.former,
        [Former {
            name: first.clone(),
            until: "2026-10-31T09:10:00Z".into()
        }]
    );
    assert!(copy.is(&copy.name, "2026-11-01T00:00:00Z"));
    assert!(copy.is(&first, "2026-10-31T09:00:00Z"));
    assert!(copy.is(&first, "2026-10-31T09:10:00Z"));
    assert!(copy.is(&first, "2026-10-31T10:10:00+01:00"));
    assert!(!copy.is(&first, "2026-10-31T09:10:01Z"));
    assert!(!me.is(&copy.name, "2026-10-31T09:00:00Z"));
    // The copy carries on where it left off; the first steps back once the
    // copy posts.
    let mut p = Posting::default();
    p.read(
        &[comment(Some(&first), "2026-10-31T09:00:00Z", "u1")],
        &copy,
    );
    assert!(!p.stepped_back());
    assert_eq!(p.mine.as_ref().unwrap().url, "u1");
    let mut q = Posting::default();
    q.read(
        &[
            comment(Some(&first), "2026-10-31T09:00:00Z", "u1"),
            comment(Some(&copy.name), "2026-10-31T09:15:00Z", "u2"),
        ],
        &me,
    );
    assert!(q.stepped_back());
    // Only so many old names are kept, the newest.
    for n in 0..FORMER + 5 {
        copy.settle(&format!("place {n}"), copied);
    }
    assert_eq!(copy.former.len(), FORMER);
    assert!(!copy.is(&first, "2026-10-31T09:00:00Z"));
    let last = copy.former.last().unwrap();
    assert!(copy.is(&last.name, &last.until));
}

/// You can hand a pull request back from a copy to the Orbit it was copied
/// from: what the original posts after the copy was made is another
/// Orbit's to the copy, so the copy steps back for it.
#[test]
fn a_copy_hands_back_to_the_orbit_it_was_copied_from() {
    let mut original = Instance::default();
    original.settle("here", at("2026-10-31T08:00:00Z"));
    let mut copy = original.clone();
    copy.settle("there", at("2026-10-31T09:10:00Z"));
    let name = original.name.clone();
    let before = comment(Some(&name), "2026-10-31T09:00:00Z", "u1");
    let by_copy = comment(Some(&copy.name), "2026-10-31T09:20:00Z", "u2");

    // The copy posts: the original steps back.
    let mut on_original = Posting::default();
    on_original.read(&[before.clone(), by_copy.clone()], &original);
    assert!(on_original.stepped_back());
    let mut on_copy = Posting::default();
    on_copy.read(&[before.clone(), by_copy.clone()], &copy);
    assert!(!on_copy.stepped_back());

    // You tap Post here again on the original, and it posts.
    assert!(on_original.resume());
    let by_original = comment(Some(&name), "2026-10-31T09:30:00Z", "u3");
    on_original.read(std::slice::from_ref(&by_original), &original);
    assert!(!on_original.stepped_back());
    // The copy reads that as the original's post, not its own, and steps
    // back, while the post from before the copy stays its own.
    on_copy.read(&[before, by_copy, by_original], &copy);
    assert!(on_copy.stepped_back());
    assert_eq!(on_copy.mine.as_ref().unwrap().url, "u2");
    assert_eq!(
        on_copy.other,
        Some(post(&name, "2026-10-31T09:30:00Z", "u3"))
    );
}

#[test]
fn where_orbit_runs_is_its_computer_and_data_folder() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let here = home(a.path());
    assert_eq!(here.len(), 64);
    assert_eq!(here, home(a.path()));
    assert_ne!(here, home(b.path()));
    // The same folder by another way there is the same place.
    #[cfg(unix)]
    {
        let link = b.path().join("link");
        std::os::unix::fs::symlink(a.path(), &link).unwrap();
        assert_eq!(home(&link), here);
    }
    assert!(!host_name().is_empty());
}
