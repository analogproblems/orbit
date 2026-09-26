use super::Engine;
use crate::{
    models::{self, tests::catalog},
    rpc::Rpc,
    types::*,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

async fn fixture() -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>, String) {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(dir.path().into(), |_| {}).unwrap();
    let (tx, rx) = mpsc::channel(64);
    let mut cmd = tokio::process::Command::new("node");
    cmd.args(["-e", include_str!("../model_fixture.cjs")]);
    let rpc = Rpc::from_command(cmd, tx).await.unwrap();
    *e.rpc.lock().await = Some(rpc.clone());
    tokio::spawn(e.clone().pump(rpc.clone(), rx));
    let p = crate::types::tests::pr();
    let settings = Settings {
        model: "model-a".into(),
        effort: "high".into(),
        ..Settings::default()
    };
    let mut r = Review::new(&p, &settings, false);
    r.status = "preparing".into();
    r.thread_id = Some("fixture-thread".into());
    r.worktree = Some(dir.path().to_string_lossy().into());
    let id = r.id.clone();
    e.update(|s| {
        s.settings = settings;
        s.codex_ready = true;
        s.model_catalog_ready = true;
        s.models = catalog();
        s.prs.push(p);
        s.reviews.push(r);
    })
    .await;
    e.flush().await.unwrap();
    configure(&rpc, json!({})).await;
    (dir, e, rpc, id)
}
async fn configure(rpc: &Rpc, options: Value) {
    rpc.request("fixture/configure", options).await.unwrap();
}
async fn requests(rpc: &Rpc) -> Vec<Value> {
    rpc.request("fixture/requests", json!({}))
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}
async fn wait_for_request(rpc: &Rpc, method: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if requests(rpc).await.iter().any(|r| r["method"] == method) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Timed out waiting for {method}"));
}
async fn release(rpc: &Rpc) {
    rpc.request("fixture/release", json!({})).await.unwrap();
}
fn assert_no_inference(requests: &[Value]) {
    assert!(
        !requests
            .iter()
            .any(|r| r["method"] == "review/start" || r["method"] == "turn/start"),
        "{requests:?}"
    );
    assert!(!requests
        .iter()
        .any(|r| r.to_string().contains("allowProviderModelFallback")));
}
async fn completed(e: &Engine, legacy: bool) {
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.status = "posted".into();
        r.report = "Original report".into();
        r.comment_url = Some("https://github.com/a/b/pull/1#issuecomment-1".into());
        if legacy {
            r.model.clear();
        }
    })
    .await;
}

