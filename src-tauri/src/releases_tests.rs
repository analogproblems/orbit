use super::*;
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn ago(minutes: i64) -> String {
    stamp(now() - Duration::minutes(minutes))
}
fn read(repo: &str, minutes: i64) -> RepoReleases {
    RepoReleases {
        repo: repo.into(),
        read_at: ago(minutes),
        ..Default::default()
    }
}
fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}
fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
/// A release as GitHub lists it, made `days` ago.
fn listed(id: u64, tag: &str, days: i64, draft: bool, prerelease: bool) -> Value {
    json!({
        "id": id,
        "tag_name": tag,
        "name": format!("App {tag}"),
        "draft": draft,
        "prerelease": prerelease,
        "created_at": stamp(now() - Duration::days(days)),
        "published_at": (!draft).then(|| stamp(now() - Duration::days(days))),
        "html_url": format!("https://github.com/a/b/releases/tag/{tag}"),
        "assets": [{ "id": 1 }, { "id": 2 }],
    })
}
/// The release GitHub marks latest in a list, when nobody chose another:
/// the newest made that is neither a draft nor a pre-release, or none.
fn marked(list: &Value) -> Value {
    let mut out: Vec<&Value> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] != true && r["prerelease"] != true)
        .collect();
    out.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    out.first().map_or(Value::Null, |r| (*r).clone())
}
/// GitHub's answer about a repository's tags, as `(name, commit)`, the
/// first an annotated tag.
fn tags(list: &[(&str, char)], total: u64, permission: &str) -> Value {
    let nodes: Vec<Value> = list
        .iter()
        .enumerate()
        .map(|(i, (name, c))| {
            let target = if i == 0 {
                json!({ "__typename": "Tag", "oid": sha('9'), "target": { "__typename": "Commit", "oid": sha(*c) } })
            } else {
                json!({ "__typename": "Commit", "oid": sha(*c) })
            };
            json!({ "name": name, "target": target })
        })
        .collect();
    json!({ "data": { "repository": {
        "viewerPermission": permission,
        "defaultBranchRef": { "name": "main", "target": { "oid": sha('f') } },
        "refs": { "totalCount": total, "nodes": nodes },
    } } })
}

#[test]
fn version_tags_are_told_from_other_tags_and_ordered_as_versions() {
    let v = |t: &str| version(t);
    assert_eq!(
        v("v1.2.3"),
        Some(Version {
            major: 1,
            minor: 2,
            patch: 3,
            pre: vec![]
        })
    );
    assert_eq!(v("1.2.3"), v("v1.2.3"));
    assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
    assert_eq!(v("2.0.0-beta.2").unwrap().pre, ["beta", "2"]);
    for other in [
        "latest",
        "v1.2",
        "1.2.3.4",
        "app@1.2.3",
        "release-1.2.3",
        "v1.2.x",
        "1.2.3-",
        "1.2.3-beta..1",
        "1.2.3+",
        "1.2.3-beta_1",
        "",
    ] {
        assert_eq!(v(other), None, "{other}");
    }
    // Lowest first, as semantic versioning orders them.
    let order = [
        "0.9.0",
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.2.0",
        "1.10.0",
        "2.0.0",
    ];
    for pair in order.windows(2) {
        assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
    }
    assert_eq!(v("v1.0.0").cmp(&v("1.0.0")), Ordering::Equal);
}

#[test]
fn repositories_are_read_never_read_first_then_the_oldest_every_half_hour() {
    let all = repos(&["a/new", "a/old", "a/fresh", "a/older", "bad name"]);
    let known = [read("a/old", 40), read("A/Fresh", 29), read("a/older", 90)];
    assert_eq!(wanted(&all, &known, now()), ["a/new", "a/older", "a/old"]);
    let many = repos(&["a/1", "a/2", "a/3", "a/4", "a/5"]);
    assert_eq!(wanted(&many, &[], now()).len(), PER_PASS);
    // One that failed waits for its retry, however long ago it was read.
    let failed = RepoReleases {
        error: Some("down".into()),
        retry_at: Some(ago(-5)),
        ..read("a/old", 40)
    };
    assert!(wanted(&repos(&["a/old"]), std::slice::from_ref(&failed), now()).is_empty());
    let due = RepoReleases {
        retry_at: Some(ago(0)),
        ..failed
    };
    assert_eq!(wanted(&repos(&["a/old"]), &[due], now()), ["a/old"]);
}

