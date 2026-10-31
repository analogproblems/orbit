use super::*;
use serde_json::json;

const ME: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "fedcba9876543210fedcba9876543210";

fn me() -> Instance {
    Instance {
        name: ME.into(),
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
    assert_eq!(signer(body), None);
    // Only a whole name counts, so text quoting part of one doesn't.
    assert_eq!(signer("<!-- orbit-from 0123 -->"), None);
    assert_eq!(
        signer("<!-- orbit-from XYZ3456789abcdef0123456789abcdef -->"),
        None
    );
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

    // Its own post, then another Orbit's earlier one.
    p.read(
        &[
            comment(Some(ME), "2026-10-31T09:00:00Z", "u1"),
            comment(Some(OTHER), "2026-10-31T08:00:00Z", "u2"),
        ],
        &me(),
    );
    assert_eq!(p.mine, "2026-10-31T09:00:00Z");
    assert!(!p.stepped_back());

    // The other Orbit posts later: Orbit steps back.
    p.read(&[comment(Some(OTHER), "2026-10-31T09:30:00Z", "u3")], &me());
    assert!(p.stepped_back());
    assert_eq!(
        p.other,
        Some(Post {
            url: "u3".into(),
            at: "2026-10-31T09:30:00Z".into()
        })
    );
    // An older post read afterwards doesn't take its place.
    p.read(&[comment(Some(OTHER), "2026-10-31T07:00:00Z", "u4")], &me());
    assert_eq!(p.other.as_ref().unwrap().url, "u3");

    // You say to post here again: it does, until the other posts again.
    assert!(p.resume());
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
    assert_eq!(p.mine, "2026-10-31T09:00:00Z");
    assert_eq!(p.other.as_ref().unwrap().url, "r2");
    assert!(!p.stepped_back());
    // Its own older post, read after, doesn't take the newer one's place.
    p.read(&[comment(Some(ME), "2026-10-31T08:00:00Z", "u")], &me());
    assert_eq!(p.mine, "2026-10-31T09:00:00Z");
    let mut q = Posting::default();
    q.read(&[comment(Some(OTHER), "", "u")], &me());
    assert_eq!(q, Posting::default());
}

#[test]
fn a_copy_of_the_data_folder_is_another_orbit_that_keeps_its_posts() {
    let mut me = Instance::default();
    assert!(me.settle("here"));
    let first = me.name.clone();
    assert_eq!(first.len(), 32);
    // The same folder on the same computer: the same Orbit.
    assert!(!me.settle("here"));
    assert_eq!(me.name, first);
    // Copied, or moved: a new name, and the old one is still its own.
    let mut copy = me.clone();
    assert!(copy.settle("there"));
    assert_ne!(copy.name, first);
    assert_eq!(copy.former, std::slice::from_ref(&first));
    assert!(copy.is(&first) && copy.is(&copy.name) && !me.is(&copy.name));
    // The copy carries on where it left off; the first steps back once the
    // copy posts.
    let mut p = Posting::default();
    p.read(
        &[comment(Some(&first), "2026-10-31T09:00:00Z", "u1")],
        &copy,
    );
    assert!(!p.stepped_back());
    let mut q = Posting::default();
    q.read(
        &[
            comment(Some(&first), "2026-10-31T09:00:00Z", "u1"),
            comment(Some(&copy.name), "2026-10-31T09:05:00Z", "u2"),
        ],
        &me,
    );
    assert!(q.stepped_back());
    // Only so many old names are kept, the newest.
    for n in 0..FORMER + 5 {
        copy.settle(&format!("place {n}"));
    }
    assert_eq!(copy.former.len(), FORMER);
    assert!(!copy.is(&first));
    assert!(copy.is(copy.former.last().unwrap()));
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
