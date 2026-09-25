use super::*;
use crate::models::tests::catalog;

async fn fixture() -> (tempfile::TempDir, Arc<Engine>, Arc<Rpc>, String) {
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(dir.path().into(), |_| {}).unwrap();
    let (tx, rx) = mpsc::channel(64);
    let mut cmd = tokio::process::Command::new("node");
    cmd.args(["-e", include_str!("model_fixture.cjs")]);
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
    completed(&e, false).await;
    configure(&rpc, json!({})).await;
    assert!(e
        .message(&id, "Keep my question")
        .await
        .unwrap_err()
        .contains("Could not save confirmed"));
    let s = e.snapshot().await;
    assert_eq!(s.reviews[0].status, "posted");
    assert_eq!(s.reviews[0].report, "Original report");
    assert!(s.reviews[0].comment_url.is_some());
    assert!(s.reviews[0].messages.is_empty());
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
        assert!(r.messages.is_empty());
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