#[test]
fn the_latest_release_drafts_and_tags_with_no_release_are_worked_out() {
    let list = json!([
        listed(5, "v0.3.0", 1, true, false),
        listed(4, "v0.3.0-beta.1", 3, false, true),
        listed(3, "v0.2.0", 10, false, false),
        listed(2, "v0.2.0-rc.1", 12, false, true),
        listed(1, "v0.1.0", 30, false, false),
    ]);
    let refs = tags(
        &[
            ("v0.2.2", 'e'),
            ("v0.2.1", 'd'),
            ("v0.3.0-beta.1", 'c'),
            ("v0.2.0", 'b'),
            ("nightly", 'a'),
            ("v0.1.5", '8'),
            ("v0.1.0", '7'),
        ],
        7,
        "WRITE",
    );
    let (r, base) = understand("a/b", &list, &marked(&list), &refs, now()).unwrap();
    assert_eq!(r.repo, "a/b");
    assert_eq!(r.read_at, stamp(now()));
    assert_eq!((r.branch.as_str(), r.head.clone()), ("main", sha('f')));
    let latest = r.latest.clone().unwrap();
    assert_eq!(
        latest,
        Release {
            id: 3,
            tag: "v0.2.0".into(),
            name: "App v0.2.0".into(),
            prerelease: false,
            draft: false,
            created_at: stamp(now() - Duration::days(10)),
            published_at: Some(stamp(now() - Duration::days(10))),
            url: "https://github.com/a/b/releases/tag/v0.2.0".into(),
            assets: 2,
        }
    );
    // Only a pre-release newer than the latest release counts.
    assert_eq!(r.prerelease.as_ref().map(|p| p.id), Some(4));
    assert_eq!(r.drafts.iter().map(|d| d.id).collect::<Vec<_>>(), [5]);
    assert_eq!((r.draft_count, r.published), (1, 4));
    // Version tags only, newer than the latest release, the highest first.
    let names: Vec<&str> = r.unreleased.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["v0.2.2", "v0.2.1"]);
    assert_eq!(r.unreleased[0].sha, sha('e'));
    assert_eq!((r.unreleased_count, r.version_tags), (2, 6));
    assert!(!r.more_tags && !r.more_releases && !r.drafts_hidden);
    // The latest release's tag, an annotated one here, through to its commit.
    assert_eq!(base, Some(sha('b')));
    let one = tags(&[("v0.2.0", 'b')], 1, "ADMIN");
    let (_, base) = understand("a/b", &list, &marked(&list), &one, now()).unwrap();
    assert_eq!(base, Some(sha('b')));
}

#[test]
fn a_repository_with_no_releases_lists_every_version_tag() {
    let refs = tags(
        &[("v1.0.0", 'a'), ("v1.4.0", 'b'), ("v1.2.0", 'c')],
        300,
        "READ",
    );
    let (r, base) = understand("a/b", &json!([]), &Value::Null, &refs, now()).unwrap();
    assert_eq!(r.latest, None);
    assert_eq!(base, None);
    let names: Vec<&str> = r.unreleased.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["v1.4.0", "v1.2.0", "v1.0.0"]);
    // GitHub has more tags than Orbit read, and hides drafts from a
    // sign-in that can't push.
    assert!(r.more_tags && r.drafts_hidden);
    assert_eq!(
        sentence(&r),
        " It has no releases on GitHub; its newest version tag is v1.4.0."
    );
    let none = understand(
        "a/b",
        &json!([]),
        &Value::Null,
        &tags(&[], 0, "ADMIN"),
        now(),
    )
    .unwrap()
    .0;
    assert_eq!(sentence(&none), " It has no releases or version tags yet.");
}

