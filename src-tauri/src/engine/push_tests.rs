use super::Engine;
use crate::{
    push::{
        tests::{fake_service, FakeService},
        Added, MAX_DEVICES, NOT_A_PUSH_SERVICE,
    },
    types::*,
};
use axum::http::StatusCode;
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

fn engine(root: &Path) -> Arc<Engine> {
    let e = Engine::new(root.into(), |_| {}).unwrap();
    e.wake_locally();
    e
}
fn repository() -> Repository {
    Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
        dev_container: Default::default(),
    }
}
fn review(status: &str) -> Review {
    let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), true);
    r.status = status.into();
    r
}
/// A failed automatic review, which Orbit sends a notification for.
async fn fail_a_review(e: &Engine) {
    fail_a_review_of(e, 1).await;
}
/// The same for pull request `number`.
async fn fail_a_review_of(e: &Engine, number: u64) {
    let mut pr = crate::types::tests::pr();
    pr.id = format!("a/b#{number}");
    pr.number = number;
    let mut failed = Review::new(&pr, &Settings::default(), true);
    failed.status = "failed".into();
    e.update(|s| {
        s.settings.repositories = vec![repository()];
        s.prs.push(pr);
        s.reviews.push(failed);
    })
    .await;
    e.flush().await.unwrap();
}
/// Lets the device `id` be woken now, as if its wait were over.
fn wait_over(e: &Engine, id: &str) {
    let mut retry = e.wake_retry.lock().unwrap();
    let wait = retry[id].1;
    retry.insert(id.into(), (Instant::now(), wait));
}
async fn add(e: &Engine, service: &FakeService, device: u32) -> Added {
    e.add_push_device(&format!("{}/wake/{device}", service.url), "iPhone")
        .await
        .unwrap()
}
/// Runs one pass of waking devices and waits for it to end.
async fn wake(e: &Arc<Engine>) {
    e.wake_devices().await;
    let start = Instant::now();
    while e.waking.load(Ordering::SeqCst) {
        assert!(start.elapsed() < Duration::from_secs(10), "still waking");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
fn taken(service: &FakeService) -> usize {
    service.taken.lock().unwrap().len()
}

#[tokio::test]
async fn wakes_a_device_once_for_each_alert_and_tells_it_once() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    // Nothing yet, so nothing to wake it for.
    wake(&e).await;
    assert_eq!(taken(&service), 0);
    fail_a_review(&e).await;
    wake(&e).await;
    assert_eq!(taken(&service), 1);
    assert_eq!(service.taken.lock().unwrap()[0].body, 0);
    wake(&e).await;
    assert_eq!(taken(&service), 1, "woken once");
    let notices = e.push_notices(&added.key).await.unwrap();
    assert_eq!(
        notices.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
        ["An automatic review stopped"]
    );
    assert!(e.push_notices(&added.key).await.unwrap().is_empty());
    // Another key reads nothing.
    assert!(e.push_notices(&"0".repeat(64)).await.is_none());
}

#[tokio::test]
async fn a_burst_of_alerts_wakes_a_device_once_till_it_reads_them() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    fail_a_review_of(&e, 1).await;
    wake(&e).await;
    fail_a_review_of(&e, 2).await;
    wake(&e).await;
    // The wake-up it has coming reads both.
    assert_eq!(taken(&service), 1);
    assert_eq!(e.push_notices(&added.key).await.unwrap().len(), 2);
    // Read, the next one wakes it straight away.
    fail_a_review_of(&e, 3).await;
    wake(&e).await;
    assert_eq!(taken(&service), 2);
    // Not read a while after it was woken: woken again for what's newer.
    fail_a_review_of(&e, 4).await;
    wake(&e).await;
    assert_eq!(taken(&service), 2);
    wait_over(&e, &added.id);
    wake(&e).await;
    assert_eq!(taken(&service), 3);
    assert_eq!(e.push_notices(&added.key).await.unwrap().len(), 2);
}

#[tokio::test]
async fn keeps_where_each_device_is_woken_to_itself() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    let saved = e.snapshot().await.push_devices[0].clone();
    assert!(saved.endpoint.ends_with("/wake/1"));
    assert_eq!(saved.key_hash, crate::push::hash(&added.key));
    let shown = e.shown_snapshot().await.push_devices[0].clone();
    assert_eq!(
        (shown.id.as_str(), shown.name.as_str()),
        (added.id.as_str(), "iPhone")
    );
    assert!(shown.endpoint.is_empty() && shown.key_hash.is_empty());
    // A device's key is never kept, only its hash.
    let state = std::fs::read_to_string(d.path().join("state.json")).unwrap();
    assert!(state.contains(&added.id) && !state.contains(&added.key));
}

#[tokio::test]
async fn refuses_what_isnt_a_push_service() {
    let d = tempfile::tempdir().unwrap();
    let e = Engine::new(d.path().into(), |_| {}).unwrap();
    assert_eq!(
        e.add_push_device("http://127.0.0.1:9/wake/1", "x").await,
        Err(NOT_A_PUSH_SERVICE.into())
    );
    assert_eq!(
        e.add_push_device("https://example.com/wake", "x").await,
        Err(NOT_A_PUSH_SERVICE.into())
    );
    assert!(e.snapshot().await.push_devices.is_empty());
}

