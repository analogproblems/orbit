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
    let (r, base) = understand("a/b", &list, &refs, now()).unwrap();
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
    let (_, base) = understand("a/b", &list, &tags(&[("v0.2.0", 'b')], 1, "ADMIN"), now()).unwrap();
    assert_eq!(base, Some(sha('b')));
}

#[test]
fn a_repository_with_no_releases_lists_every_version_tag() {
    let refs = tags(
        &[("v1.0.0", 'a'), ("v1.4.0", 'b'), ("v1.2.0", 'c')],
        300,
        "READ",
    );
    let (r, base) = understand("a/b", &json!([]), &refs, now()).unwrap();
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
    let none = understand("a/b", &json!([]), &tags(&[], 0, "ADMIN"), now())
        .unwrap()
        .0;
    assert_eq!(sentence(&none), " It has no releases on GitHub.");
}

#[test]
fn drafts_and_tags_are_kept_to_a_few_and_counted() {
    let list: Vec<Value> = (1..=12)
        .map(|n| listed(n, &format!("v1.{n}.0"), 20 - n as i64, true, false))
        .collect();
    let names: Vec<String> = (0..15).map(|n| format!("v2.{n}.0")).collect();
    let refs: Vec<(&str, char)> = names.iter().map(|n| (n.as_str(), 'a')).collect();
    let (r, _) = understand("a/b", &json!(list), &tags(&refs, 15, "ADMIN"), now()).unwrap();
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
    let (r, _) = understand("a/b", &json!(full), &tags(&[], 0, "ADMIN"), now()).unwrap();
    assert!(r.more_releases);
}

#[test]
fn githubs_odd_answers_are_errors_or_left_out() {
    let refs = tags(&[], 0, "ADMIN");
    assert_eq!(
        understand("a/b", &json!({ "message": "x" }), &refs, now()).unwrap_err(),
        "GitHub didn't return the releases."
    );
    assert_eq!(
        understand(
            "a/b",
            &json!([]),
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
    let (r, base) = understand("a/b", &list, &odd, now()).unwrap();
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