#[test]
fn drafts_and_tags_are_kept_to_a_few_and_counted() {
    let list: Vec<Value> = (1..=12)
        .map(|n| listed(n, &format!("v1.{n}.0"), 20 - n as i64, true, false))
        .collect();
    let names: Vec<String> = (0..15).map(|n| format!("v2.{n}.0")).collect();
    let refs: Vec<(&str, char)> = names.iter().map(|n| (n.as_str(), 'a')).collect();
    let refs = tags(&refs, 15, "ADMIN");
    let (r, _) = understand("a/b", &json!(list), &Value::Null, &refs, now()).unwrap();
    assert_eq!((r.drafts.len(), r.draft_count), (SHOWN, 12));
    // The newest draft first.
    assert_eq!(r.drafts[0].id, 12);
    assert_eq!((r.unreleased.len(), r.unreleased_count), (SHOWN, 15));
    assert_eq!(r.unreleased[0].name, "v2.14.0");
    assert_eq!(r.latest, None);
    // A full page of releases may not be all of them.
    let full: Vec<Value> = (1..=PAGE as u64)
        .map(|n| listed(n, &format!("v0.0.{n}"), 1, false, false))
        .collect();
    let full = json!(full);
    let (r, _) = understand("a/b", &full, &marked(&full), &tags(&[], 0, "ADMIN"), now()).unwrap();
    assert!(r.more_releases);
}

#[test]
fn githubs_odd_answers_are_errors_or_left_out() {
    let refs = tags(&[], 0, "ADMIN");
    assert_eq!(
        understand(
            "a/b",
            &json!({ "message": "x" }),
            &Value::Null,
            &refs,
            now()
        )
        .unwrap_err(),
        "GitHub didn't return the releases."
    );
    assert_eq!(
        understand(
            "a/b",
            &json!([]),
            &Value::Null,
            &json!({ "errors": [{ "message": "Something went wrong" }] }),
            now()
        )
        .unwrap_err(),
        "Something went wrong"
    );
    assert_eq!(
        understand(
            "a/b",
            &json!([]),
            &Value::Null,
            &json!({ "data": { "repository": null } }),
            now()
        )
        .unwrap_err(),
        "GitHub didn't return the repository."
    );
    // A release with no tag or id, a link away from GitHub, a tag of a tag,
    // a tag with a bad commit, and a repository with no commits yet.
    let list = json!([
        { "id": 1, "tag_name": "", "created_at": ago(1) },
        { "tag_name": "v1.0.0" },
        { "id": 3, "tag_name": "v1.0.0", "name": "  ", "html_url": "https://evil.example/x",
          "created_at": ago(1), "draft": false, "prerelease": false },
    ]);
    let odd = json!({ "data": { "repository": {
        "viewerPermission": null,
        "defaultBranchRef": null,
        "refs": { "totalCount": 3, "nodes": [
            { "name": "v2.0.0", "target": { "__typename": "Tag", "oid": sha('1'),
              "target": { "__typename": "Tag", "oid": sha('3') } } },
            { "name": "v3.0.0", "target": { "__typename": "Commit", "oid": "abc" } },
            { "name": "v4.0.0", "target": { "__typename": "Tree", "oid": sha('2') } },
        ] },
    } } });
    let (r, base) = understand("a/b", &list, &list[2], &odd, now()).unwrap();
    let latest = r.latest.unwrap();
    assert_eq!(
        (latest.id, latest.name.as_str(), latest.url.as_str()),
        (3, "v1.0.0", "")
    );
    assert_eq!(r.published, 1);
    assert!(r.unreleased.is_empty() && r.version_tags == 0 && !r.more_tags);
    assert_eq!((r.branch.as_str(), r.head.as_str()), ("", ""));
    // With no default branch commit, there is nothing to count since.
    assert_eq!(base, None);
    assert!(r.drafts_hidden);
}