#[tokio::test]
async fn a_device_hears_only_of_alerts_sent_after_it_was_added() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    fail_a_review(&e).await;
    let added = add(&e, &service, 1).await;
    wake(&e).await;
    assert_eq!(taken(&service), 0);
    assert!(e.push_notices(&added.key).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_alert_dealt_with_before_the_device_is_woken_wakes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    fail_a_review(&e).await;
    // Reviewed again before the next pass: nothing still needs you.
    e.update(|s| s.reviews.push(review("reviewing"))).await;
    wake(&e).await;
    assert_eq!(taken(&service), 0);
    let device = e.snapshot().await.push_devices[0].clone();
    assert_eq!(device.told, e.snapshot().await.notices_sent);
    assert!(e.push_notices(&added.key).await.unwrap().is_empty());
}

#[tokio::test]
async fn forgets_a_device_its_push_service_no_longer_knows() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    *service.status.lock().unwrap() = StatusCode::GONE;
    fail_a_review(&e).await;
    wake(&e).await;
    assert_eq!(taken(&service), 1);
    assert!(e.snapshot().await.push_devices.is_empty());
    assert!(e.push_notices(&added.key).await.is_none());
}

#[tokio::test]
async fn shows_a_failed_wake_up_and_tries_again_later() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    *service.status.lock().unwrap() = StatusCode::INTERNAL_SERVER_ERROR;
    fail_a_review(&e).await;
    wake(&e).await;
    assert_eq!(taken(&service), 1);
    assert_eq!(
        e.shown_snapshot().await.push_devices[0].error.as_deref(),
        Some("Orbit couldn't wake it: Its push service answered 500.")
    );
    // Not again straight away.
    wake(&e).await;
    assert_eq!(taken(&service), 1);
    let first = e.wake_retry.lock().unwrap()[&added.id].1;
    assert_eq!(first, Duration::from_secs(30));
    // Once its wait is over, it's tried again, and waits longer if that
    // fails too.
    wait_over(&e, &added.id);
    wake(&e).await;
    assert_eq!(taken(&service), 2);
    assert_eq!(e.wake_retry.lock().unwrap()[&added.id].1, first * 2);
    *service.status.lock().unwrap() = StatusCode::CREATED;
    wait_over(&e, &added.id);
    wake(&e).await;
    assert_eq!(taken(&service), 3);
    assert_eq!(e.shown_snapshot().await.push_devices[0].error, None);
    assert_eq!(e.wake_retry.lock().unwrap()[&added.id].1, Duration::ZERO);
    wake(&e).await;
    assert_eq!(taken(&service), 3);
    // Failing again after one went, it waits the first wait again.
    *service.status.lock().unwrap() = StatusCode::INTERNAL_SERVER_ERROR;
    fail_a_review_of(&e, 2).await;
    wait_over(&e, &added.id);
    wake(&e).await;
    assert_eq!(taken(&service), 4);
    assert_eq!(e.wake_retry.lock().unwrap()[&added.id].1, first);
    // Reading doesn't cut a failing device's wait short.
    e.push_notices(&added.key).await.unwrap();
    assert_eq!(e.wake_retry.lock().unwrap()[&added.id].1, first);
}

#[tokio::test]
async fn a_new_access_link_forgets_every_device() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    crate::remote::new_token(d.path()).unwrap();
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    assert!(e.push_notices(&added.key).await.is_some());
    crate::remote::new_token(d.path()).unwrap();
    // Before Orbit forgets it, it reads nothing and isn't woken.
    assert!(e.push_notices(&added.key).await.is_none());
    fail_a_review(&e).await;
    wake(&e).await;
    assert_eq!(taken(&service), 0);
    assert!(e.snapshot().await.push_devices.is_empty());
    // And a new link forgets every device at once.
    let again = add(&e, &service, 2).await;
    crate::remote::new_token(d.path()).unwrap();
    e.forget_old_push_devices().await;
    assert!(e.snapshot().await.push_devices.is_empty());
    assert!(e.push_notices(&again.key).await.is_none());
}

#[tokio::test]
async fn the_same_browser_replaces_itself_and_the_oldest_goes_past_ten() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let old = add(&e, &service, 1).await;
    let new = add(&e, &service, 1).await;
    let devices = e.snapshot().await.push_devices;
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, new.id);
    assert!(e.push_notices(&old.key).await.is_none());
    for device in 2..=MAX_DEVICES as u32 + 1 {
        add(&e, &service, device).await;
    }
    let devices = e.snapshot().await.push_devices;
    assert_eq!(devices.len(), MAX_DEVICES);
    assert!(devices.iter().all(|d| d.id != new.id), "the oldest went");
}

#[tokio::test]
async fn removes_a_device() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    e.remove_push_device(&added.id).await.unwrap();
    e.remove_push_device(&added.id).await.unwrap();
    assert!(e.snapshot().await.push_devices.is_empty());
    fail_a_review(&e).await;
    wake(&e).await;
    assert_eq!(taken(&service), 0);
}

#[tokio::test]
async fn keeps_its_devices_across_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let e = engine(d.path());
    let service = fake_service().await;
    let added = add(&e, &service, 1).await;
    let public = e.push_key().await.unwrap();
    drop(e);
    let e = engine(d.path());
    assert_eq!(e.push_key().await.unwrap(), public);
    fail_a_review(&e).await;
    wake(&e).await;
    assert_eq!(taken(&service), 1);
    assert_eq!(e.push_notices(&added.key).await.unwrap().len(), 1);
}
