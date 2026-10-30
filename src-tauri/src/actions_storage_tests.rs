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
fn read(repo: &str, minutes: i64) -> Storage {
    Storage {
        repo: repo.into(),
        read_at: ago(minutes),
        ..Default::default()
    }
}
fn repos(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn repositories_are_read_never_read_first_then_the_oldest_every_half_hour() {
    let all = repos(&["a/new", "a/old", "a/fresh", "a/older", "bad name"]);
    let known = [read("a/old", 40), read("A/Fresh", 29), read("a/older", 90)];
    assert_eq!(wanted(&all, &known, now()), ["a/new", "a/older", "a/old"]);
    let many = repos(&["a/1", "a/2", "a/3", "a/4", "a/5"]);
    assert_eq!(wanted(&many, &[], now()).len(), PER_PASS);
    // One that failed waits for its retry, however long ago it was read.
    let failed = Storage {
        error: Some("down".into()),
        retry_at: Some(ago(-5)),
        ..read("a/old", 40)
    };
    assert!(wanted(&repos(&["a/old"]), std::slice::from_ref(&failed), now()).is_empty());
    let due = Storage {
        retry_at: Some(ago(0)),
        ..failed
    };
    assert_eq!(wanted(&repos(&["a/old"]), &[due], now()), ["a/old"]);
}

#[test]
fn caches_and_kept_artifacts_are_read_from_githubs_answers() {
    let usage =
        json!({ "active_caches_size_in_bytes": 9_800_000_000u64, "active_caches_count": 42 });
    assert_eq!(caches(&usage), Ok((42, 9_800_000_000)));
    assert_eq!(
        caches(&json!({ "active_caches_count": 1 })),
        Err("GitHub didn't say how much the caches hold.".into())
    );
    assert_eq!(
        caches(&json!({ "active_caches_size_in_bytes": 5 })),
        Ok((0, 5))
    );
    let artifacts = [
        json!({ "id": 1, "size_in_bytes": 300, "expired": false }),
        json!({ "id": 2, "size_in_bytes": 5000, "expired": true }),
        json!({ "id": 3, "size_in_bytes": 200 }),
        json!({ "id": 4 }),
    ];
    assert_eq!(kept(&artifacts), (3, 500), "expired ones don't count");
}

#[test]
fn storage_orbit_may_not_see_is_said_so_and_other_errors_are_tried_again() {
    let mut s = read("a/b", 0);
    let refused = Err("gh: Resource not accessible by integration (HTTP 403)".to_string());
    assert_eq!(seen(&mut s, refused), Ok(None));
    assert_eq!(
        s.unseen.as_deref(),
        Some("Orbit can't see the Actions storage of a/b: GitHub doesn't show it to Orbit's sign-in.")
    );
    let mut s = read("a/b", 0);
    assert_eq!(
        seen(&mut s, Err("gh: Not Found (HTTP 404)".into())),
        Ok(None)
    );
    assert!(s.unseen.is_some());
    let mut s = read("a/b", 0);
    let limited = "gh: API rate limit exceeded for user ID 1. (HTTP 403)".to_string();
    assert_eq!(seen(&mut s, Err(limited.clone())), Err(limited));
    assert_eq!(seen(&mut s, Err("HTTP 502".into())), Err("HTTP 502".into()));
    assert_eq!(seen(&mut s, Ok(json!({}))), Ok(Some(json!({}))));
    assert!(s.unseen.is_none());
}

#[test]
fn a_failed_read_keeps_what_was_read_and_waits_longer_each_time() {
    let mut all = vec![];
    record(&mut all, "a/b", Err("HTTP 502".into()), now());
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].repo, "a/b");
    assert!(all[0].read_at.is_empty(), "never read");
    assert_eq!(
        all[0].error.as_deref(),
        Some("Orbit couldn't read the storage yet: HTTP 502. It tries again later.")
    );
    let good = Storage {
        caches: 3,
        cache_bytes: 1000,
        ..read("a/b", 0)
    };
    record(&mut all, "A/B", Ok(good.clone()), now());
    assert_eq!(all, [good]);
    let mut waits = vec![];
    for _ in 0..8 {
        record(&mut all, "a/b", Err("HTTP 502".into()), now());
        let retry = time(all[0].retry_at.as_deref().unwrap()).unwrap();
        waits.push((retry - now()).num_minutes());
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    assert_eq!(all[0].caches, 3, "what was read is kept");
    assert_eq!(all.len(), 1);
}

#[test]
fn storage_goes_with_its_repository() {
    let mut all = vec![read("a/b", 0), read("a/c", 0)];
    prune(&mut all, &repos(&["A/B"]));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].repo, "a/b");
}

#[test]
fn bit_mentions_caches_only_near_the_usual_limit() {
    let near = Storage {
        cache_bytes: NEAR,
        ..read("a/b", 0)
    };
    assert_eq!(
        sentence(&near),
        " Its caches hold 9.0 GB, near GitHub's usual 10 GB limit, so GitHub may soon drop the ones used least."
    );
    let below = Storage {
        cache_bytes: NEAR - 1,
        ..near.clone()
    };
    assert_eq!(sentence(&below), "");
    let hidden = Storage {
        unseen: Some(unseen("a/b")),
        ..near
    };
    assert_eq!(sentence(&hidden), "");
    assert_eq!(gb(GB + GB / 2), "1.5 GB");
}