#[test]
fn the_latest_release_is_the_one_github_marks_and_a_full_page_says_nothing_of_tags() {
    // 100 newer pre-releases push the latest release off the page Orbit
    // reads, and one of them names a tag on it.
    let full: Vec<Value> = (1..=PAGE as u64)
        .map(|n| listed(n + 1, &format!("v2.0.0-beta.{n}"), 1, false, true))
        .collect();
    let latest = listed(1, "v1.0.0", 90, false, false);
    let refs = tags(
        &[("v2.0.0-beta.100", 'c'), ("v1.0.1", 'b'), ("v1.0.0", 'a')],
        3,
        "ADMIN",
    );
    let (r, base) = understand("a/b", &json!(full), &latest, &refs, now()).unwrap();
    assert_eq!(r.latest.as_ref().map(|l| l.id), Some(1));
    assert_eq!(base, Some(sha('a')));
    assert_eq!(r.prerelease.as_ref().map(|p| p.id), Some(2));
    // v1.0.1's release may be among those Orbit didn't read.
    assert!(r.more_releases);
    assert!(r.unreleased.is_empty() && r.unreleased_count == 0);
    assert_eq!(r.version_tags, 3);
    assert_eq!(
        sentence(&r),
        " Its latest release is v1.0.0. A newer pre-release, v2.0.0-beta.1, is out."
    );
    // Someone kept a newer maintenance release from being marked latest.
    let list = json!([
        listed(2, "v1.4.1", 1, false, false),
        listed(1, "v2.0.0", 5, false, false),
    ]);
    let refs = tags(&[("v1.4.1", 'b'), ("v2.0.0", 'a')], 2, "ADMIN");
    let (r, base) = understand("a/b", &list, &list[1], &refs, now()).unwrap();
    assert_eq!(r.latest.map(|l| l.tag), Some("v2.0.0".into()));
    assert_eq!(base, Some(sha('a')));
    assert!(r.unreleased.is_empty() && !r.more_releases);
    // Nothing GitHub marks latest is a draft or a pre-release.
    let beta = listed(3, "v3.0.0-beta.1", 1, false, true);
    let (r, _) = understand("a/b", &json!([beta.clone()]), &beta, &refs, now()).unwrap();
    assert!(r.latest.is_none());
}

#[test]
fn pre_releases_and_drafts_alone_are_said_so() {
    // Only a pre-release is out, and a newer tag has no release.
    let list = json!([listed(1, "v1.0.0-beta.1", 3, false, true)]);
    let refs = tags(
        &[
            ("v1.0.0-beta.2", 'b'),
            ("v1.0.0-beta.1", 'a'),
            ("v0.9.0", '9'),
        ],
        3,
        "ADMIN",
    );
    let (r, base) = understand("a/b", &list, &Value::Null, &refs, now()).unwrap();
    assert_eq!((r.latest.is_none(), r.published), (true, 1));
    assert_eq!(r.prerelease.as_ref().map(|p| p.id), Some(1));
    assert_eq!(base, None);
    // Only tags newer than the newest pre-release count.
    let names: Vec<&str> = r.unreleased.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["v1.0.0-beta.2"]);
    assert_eq!(
        sentence(&r),
        " It has no full release yet, only pre-releases. Its newest pre-release is v1.0.0-beta.1. \
         Its tag v1.0.0-beta.2 has no release."
    );
    // A draft for a tag already pushed: nothing is out, and the tag has
    // its draft.
    let list = json!([listed(1, "v1.0.0", 1, true, false)]);
    let refs = tags(&[("v1.0.0", 'a')], 1, "ADMIN");
    let (r, _) = understand("a/b", &list, &Value::Null, &refs, now()).unwrap();
    assert_eq!((r.draft_count, r.published, r.version_tags), (1, 0, 1));
    assert!(r.unreleased.is_empty());
    assert_eq!(
        sentence(&r),
        " It has nothing published yet. A draft, v1.0.0, waits to be published."
    );
}

#[test]
fn bit_sums_up_a_repositorys_releases() {
    let release = |tag: &str| Release {
        tag: tag.into(),
        ..Default::default()
    };
    let mut r = RepoReleases {
        read_at: ago(1),
        branch: "main".into(),
        latest: Some(release("v0.2.0")),
        since: Some(5),
        ..Default::default()
    };
    assert_eq!(
        sentence(&r),
        " Its latest release is v0.2.0, and main has had 5 commits since."
    );
    r.since = Some(1);
    assert!(sentence(&r).ends_with("main has had 1 commit since."));
    r.since = Some(0);
    assert!(sentence(&r).ends_with("main has had no commits since."));
    r.since = None;
    r.prerelease = Some(release("v0.3.0-beta.1"));
    r.drafts = vec![release("v0.3.0")];
    r.draft_count = 1;
    r.unreleased = vec![Tag {
        name: "v0.2.1".into(),
        sha: sha('a'),
    }];
    r.unreleased_count = 1;
    assert_eq!(
        sentence(&r),
        " Its latest release is v0.2.0. A newer pre-release, v0.3.0-beta.1, is out. \
         A draft, v0.3.0, waits to be published. Its tag v0.2.1 has no release."
    );
    r.draft_count = 3;
    r.unreleased_count = 2;
    assert!(sentence(&r)
        .ends_with(" 3 drafts wait to be published. 2 newer version tags have no release."));
    // Nothing until Orbit has read them, or when it can't see them.
    assert_eq!(sentence(&RepoReleases::default()), "");
    r.unseen = Some(unseen("a/b"));
    assert_eq!(sentence(&r), "");
}