#[tokio::test]
async fn review_requires_exact_response_fields_before_inference() {
    for response in [
        json!({}),
        json!({"model":"model-a"}),
        json!({"reasoningEffort":"high"}),
        json!({"model":null,"reasoningEffort":"high"}),
        json!({"model":"model-a","reasoningEffort":null}),
        json!({"model":"other","reasoningEffort":"high"}),
        json!({"model":"model-a","reasoningEffort":"medium"}),
    ] {
        let (d, e, rpc, id) = fixture().await;
        configure(&rpc, json!({"startResponse":response})).await;
        let error = e
            .start_review_inference(&id, &rpc, d.path(), false)
            .await
            .unwrap_err();
        assert!(error.contains("did not confirm"), "{error}");
        let calls = requests(&rpc).await;
        assert_no_inference(&calls);
        assert_eq!(calls.last().unwrap()["method"], "thread/archive");
        assert_eq!(
            calls.last().unwrap()["params"]["threadId"],
            "fixture-thread"
        );
        let p = &calls[0]["params"];
        assert_eq!(p["model"], "model-a");
        assert_eq!(p["config"]["review_model"], "model-a");
        assert_eq!(p["config"]["model_reasoning_effort"], "high");
        assert!(e.snapshot().await.reviews[0]
            .confirmed_review_selection
            .is_none());
        rpc.shutdown().await;
    }
}
#[tokio::test]
async fn queued_selection_is_immutable_and_confirmation_is_durable_before_review() {
    let (d, e, rpc, _) = fixture().await;
    e.update(|s| {
        s.reviews.clear();
        s.settings.repositories.push(Repository {
            name: s.prs[0].repo.clone(),
            path: d.path().to_string_lossy().into(),
            automatic: false,
            setup_command: String::new(),
            network_access: false,
        });
    })
    .await;
    let id = e.enqueue("a/b#1", false).await.unwrap();
    e.update_settings(SettingsPatch {
        model: Some("model-b".into()),
        effort: Some("high".into()),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    e.update(|s| s.reviews[0].status = "preparing".into()).await;
    configure(&rpc, json!({"statePath":d.path().join("state.json")})).await;
    e.start_review_inference(&id, &rpc, d.path(), true)
        .await
        .unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.model, "model-b");
    assert_eq!(s.reviews[0].model, "model-a");
    assert_eq!(
        s.reviews[0]
            .confirmed_review_selection
            .as_ref()
            .unwrap()
            .model,
        "model-a"
    );
    let calls = requests(&rpc).await;
    assert_eq!(calls[0]["params"]["model"], "model-a");
    assert_eq!(calls[1]["method"], "review/start");
    assert!(!calls
        .iter()
        .any(|r| r.to_string().contains("allowProviderModelFallback")));
    rpc.shutdown().await;
}
#[tokio::test]
async fn invalid_queued_selection_fails_before_preparation_or_setup() {
    let (_d, e, rpc, id) = fixture().await;
    e.update(|s| s.models.clear()).await;
    let error = e.prepare(&id).await.unwrap_err();
    assert!(error.contains("unavailable"));
    assert!(requests(&rpc).await.is_empty());
    e.update(|s| {
        s.reviews.clear();
        s.settings.model.clear();
    })
    .await;
    assert!(e.enqueue("a/b#1", false).await.is_err());
    assert!(e.snapshot().await.reviews.is_empty());
    rpc.shutdown().await;
}
#[tokio::test]
async fn persistence_failure_stops_both_review_and_follow_up() {
    let (d, e, rpc, id) = fixture().await;
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e
        .start_review_inference(&id, &rpc, d.path(), false)
        .await
        .unwrap_err()
        .contains("Could not save confirmed"));
    assert_no_inference(&requests(&rpc).await);
    assert_eq!(
        requests(&rpc).await.last().unwrap()["method"],
        "thread/archive"
    );
    completed(&e, false).await;
    configure(&rpc, json!({})).await;
    assert!(e
        .message(&id, "Keep my question")
        .await
        .unwrap_err()
        .contains("Could not save the pending question"));
    let s = e.snapshot().await;
    assert_eq!(s.reviews[0].status, "posted");
    assert_eq!(s.reviews[0].report, "Original report");
    assert!(s.reviews[0].comment_url.is_some());
    assert_eq!(s.reviews[0].messages[0].text, "Keep my question");
    assert_eq!(
        s.reviews[0].messages[0].delivery,
        Some(MessageDelivery::Failed)
    );
    assert!(s.reviews[0].messages[0].confirmed_selection.is_none());
    assert!(requests(&rpc).await.is_empty());
    assert_no_inference(&requests(&rpc).await);
    rpc.shutdown().await;
}
#[tokio::test]
async fn follow_up_requires_exact_response_and_preserves_published_outcome_on_failure() {
    let (_d, e, rpc, id) = fixture().await;
    completed(&e, false).await;
    for response in [
        json!({}),
        json!({"model":"model-a"}),
        json!({"reasoningEffort":"high"}),
        json!({"model":null,"reasoningEffort":"high"}),
        json!({"model":"model-a","reasoningEffort":null}),
        json!({"model":"other","reasoningEffort":"high"}),
        json!({"model":"model-a","reasoningEffort":"low"}),
    ] {
        configure(&rpc, json!({"resumeResponse":response})).await;
        assert!(e
            .message(&id, "Explain")
            .await
            .unwrap_err()
            .contains("did not confirm"));
        let r = e.snapshot().await.reviews.remove(0);
        assert_eq!(r.status, "posted");
        assert_eq!(r.report, "Original report");
        assert!(r.comment_url.is_some() && r.before_follow_up.is_none());
        assert!(r.messages.iter().all(
            |m| m.delivery == Some(MessageDelivery::Failed) && m.confirmed_selection.is_none()
        ));
        assert_no_inference(&requests(&rpc).await);
    }
    rpc.shutdown().await;
}
#[tokio::test]
async fn warm_and_cold_legacy_resume_detach_once_and_pin_every_turn() {
    for warm in [false, true] {
        let (d, e, rpc, id) = fixture().await;
        completed(&e, true).await;
        e.set_legacy_chat_model(&id, "model-b".into(), "high".into())
            .await
            .unwrap();
        configure(
            &rpc,
            json!({"warm":warm,"statePath":d.path().join("state.json")}),
        )
        .await;
        e.message(&id, "First question").await.unwrap();
        let calls = requests(&rpc).await;
        assert_eq!(calls[0]["method"], "thread/unsubscribe");
        assert_eq!(calls[1]["method"], "thread/resume");
        assert_eq!(calls[1]["params"]["model"], "model-b");
        assert_eq!(
            calls[1]["params"]["config"]["model_reasoning_effort"],
            "high"
        );
        assert_eq!(calls[2]["params"]["model"], "model-b");
        assert_eq!(calls[2]["params"]["effort"], "high");
        e.update(|s| {
            s.reviews[0].end_follow_up(None);
            s.settings.model = "model-a".into();
            s.settings.effort = "low".into();
        })
        .await;
        // A warm resume of the already-pinned selection reports that same pair.
        configure(&rpc, json!({"warm":true,"resumeResponse":{"model":"model-b","reasoningEffort":"high"},"statePath":d.path().join("state.json")})).await;
        e.message(&id, "Second question").await.unwrap();
        let calls = requests(&rpc).await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["method"], "thread/resume");
        assert_eq!(calls[1]["params"]["model"], "model-b");
        let r = e.snapshot().await.reviews.remove(0);
        assert!(r.model.is_empty());
        assert_eq!(r.effort, "high");
        assert!(r.confirmed_review_selection.is_none());
        assert_eq!(
            r.messages[0].confirmed_selection.as_ref().unwrap().model,
            "model-b"
        );
        assert_eq!(
            r.messages[1].confirmed_selection,
            r.messages[0].confirmed_selection
        );
        assert_eq!(r.report, "Original report");
        rpc.shutdown().await;
    }
}
#[tokio::test]
async fn warm_resume_with_other_subscriber_cannot_silently_ignore_legacy_choice() {
    let (_d, e, rpc, id) = fixture().await;
    completed(&e, true).await;
    e.set_legacy_chat_model(&id, "model-a".into(), "high".into())
        .await
        .unwrap();
    configure(&rpc, json!({"warm":true,"otherSubscriber":true})).await;
    assert!(e
        .message(&id, "Explain")
        .await
        .unwrap_err()
        .contains("did not confirm"));
    assert_no_inference(&requests(&rpc).await);
    assert_eq!(e.snapshot().await.reviews[0].status, "posted");
    rpc.shutdown().await;
}
#[tokio::test]
async fn historical_explicit_selection_is_verified_for_chat_but_not_promoted_to_report_metadata() {
    for warm in [false, true] {
        let (d, e, rpc, id) = fixture().await;
        completed(&e, false).await;
        configure(&rpc, json!({"warm":warm,"resumeResponse":{"model":"model-a","reasoningEffort":"high"},"statePath":d.path().join("state.json")})).await;
        e.message(&id, "Explain").await.unwrap();
        let calls = requests(&rpc).await;
        assert_eq!(calls[0]["method"], "thread/resume");
        assert_eq!(calls[1]["params"]["model"], "model-a");
        assert_eq!(calls[1]["params"]["effort"], "high");
        let r = e.snapshot().await.reviews.remove(0);
        assert!(r.confirmed_review_selection.is_none());
        assert_eq!(
            r.messages[0].confirmed_selection.as_ref().unwrap().effort,
            "high"
        );
        assert!(models::report_labels(&r)
            .0
            .contains("requested, unverified"));
        rpc.shutdown().await;
    }
}
#[tokio::test]
async fn legacy_choice_is_validated_durable_idempotent_and_cannot_change() {
    let (d, e, rpc, id) = fixture().await;
    completed(&e, true).await;
    assert!(e
        .set_legacy_chat_model(&id, "model-b".into(), "low".into())
        .await
        .is_err());
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e
        .set_legacy_chat_model(&id, "model-b".into(), "high".into())
        .await
        .unwrap_err()
        .contains("not saved"));
    assert!(e.snapshot().await.reviews[0]
        .legacy_chat_selection
        .is_none());
    std::fs::remove_dir(d.path().join("state.json.tmp")).unwrap();
    e.set_legacy_chat_model(&id, "model-b".into(), "high".into())
        .await
        .unwrap();
    e.set_legacy_chat_model(&id, "model-b".into(), "high".into())
        .await
        .unwrap();
    assert!(e
        .set_legacy_chat_model(&id, "model-a".into(), "high".into())
        .await
        .unwrap_err()
        .contains("already been pinned"));
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap()).unwrap();
    assert!(saved.reviews[0].model.is_empty());
    assert_eq!(
        saved.reviews[0]
            .legacy_chat_selection
            .as_ref()
            .unwrap()
            .model,
        "model-b"
    );
    assert!(saved.reviews[0].confirmed_review_selection.is_none());
    assert_eq!(saved.reviews[0].report, "Original report");
    assert!(requests(&rpc).await.is_empty());
    for status in ["reviewing", "publishing"] {
        e.update(|s| s.reviews[0].status = status.into()).await;
        assert!(e
            .set_legacy_chat_model(&id, "model-b".into(), "high".into())
            .await
            .is_err());
    }
    e.update(|s| {
        s.reviews[0].status = "posted".into();
        s.reviews[0].cleanup_pending = true;
    })
    .await;
    assert!(e
        .set_legacy_chat_model(&id, "model-b".into(), "high".into())
        .await
        .is_err());
    e.update(|s| {
        s.reviews[0].cleanup_pending = false;
        s.reviews[0].worktree = None;
    })
    .await;
    assert!(e
        .set_legacy_chat_model(&id, "model-b".into(), "high".into())
        .await
        .is_err());
    rpc.shutdown().await;
}
#[tokio::test]
async fn reconnect_refreshes_existing_process_and_recovers_discovery_resolution_and_persistence() {
    let (d, e, rpc, _) = fixture().await;
    e.update(|s| {
        s.settings.model.clear();
        s.settings.effort.clear();
    })
    .await;
    configure(&rpc, json!({"catalog":catalog(),"failMethod":"model/list"})).await;
    assert!(e.connect().await.unwrap_err().contains("discovery failed"));
    assert!(e.snapshot().await.codex_ready);
    assert!(!e.snapshot().await.model_catalog_ready);
    configure(
        &rpc,
        json!({"catalog":catalog(),"effectiveModel":"model-b","failMethod":"config/read"}),
    )
    .await;
    assert!(e
        .connect()
        .await
        .unwrap_err()
        .contains("effective Codex settings"));
    configure(
        &rpc,
        json!({"catalog":catalog(),"effectiveModel":"model-b"}),
    )
    .await;
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e.connect().await.unwrap_err().contains("not saved"));
    assert!(e.snapshot().await.settings.model.is_empty());
    std::fs::remove_dir(d.path().join("state.json.tmp")).unwrap();
    e.connect().await.unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.model, "model-b");
    assert_eq!(s.settings.effort, "high");
    assert!(s.model_selection_notice.unwrap().contains("model-b · high"));
    assert!(s.model_configuration_error.is_none());
    assert!(Arc::ptr_eq(e.rpc.lock().await.as_ref().unwrap(), &rpc));
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(saved.settings.model, "model-b");
    assert_eq!(saved.settings.effort, "high");
    rpc.shutdown().await;
}
#[tokio::test]
async fn resolution_merges_current_state_and_invalid_explicit_selection_allows_unrelated_settings()
{
    let (_d, e, rpc, _) = fixture().await;
    // An explicit edit made while discovery was in flight wins over its config.
    e.update_settings(SettingsPatch {
        model: Some("model-b".into()),
        effort: Some("high".into()),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    e.apply_model_discovery(catalog(), Ok(json!({"config":{"model":"model-a"}})))
        .await
        .unwrap();
    assert_eq!(e.snapshot().await.settings.model, "model-b");
    e.update(|s| {
        s.settings.model.clear();
        s.settings.effort.clear();
        s.settings.automatic = true;
    })
    .await;
    configure(
        &rpc,
        json!({"catalog":catalog(),"effectiveModel":"model-a","delayMethod":"config/read"}),
    )
    .await;
    let clone = e.clone();
    let connecting = tokio::spawn(async move { clone.connect().await });
    loop {
        if requests(&rpc)
            .await
            .iter()
            .any(|r| r["method"] == "config/read")
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    e.update_settings(SettingsPatch {
        automatic: Some(false),
        poll_seconds: Some(300),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    connecting.await.unwrap().unwrap();
    let s = e.snapshot().await;
    assert!(!s.settings.automatic);
    assert_eq!(s.settings.poll_seconds, 300);
    assert_eq!(s.settings.model, "model-a");
    // Explicit model no longer available: keep it and block only model execution.
    e.apply_model_discovery(vec![catalog()[1].clone()], Ok(Value::Null))
        .await
        .unwrap_err();
    e.update_settings(SettingsPatch {
        automatic: Some(false),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    assert_eq!(e.snapshot().await.settings.model, "model-a");
    assert!(e.snapshot().await.model_configuration_error.is_some());
    e.update_settings(SettingsPatch {
        model: Some("model-b".into()),
        effort: Some("high".into()),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    assert!(e.snapshot().await.model_configuration_error.is_none());
    rpc.shutdown().await;
}

#[tokio::test]
async fn rejected_settings_and_queue_writes_do_not_activate_unsaved_selections_or_runs() {
    let (d, e, rpc, _) = fixture().await;
    e.update(|s| {
        s.reviews.clear();
        s.settings.repositories.push(Repository {
            name: s.prs[0].repo.clone(),
            path: d.path().to_string_lossy().into(),
            automatic: false,
            setup_command: String::new(),
            network_access: false,
        });
    })
    .await;
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e
        .update_settings(SettingsPatch {
            model: Some("model-b".into()),
            effort: Some("high".into()),
            ..SettingsPatch::default()
        })
        .await
        .unwrap_err()
        .contains("not saved"));
    assert_eq!(e.snapshot().await.settings.model, "model-a");
    assert!(e
        .enqueue("a/b#1", false)
        .await
        .unwrap_err()
        .contains("not queued"));
    assert!(e.snapshot().await.reviews.is_empty());
    std::fs::remove_dir(d.path().join("state.json.tmp")).unwrap();
    assert!(e
        .update_settings(SettingsPatch {
            model: Some("model-b".into()),
            effort: Some("low".into()),
            ..SettingsPatch::default()
        })
        .await
        .is_err());
    assert_eq!(e.snapshot().await.settings.model, "model-a");
    e.update_settings(SettingsPatch {
        model: Some("model-b".into()),
        effort: Some("high".into()),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(saved.settings.model, "model-b");
    assert_eq!(saved.settings.effort, "high");
    assert!(requests(&rpc).await.is_empty());
    rpc.shutdown().await;
}

#[tokio::test]
async fn malformed_or_duplicate_catalogs_block_without_disconnecting_codex() {
    let (_d, e, rpc, _) = fixture().await;
    for models in [
        vec![],
        vec![json!({"model":"incomplete"})],
        vec![catalog()[0].clone(), catalog()[0].clone()],
    ] {
        configure(&rpc, json!({"catalog":models})).await;
        assert!(e
            .connect()
            .await
            .unwrap_err()
            .contains("Model discovery failed"));
        let s = e.snapshot().await;
        assert!(s.codex_ready);
        assert!(!s.model_catalog_ready);
        assert!(s.models.is_empty());
        assert_eq!(s.settings.model, "model-a");
    }
    rpc.shutdown().await;
}

#[tokio::test]
async fn reconnect_keeps_verified_catalog_available_for_review_and_follow_up_until_refresh_finishes(
) {
    for follow_up in [false, true] {
        let (d, e, rpc, id) = fixture().await;
        if follow_up {
            completed(&e, false).await;
        }
        configure(
            &rpc,
            json!({"holdMethod":"model/list", "catalog":catalog()}),
        )
        .await;
        let clone = e.clone();
        let connecting = tokio::spawn(async move { clone.connect().await });
        wait_for_request(&rpc, "model/list").await;
        let s = e.snapshot().await;
        assert!(s.model_catalog_ready && s.model_configuration_error.is_none());
        if follow_up {
            e.message(&id, "During refresh").await.unwrap();
        } else {
            e.start_review_inference(&id, &rpc, d.path(), false)
                .await
                .unwrap();
        }
        release(&rpc).await;
        connecting.await.unwrap().unwrap();
        assert_eq!(e.snapshot().await.reviews[0].status, "reviewing");
        rpc.shutdown().await;
    }
}

#[tokio::test]
async fn pending_question_is_visible_and_durable_before_resume_then_confirmed_on_same_message() {
    let (d, e, rpc, id) = fixture().await;
    completed(&e, false).await;
    configure(
        &rpc,
        json!({"holdMethod":"thread/resume", "statePath":d.path().join("state.json")}),
    )
    .await;
    let clone = e.clone();
    let sending = tokio::spawn(async move { clone.message(&id, "Slow resume").await });
    wait_for_request(&rpc, "thread/resume").await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.messages.len(), 1);
    let message_id = r.messages[0].id.clone();
    assert_eq!(r.messages[0].text, "Slow resume");
    assert_eq!(r.messages[0].delivery, Some(MessageDelivery::Pending));
    assert!(r.messages[0].confirmed_selection.is_none());
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(
        saved.reviews[0].messages[0].delivery,
        Some(MessageDelivery::Pending)
    );
    assert_no_inference(&requests(&rpc).await);
    release(&rpc).await;
    sending.await.unwrap().unwrap();
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.messages.len(), 1);
    assert_eq!(r.messages[0].id, message_id);
    assert_eq!(r.messages[0].delivery, Some(MessageDelivery::Sent));
    assert_eq!(
        r.messages[0].confirmed_selection.as_ref().unwrap().model,
        "model-a"
    );
    rpc.shutdown().await;
}

#[tokio::test]
async fn confirmation_save_failure_keeps_question_without_starting_turn() {
    let (d, e, rpc, id) = fixture().await;
    completed(&e, false).await;
    configure(&rpc, json!({"holdMethod":"thread/resume"})).await;
    let clone = e.clone();
    let sending = tokio::spawn(async move { clone.message(&id, "Keep after save failure").await });
    wait_for_request(&rpc, "thread/resume").await;
    std::fs::create_dir(d.path().join("state.json.tmp")).unwrap();
    release(&rpc).await;
    assert!(sending
        .await
        .unwrap()
        .unwrap_err()
        .contains("Could not save confirmed chat"));
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "posted");
    assert_eq!(r.messages[0].delivery, Some(MessageDelivery::Failed));
    assert!(r.messages[0].confirmed_selection.is_none());
    assert_eq!(r.report, "Original report");
    assert!(r.comment_url.is_some());
    assert_no_inference(&requests(&rpc).await);
    rpc.shutdown().await;
}

#[tokio::test]
async fn stopped_resume_cannot_confirm_or_end_a_later_question() {
    let (_d, e, rpc, id) = fixture().await;
    completed(&e, false).await;
    configure(&rpc, json!({"holdMethod":"thread/resume", "resumeResponse":{"model":"model-a", "reasoningEffort":"high"}})).await;
    let clone = e.clone();
    let first_id = id.clone();
    let first = tokio::spawn(async move { clone.message(&first_id, "Stopped question").await });
    wait_for_request(&rpc, "thread/resume").await;
    e.cancel(&id).await.unwrap();
    let clone = e.clone();
    let second = tokio::spawn(async move { clone.message(&id, "Current question").await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests(&rpc)
            .await
            .iter()
            .filter(|r| r["method"] == "thread/resume")
            .count()
            < 2
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    release(&rpc).await;
    assert!(first.await.unwrap().unwrap_err().contains("stopped"));
    second.await.unwrap().unwrap();
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "reviewing");
    assert_eq!(r.messages[0].delivery, Some(MessageDelivery::Interrupted));
    assert!(r.messages[0].confirmed_selection.is_none());
    assert_eq!(r.messages[1].delivery, Some(MessageDelivery::Sent));
    assert!(r.messages[1].confirmed_selection.is_some());
    assert_eq!(
        requests(&rpc)
            .await
            .iter()
            .filter(|r| r["method"] == "turn/start")
            .count(),
        1
    );
    rpc.shutdown().await;
}

#[tokio::test]
async fn pending_questions_recover_as_failed_after_restart() {
    let (d, e, rpc, id) = fixture().await;
    completed(&e, false).await;
    configure(&rpc, json!({"holdMethod":"thread/resume"})).await;
    let clone = e.clone();
    let sending = tokio::spawn(async move { clone.message(&id, "Interrupted by restart").await });
    wait_for_request(&rpc, "thread/resume").await;
    // Copy the state as it was on disk at the instant the process exited.
    let recovered_dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        d.path().join("state.json"),
        recovered_dir.path().join("state.json"),
    )
    .unwrap();
    let recovered = Engine::new(recovered_dir.path().into(), |_| {}).unwrap();
    let r = recovered.snapshot().await.reviews.remove(0);
    assert_eq!(r.messages[0].delivery, Some(MessageDelivery::Failed));
    assert!(r.messages[0].confirmed_selection.is_none());
    assert_eq!(r.status, "posted");
    assert_eq!(r.report, "Original report");
    assert!(r.comment_url.is_some());
    sending.abort();
    rpc.shutdown().await;
}

#[tokio::test]
async fn stopping_preparation_still_archives_a_late_created_thread() {
    let (d, e, rpc, id) = fixture().await;
    e.update(|s| {
        s.reviews[0].thread_id = None;
        s.reviews[0].worktree = None;
    })
    .await;
    configure(&rpc, json!({"holdMethod":"thread/start"})).await;
    let clone = e.clone();
    let task_id = id.clone();
    let client = rpc.clone();
    let path = d.path().to_path_buf();
    let preparing = tokio::spawn(async move {
        clone
            .start_review_inference(&task_id, &client, &path, false)
            .await
    });
    e.preparing
        .lock()
        .unwrap()
        .insert(id.clone(), preparing.abort_handle());
    wait_for_request(&rpc, "thread/start").await;
    e.cancel(&id).await.unwrap();
    assert!(preparing.await.unwrap_err().is_cancelled());
    release(&rpc).await;
    wait_for_request(&rpc, "thread/archive").await;
    assert_no_inference(&requests(&rpc).await);
    assert_eq!(e.snapshot().await.reviews[0].status, "cancelled");
    rpc.shutdown().await;
}

#[tokio::test]
async fn archive_failure_unsubscribes_and_records_unused_thread_identity() {
    let (d, e, rpc, id) = fixture().await;
    configure(
        &rpc,
        json!({"startResponse":{}, "failMethod":"thread/archive"}),
    )
    .await;
    assert!(e
        .start_review_inference(&id, &rpc, d.path(), false)
        .await
        .unwrap_err()
        .contains("did not confirm"));
    let calls = requests(&rpc).await;
    assert_eq!(calls[1]["method"], "thread/archive");
    assert_eq!(calls[2]["method"], "thread/unsubscribe");
    assert_no_inference(&calls);
    assert!(e.snapshot().await.reviews[0].messages[0]
        .text
        .contains("Unused Codex thread fixture-thread"));
    rpc.shutdown().await;
}

#[tokio::test]
async fn resolution_notice_is_cleared_and_persisted_after_next_successful_connection() {
    let (d, e, rpc, _) = fixture().await;
    e.update(|s| {
        s.settings.model.clear();
        s.settings.effort.clear();
    })
    .await;
    configure(&rpc, json!({"catalog":catalog()})).await;
    e.connect().await.unwrap();
    let notice = e.snapshot().await.model_selection_notice;
    assert!(notice.is_some());
    configure(&rpc, json!({"failMethod":"model/list"})).await;
    assert!(e.connect().await.is_err());
    assert_eq!(e.snapshot().await.model_selection_notice, notice);
    configure(&rpc, json!({"catalog":catalog()})).await;
    e.connect().await.unwrap();
    assert!(e.snapshot().await.model_selection_notice.is_none());
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap()).unwrap();
    assert!(saved.model_selection_notice.is_none());
    rpc.shutdown().await;
}

#[tokio::test]
async fn connection_resolves_only_missing_values_from_config_and_persists_their_sources() {
    for (
        model,
        effort,
        effective_model,
        effective_effort,
        fail_config,
        expected_model,
        expected_effort,
        reason,
    ) in [
        (
            "",
            "",
            "model-a",
            "high",
            false,
            "model-a",
            "high",
            "Effort: your Codex configuration.",
        ),
        (
            "",
            "",
            "model-b",
            "low",
            false,
            "model-b",
            "high",
            "your configured effort 'low' isn't supported",
        ),
        (
            "model-a",
            "",
            "model-b",
            "low",
            false,
            "model-a",
            "low",
            "Model: your saved Orbit selection.",
        ),
        (
            "model-a",
            "",
            "model-b",
            "high",
            true,
            "model-a",
            "medium",
            "Codex configuration could not be read",
        ),
        (
            "",
            "low",
            "model-a",
            "high",
            false,
            "model-a",
            "low",
            "Effort: your saved Orbit selection.",
        ),
        (
            "model-a", "low", "model-b", "high", false, "model-a", "low", "",
        ),
    ] {
        let (d, e, rpc, _) = fixture().await;
        e.update(|s| {
            s.settings.model = model.into();
            s.settings.effort = effort.into();
        })
        .await;
        configure(&rpc, json!({"catalog":catalog(), "effectiveModel":effective_model, "effectiveEffort":effective_effort,
            "failMethod":if fail_config { "config/read" } else { "" }})).await;
        e.connect().await.unwrap();
        let s = e.snapshot().await;
        assert_eq!(s.settings.model, expected_model);
        assert_eq!(s.settings.effort, expected_effort);
        assert!(s.model_configuration_error.is_none());
        assert_eq!(
            requests(&rpc)
                .await
                .iter()
                .any(|r| r["method"] == "config/read"),
            model.is_empty() || effort.is_empty()
        );
        if reason.is_empty() {
            assert!(s.model_selection_notice.is_none());
        } else {
            assert!(s.model_selection_notice.as_ref().unwrap().contains(reason));
            let saved: Snapshot =
                serde_json::from_slice(&std::fs::read(d.path().join("state.json")).unwrap())
                    .unwrap();
            assert_eq!(saved.settings.model, expected_model);
            assert_eq!(saved.settings.effort, expected_effort);
            assert_eq!(saved.model_selection_notice, s.model_selection_notice);
        }
        // Migrating workspace settings cannot rewrite an existing run.
        assert_eq!(s.reviews[0].model, "model-a");
        assert_eq!(s.reviews[0].effort, "high");
        assert_no_inference(&requests(&rpc).await);
        rpc.shutdown().await;
    }
}

#[tokio::test]
async fn effort_migration_cannot_replace_a_concurrent_explicit_selection() {
    let (_d, e, rpc, _) = fixture().await;
    e.update(|s| s.settings.effort.clear()).await;
    configure(
        &rpc,
        json!({"catalog":catalog(), "effectiveEffort":"high", "holdMethod":"config/read"}),
    )
    .await;
    let clone = e.clone();
    let connecting = tokio::spawn(async move { clone.connect().await });
    wait_for_request(&rpc, "config/read").await;
    e.update_settings(SettingsPatch {
        model: Some("model-a".into()),
        effort: Some("low".into()),
        ..SettingsPatch::default()
    })
    .await
    .unwrap();
    release(&rpc).await;
    connecting.await.unwrap().unwrap();
    let s = e.snapshot().await;
    assert_eq!(s.settings.effort, "low");
    assert!(s.model_selection_notice.is_none());
    rpc.shutdown().await;
}
async fn awaiting_extraction(e: &Engine) {
    e.update(|s| {
        let r = &mut s.reviews[0];
        r.status = "reviewing".into();
        r.report = "[P1] A finding".into();
        r.extracting = true;
    })
    .await;
    e.flush().await.unwrap();
}
#[tokio::test]
async fn extraction_pins_the_review_selection_and_saves_confirmation_before_inference() {
    let (d, e, rpc, id) = fixture().await;
    awaiting_extraction(&e).await;
    configure(&rpc, json!({"statePath":d.path().join("state.json")})).await;
    e.extract_findings(&id).await;
    let calls = requests(&rpc).await;
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0]["method"], "thread/resume");
    assert_eq!(calls[0]["params"]["model"], "model-a");
    assert_eq!(
        calls[0]["params"]["config"]["model_reasoning_effort"],
        "high"
    );
    // The fixture refuses turn/start unless the confirmation is already on disk.
    let turn = &calls[1];
    assert_eq!(turn["method"], "turn/start");
    assert_eq!(turn["params"]["model"], "model-a");
    assert_eq!(turn["params"]["effort"], "high");
    assert_eq!(turn["params"]["outputSchema"]["required"][2], "findings");
    let r = e.snapshot().await.reviews.remove(0);
    assert!(r.extracting && r.status == "reviewing" && r.error.is_none());
    assert_eq!(r.turn_id.as_deref(), Some("fixture-turn"));
    assert_eq!(r.extraction_selection.unwrap().model, "model-a");
    let text =
        json!({"verdict":"clean","summary":"Nothing to fix.","findings":[],"testsNotRun":[]});
    e.event(json!({"method":"item/completed","params":{"threadId":"fixture-thread","turnId":"fixture-turn","item":{"id":"j","type":"agentMessage","text":text.to_string()}}})).await;
    e.event(json!({"method":"turn/completed","params":{"threadId":"fixture-thread","turn":{"id":"fixture-turn","status":"completed"}}})).await;
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "ready");
    assert_eq!(r.verdict, Some(Verdict::Clean));
    assert_eq!(r.findings, Some(vec![]));
    assert_eq!(r.summary.as_deref(), Some("Nothing to fix."));
    rpc.shutdown().await;
}
#[tokio::test]
async fn extraction_selection_mismatch_stops_before_inference_and_keeps_the_report() {
    let (_d, e, rpc, id) = fixture().await;
    awaiting_extraction(&e).await;
    configure(
        &rpc,
        json!({"resumeResponse":{"model":"other","reasoningEffort":"high"}}),
    )
    .await;
    e.extract_findings(&id).await;
    assert_no_inference(&requests(&rpc).await);
    let r = e.snapshot().await.reviews.remove(0);
    assert_eq!(r.status, "ready");
    assert_eq!(r.report, "[P1] A finding");
    assert!(r.findings.is_none() && r.extraction_selection.is_none() && !r.extracting);
    let error = r.error.unwrap();
    assert!(
        error.contains("unstructured") && error.contains("did not confirm"),
        "{error}"
    );
    rpc.shutdown().await;
}