#[test]
fn a_failed_read_keeps_what_was_read_and_waits_longer_each_time() {
    let mut all = vec![];
    let good = RepoReleases {
        latest: Some(Release {
            tag: "v1.0.0".into(),
            ..Default::default()
        }),
        ..read("a/b", 0)
    };
    record(&mut all, "a/b", Ok(good.clone()), now());
    assert_eq!(all, std::slice::from_ref(&good));
    record(
        &mut all,
        "A/B",
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].latest, good.latest);
    assert_eq!(all[0].tries, 1);
    assert!(all[0]
        .error
        .as_deref()
        .unwrap()
        .starts_with("Orbit couldn't read the releases yet: "));
    assert_eq!(all[0].retry_at, Some(stamp(now() + Duration::minutes(1))));
    record(&mut all, "a/b", Err("down".into()), now());
    assert_eq!(all[0].retry_at, Some(stamp(now() + Duration::minutes(2))));
    for _ in 0..10 {
        record(&mut all, "a/b", Err("down".into()), now());
    }
    assert_eq!(all[0].retry_at, Some(stamp(now() + logs::MAX_RETRY)));
    // A read that works starts again.
    record(&mut all, "a/b", Ok(good.clone()), now());
    assert_eq!(all, [good]);
    // One never read before keeps just the error.
    record(&mut all, "a/c", Err("down".into()), now());
    assert_eq!(all[1].repo, "a/c");
    assert!(all[1].read_at.is_empty() && all[1].error.is_some());
    prune(&mut all, &repos(&["A/C"]));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].repo, "a/c");
}

fn tag(name: &str, c: char) -> Tag {
    Tag {
        name: name.into(),
        sha: sha(c),
    }
}
fn checked(name: &str, c: char, says: &[&str]) -> TagCheck {
    TagCheck {
        name: name.into(),
        sha: sha(c),
        at: ago(0),
        says: says.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}
fn names(tags: &[Tag]) -> Vec<&str> {
    tags.iter().map(|t| t.name.as_str()).collect()
}

#[test]
fn version_tags_are_kept_highest_first_for_the_next_read() {
    let list = json!([]);
    let refs = tags(
        &[
            ("v1.2.0", 'a'),
            ("latest", 'a'),
            ("v1.10.0", 'b'),
            ("1.2.0", 'c'),
            ("v1.3.0-beta.1", 'd'),
        ],
        5,
        "WRITE",
    );
    let (r, _) = understand("a/b", &list, &marked(&list), &refs, now()).unwrap();
    assert_eq!(
        names(&r.tags),
        ["v1.10.0", "v1.3.0-beta.1", "1.2.0", "v1.2.0"]
    );
}

/// The newest version tag is due, then each that is new or moved, and
/// each GitHub didn't answer for; with no list from before, only the
/// newest.
#[test]
fn the_newest_and_new_version_tags_are_checked_once() {
    let mut now = read("a/b", 0);
    now.tags = vec![tag("v0.3.0", 'c'), tag("v0.2.1", 'b'), tag("v0.2.0", 'a')];
    // Never read: the newest alone.
    assert_eq!(names(&due(None, &now)), ["v0.3.0"]);
    // Read before Orbit kept a list, with version tags then.
    let mut old = read("a/b", 30);
    old.version_tags = 2;
    assert_eq!(names(&due(Some(&old), &now)), ["v0.3.0"]);
    // With no version tags then, each is new.
    old.version_tags = 0;
    assert_eq!(
        names(&due(Some(&old), &now)),
        ["v0.3.0", "v0.2.1", "v0.2.0"]
    );
    // A read GitHub refused, or one that failed, isn't a list.
    old.unseen = Some(unseen("a/b"));
    assert_eq!(names(&due(Some(&old), &now)), ["v0.3.0"]);
    old.unseen = None;
    old.read_at.clear();
    assert_eq!(names(&due(Some(&old), &now)), ["v0.3.0"]);

    // From a list: the new one and the newest, unless checked there.
    let mut before = read("a/b", 30);
    before.tags = vec![tag("v0.2.1", 'b'), tag("v0.2.0", 'a')];
    before.checks = vec![checked("v0.2.1", 'b', &["0.2.1"])];
    carry(Some(&before), &mut now);
    assert_eq!(names(&due(Some(&before), &now)), ["v0.3.0"]);
    let mut after = now.clone();
    add(&mut after, checked("v0.3.0", 'c', &["0.3.0"]));
    assert_eq!(
        after
            .checks
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["v0.3.0", "v0.2.1"]
    );
    let mut next = after.clone();
    carry(Some(&after), &mut next);
    assert!(due(Some(&after), &next).is_empty());
    // A tag moved to another commit is new again, and its old check goes.
    next.tags[1] = tag("v0.2.1", 'e');
    carry(Some(&after), &mut next);
    let kept: Vec<&str> = next.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(kept, ["v0.3.0"]);
    assert_eq!(names(&due(Some(&after), &next)), ["v0.2.1"]);
    // One GitHub didn't answer for is asked again.
    let mut failed = checked("v0.2.1", 'e', &[]);
    failed.error = Some("gh: Bad Gateway (HTTP 502)".into());
    add(&mut next, failed);
    let mut again = next.clone();
    carry(Some(&next), &mut again);
    assert_eq!(names(&due(Some(&next), &again)), ["v0.2.1"]);

    // A few a read, the highest first, and a few kept.
    let mut many = read("a/b", 0);
    many.tags = (0..30)
        .rev()
        .map(|i| tag(&format!("v1.{i}.0"), 'f'))
        .collect();
    let mut none = read("a/b", 30);
    none.version_tags = 0;
    // Every one is due, the highest first; `check_due` takes a few a read.
    let first = due(Some(&none), &many);
    assert_eq!(first.len(), 30);
    assert_eq!(first[0].name, "v1.29.0");
    for i in 0..30 {
        add(&mut many, checked(&format!("v1.{i}.0"), 'f', &[]));
    }
    assert_eq!(many.checks.len(), CHECKS_KEPT);
    assert_eq!(many.checks[0].name, "v1.29.0");
    assert_eq!(many.checks[CHECKS_KEPT - 1].name, "v1.10.0");
}

/// Tags due beyond the few a read checks, and those a rate limit stopped
/// it before, are left for the next read and checked then, each once.
#[tokio::test]
async fn tags_left_over_are_checked_at_the_next_read() {
    let asked = std::sync::Mutex::new(Vec::<String>::new());
    let limit = std::sync::atomic::AtomicBool::new(false);
    let check = |t: Tag| {
        asked.lock().unwrap().push(t.name.clone());
        let mut c = TagCheck {
            says: vec![t.name[1..].to_string()],
            name: t.name,
            sha: t.sha,
            at: ago(0),
            ..Default::default()
        };
        if limit.swap(false, std::sync::atomic::Ordering::SeqCst) {
            c.error = Some("gh: API rate limit exceeded for user ID 1. (HTTP 403)".into());
        }
        async move { c }
    };
    let taken = || std::mem::take(&mut *asked.lock().unwrap());
    // With no version tags before, all 7 are new: 5 now, 2 left.
    let mut before = read("a/b", 30);
    before.version_tags = 0;
    let mut now = read("a/b", 0);
    now.tags = (0..7)
        .rev()
        .map(|i| tag(&format!("v1.{i}.0"), 'f'))
        .collect();
    assert!(!check_due(Some(&before), &mut now, check).await);
    assert_eq!(taken(), ["v1.6.0", "v1.5.0", "v1.4.0", "v1.3.0", "v1.2.0"]);
    assert_eq!(names(&now.pending), ["v1.1.0", "v1.0.0"]);
    // The next read checks those, though they aren't new any more.
    let next = |before: &RepoReleases| {
        let mut now = read("a/b", 0);
        now.tags = before.tags.clone();
        carry(Some(before), &mut now);
        now
    };
    let mut second = next(&now);
    assert!(!check_due(Some(&now), &mut second, check).await);
    assert_eq!(taken(), ["v1.1.0", "v1.0.0"]);
    assert!(second.pending.is_empty());
    let mut third = next(&second);
    assert!(!check_due(Some(&second), &mut third, check).await);
    assert!(taken().is_empty());

    // Two new tags, and a rate limit on the first: the second waits too.
    let mut fourth = next(&third);
    fourth
        .tags
        .splice(0..0, [tag("v2.1.0", 'a'), tag("v2.0.0", 'b')]);
    limit.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(check_due(Some(&third), &mut fourth, check).await);
    assert_eq!(taken(), ["v2.1.0"]);
    assert_eq!(names(&fourth.pending), ["v2.0.0"]);
    let mut fifth = next(&fourth);
    assert!(!check_due(Some(&fourth), &mut fifth, check).await);
    assert_eq!(taken(), ["v2.1.0", "v2.0.0"]);
    assert!(fifth.pending.is_empty());
    assert!(fifth
        .checks
        .iter()
        .filter(|c| c.name.starts_with("v2."))
        .all(|c| c.right() == Some(true)));
}

#[test]
fn a_tag_is_on_the_right_commit_when_a_manifest_there_says_its_version() {
    assert_eq!(checked("v0.3.0", 'a', &["0.3.0"]).right(), Some(true));
    assert_eq!(
        checked("0.3.0", 'a', &["0.2.0", "0.3.0"]).right(),
        Some(true)
    );
    assert_eq!(
        checked("v1.0.0-beta.1", 'a', &["1.0.0-beta.1"]).right(),
        Some(true)
    );
    // Build data isn't part of the version.
    assert_eq!(
        checked("v1.0.0+build.5", 'a', &["1.0.0"]).right(),
        Some(true)
    );
    // A Python project's own file, as PEP 440 compares them.
    let python = |name: &str, says: &[&str]| TagCheck {
        python: says.iter().map(|s| s.to_string()).collect(),
        ..checked(name, 'a', says)
    };
    assert_eq!(python("v1.0.0", &["1.0"]).right(), Some(true));
    assert_eq!(python("v1.0.0-alpha.1", &["1.0a1"]).right(), Some(true));
    assert_eq!(python("v1.0.0-alpha.1", &["1.0.0-a1"]).right(), Some(true));
    assert_eq!(python("v1.0.0", &["1.1"]).right(), Some(false));
    // Anywhere else they are different versions.
    assert_eq!(
        checked("v1.0.0-alpha.1", 'a', &["1.0.0-a1"]).right(),
        Some(false)
    );
    assert_eq!(checked("v1.0.0", 'a', &["1.0"]).right(), Some(false));
    // Cargo.toml says one, pyproject.toml the other.
    let both = TagCheck {
        python: vec!["1.0a1".into()],
        ..checked("v1.0.0-alpha.1", 'a', &["1.0.0-a1", "1.0a1"])
    };
    assert_eq!(both.right(), Some(true));
    assert_eq!(checked("v0.3.0", 'a', &["0.2.0"]).right(), Some(false));
    assert_eq!(
        checked("v0.3.0", 'a', &["0.3.0-beta.1"]).right(),
        Some(false)
    );
    // Orbit can't say: no file says a version, one it couldn't read may,
    // or GitHub didn't answer.
    assert_eq!(checked("v0.3.0", 'a', &[]).right(), None);
    let mut unread = checked("v0.3.0", 'a', &["0.2.0"]);
    unread.unread = vec!["Cargo.toml".into()];
    assert_eq!(unread.right(), None);
    let mut failed = checked("v0.3.0", 'a', &["0.3.0"]);
    failed.error = Some("down".into());
    assert_eq!(failed.right(), None);
}
