use super::*;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    routing::{get, post},
    Json, Router,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// A password store in memory.
#[derive(Default)]
pub(crate) struct Memory {
    pub keys: Mutex<HashMap<String, String>>,
    /// An error every use of the store gives.
    pub broken: Option<String>,
}
impl Secrets for Memory {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match &self.broken {
            Some(e) => Err(e.clone()),
            None => Ok(self.keys.lock().unwrap().get(name).cloned()),
        }
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        match &self.broken {
            Some(e) => Err(e.clone()),
            None => {
                self.keys.lock().unwrap().insert(name.into(), value.into());
                Ok(())
            }
        }
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        match &self.broken {
            Some(e) => Err(e.clone()),
            None => {
                self.keys.lock().unwrap().remove(name);
                Ok(())
            }
        }
    }
}

/// The key the fake providers take.
pub(crate) const KEY: &str = "sk-test-0123456789";

/// What a fake OpenAI, Anthropic and Google have and say, all at one
/// address.
#[derive(Clone, Default)]
pub(crate) struct Fake {
    pub models: Vec<String>,
    /// The answer's text.
    pub content: String,
    /// The model it says answered; None says the one asked for.
    pub reported: Option<String>,
    /// How the answer ended, when not as usual: OpenAI's status, Anthropic's
    /// stop reason or Gemini's finish reason.
    pub stop: Option<String>,
    /// How long it takes to answer.
    pub delay: Duration,
    /// The requests it was sent: path, key and body.
    pub requests: Arc<Mutex<Vec<(String, String, Value)>>>,
}

/// The key a request carries, in the header its provider uses.
fn key_of(path: &str, headers: &HeaderMap) -> String {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    if path.starts_with("/v1beta/") {
        header("x-goog-api-key")
    } else if header("anthropic-version") == "2023-06-01" {
        header("x-api-key")
    } else {
        header("authorization")
            .strip_prefix("Bearer ")
            .unwrap_or_default()
            .to_string()
    }
}

/// Starts fake providers on this computer and returns their address.
pub(crate) async fn serve(fake: Fake) -> String {
    fn checked(
        f: &Fake,
        uri: &Uri,
        headers: &HeaderMap,
        body: Value,
    ) -> Result<(), (StatusCode, Json<Value>)> {
        let key = key_of(uri.path(), headers);
        f.requests
            .lock()
            .unwrap()
            .push((uri.to_string(), key.clone(), body));
        if key != KEY {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"message": format!("Incorrect API key provided: {key}")}})),
            ));
        }
        Ok(())
    }
    async fn list(
        State(f): State<Fake>,
        uri: Uri,
        headers: HeaderMap,
    ) -> (StatusCode, Json<Value>) {
        if let Err(e) = checked(&f, &uri, &headers, Value::Null) {
            return e;
        }
        let body = if uri.path().starts_with("/v1beta/") {
            json!({"models": f.models.iter().map(|m| json!({
                "name": format!("models/{m}"),
                "supportedGenerationMethods": if m.contains("embedding") {
                    json!(["embedContent"])
                } else {
                    json!(["generateContent", "countTokens"])
                },
            })).collect::<Vec<_>>()})
        } else {
            json!({"data": f.models.iter().map(|m| json!({
                "id": m,
                "capabilities": {"structured_outputs": {"supported": !m.starts_with("claude-3")}},
            })).collect::<Vec<_>>()})
        };
        (StatusCode::OK, Json(body))
    }
    async fn answer(
        f: Fake,
        uri: Uri,
        headers: HeaderMap,
        model: String,
        body: Value,
    ) -> (StatusCode, Json<Value>) {
        if let Err(e) = checked(&f, &uri, &headers, body.clone()) {
            return e;
        }
        tokio::time::sleep(f.delay).await;
        let reported = f.reported.clone().unwrap_or(model);
        let stop = f.stop.clone();
        let body = match uri.path() {
            "/v1/responses" => json!({
                "model": reported,
                "status": stop.clone().unwrap_or("completed".into()),
                "incomplete_details": stop.map(|_| json!({"reason": "max_output_tokens"})),
                "output": [
                    {"type": "reasoning", "summary": []},
                    {"type": "message", "content": [{"type": "output_text", "text": f.content}]},
                ],
            }),
            "/v1/messages" => json!({
                "model": reported,
                "stop_reason": stop.unwrap_or("end_turn".into()),
                "content": [
                    {"type": "thinking", "thinking": "Hmm."},
                    {"type": "text", "text": f.content},
                ],
            }),
            _ => json!({
                "modelVersion": reported,
                "candidates": [{
                    "finishReason": stop.unwrap_or("STOP".into()),
                    "content": {"parts": [
                        {"text": "Thinking it over.", "thought": true},
                        {"text": f.content},
                    ]},
                }],
            }),
        };
        (StatusCode::OK, Json(body))
    }
    async fn chat(
        State(f): State<Fake>,
        uri: Uri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let model = body["model"].as_str().unwrap_or_default().to_string();
        answer(f, uri, headers, model, body).await
    }
    async fn generate(
        State(f): State<Fake>,
        Path(call): Path<String>,
        uri: Uri,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let model = call.trim_end_matches(":generateContent").to_string();
        answer(f, uri, headers, model, body).await
    }
    let app = Router::new()
        .route("/v1/models", get(list))
        .route("/v1beta/models", get(list))
        .route("/v1/responses", post(chat))
        .route("/v1/messages", post(chat))
        .route("/v1beta/models/{call}", post(generate))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    url
}

/// Bit's answer form, small enough to read in a test.
fn schema() -> Value {
    json!({"type": "object", "properties": {"answer": {"type": "string"}}})
}

#[test]
fn keys_are_pasted_whole_on_one_line() {
    assert!(validate_key(KEY).is_ok());
    assert!(validate_key("sk-ant-api03-abcdefgh_ijkl-mnop").is_ok());
    for key in ["", "short", "sk-test 0123456789", "sk-test-0123\n456789"] {
        assert!(validate_key(key).is_err(), "{key:?}");
    }
    assert!(validate_key(&"k".repeat(MAX_KEY + 1)).is_err());
}

#[test]
fn a_dated_version_of_a_model_is_that_model() {
    assert!(same_model("gpt-5", "gpt-5"));
    assert!(same_model("gpt-4o", "gpt-4o-2024-08-06"));
    assert!(same_model("claude-opus-4-5", "claude-opus-4-5-20251101"));
    assert!(same_model("gemini-2.5-flash", "models/gemini-2.5-flash"));
    assert!(same_model("gemini-1.5-pro", "gemini-1.5-pro-002"));
    assert!(!same_model("gpt-5", "gpt-5-mini"));
    assert!(!same_model("gpt-4o", "gpt-4o-mini-2024-07-18"));
    assert!(!same_model("gemini-2.5-flash", "gemini-2.5-flash-lite"));
    assert!(!same_model("gpt-5", ""));
    assert!(!same_model("gpt-5", "gpt-5-"));
}

#[tokio::test]
async fn each_provider_lists_only_the_models_that_can_answer_in_bits_form() {
    let fake = Fake {
        models: [
            "gpt-5",
            "gpt-5-mini",
            "o3",
            "o1",
            "o4-mini",
            "gpt-4.1",
            "gpt-4o",
            "gpt-4o-mini-2024-07-18",
            // Text models that can't answer in a strict JSON schema.
            "gpt-3.5-turbo",
            "gpt-4",
            "gpt-4-turbo",
            "gpt-4o-2024-05-13",
            "o1-mini",
            "o1-preview",
            "chatgpt-4o-latest",
            // Models for other work.
            "text-embedding-3-small",
            "gpt-4o-realtime-preview",
            "dall-e-3",
            "omni-moderation-latest",
            "gpt-image-1",
            "claude-opus-5-5",
            "claude-3-haiku-20240307",
            "gemini-2.5-flash",
            "gemini-1.5-pro",
            "gemma-3-27b-it",
            "gemini-2.5-flash-preview-tts",
            "gemini-embedding-001",
        ]
        .map(String::from)
        .to_vec(),
        ..Default::default()
    };
    let requests = fake.requests.clone();
    let base = serve(fake).await;
    assert_eq!(
        models(Api::OpenAi, &base, KEY).await.unwrap(),
        [
            "gpt-4.1",
            "gpt-4o",
            "gpt-4o-mini-2024-07-18",
            "gpt-5",
            "gpt-5-mini",
            "o1",
            "o3",
            "o4-mini"
        ]
    );
    // Anthropic says which can answer in a set form; it keeps its order.
    let anthropic = models(Api::Anthropic, &base, KEY).await.unwrap();
    assert!(anthropic.contains(&"claude-opus-5-5".to_string()));
    assert!(!anthropic.contains(&"claude-3-haiku-20240307".to_string()));
    assert_eq!(
        models(Api::Google, &base, KEY).await.unwrap(),
        ["gemini-2.5-flash"]
    );
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0].0, "/v1/models");
    assert_eq!(requests[1].0, "/v1/models?limit=1000");
    assert_eq!(requests[2].0, "/v1beta/models?pageSize=1000");
    assert!(requests.iter().all(|(_, key, _)| key == KEY));
}

#[tokio::test]
async fn a_key_a_provider_turns_down_is_named_but_never_shown() {
    let base = serve(Fake::default()).await;
    for api in Api::ALL {
        let error = models(api, &base, "sk-wrong-key-123").await.unwrap_err();
        assert!(error.contains("didn't accept your key"), "{error}");
        assert!(!error.contains("sk-wrong"), "{error}");
        let error = answer(
            api,
            &base,
            "sk-wrong-key-123",
            "m",
            "",
            "",
            &schema(),
            CHECK_TIMEOUT,
        )
        .await
        .unwrap_err();
        assert!(error.contains("didn't accept your key"), "{error}");
    }
    // Nothing answering.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = models(Api::Anthropic, &gone, KEY).await.unwrap_err();
    assert_eq!(
        error,
        "Orbit couldn't reach the Anthropic API. Check this computer's internet connection."
    );
}

#[tokio::test]
async fn each_provider_is_asked_for_an_answer_in_bits_form_and_nothing_else() {
    let fake = Fake {
        content: r#"{"answer":"Hi."}"#.into(),
        ..Default::default()
    };
    let requests = fake.requests.clone();
    let base = serve(fake).await;
    let ask = |api, model: &'static str| {
        let base = base.clone();
        async move {
            answer(
                api,
                &base,
                KEY,
                model,
                "You are Bit.",
                "Hi?",
                &schema(),
                CHECK_TIMEOUT,
            )
            .await
        }
    };
    for (api, model) in [
        (Api::OpenAi, "gpt-5"),
        (Api::Anthropic, "claude-opus-5-5"),
        (Api::Google, "gemini-2.5-flash"),
    ] {
        assert_eq!(ask(api, model).await.unwrap(), r#"{"answer":"Hi."}"#);
    }
    let requests = requests.lock().unwrap();
    let (path, _, openai) = &requests[0];
    assert_eq!(path, "/v1/responses");
    assert_eq!(
        openai,
        &json!({
            "model": "gpt-5",
            "instructions": "You are Bit.",
            "input": "Hi?",
            "text": {"format": {"type": "json_schema", "name": "bit_answer", "schema": schema(), "strict": true}},
            "store": false,
        })
    );
    let (path, _, anthropic) = &requests[1];
    assert_eq!(path, "/v1/messages");
    assert_eq!(
        anthropic,
        &json!({
            "model": "claude-opus-5-5",
            "max_tokens": ANTHROPIC_MAX_TOKENS,
            "system": "You are Bit.",
            "messages": [{"role": "user", "content": "Hi?"}],
            "output_config": {"format": {"type": "json_schema", "schema": schema()}},
        })
    );
    let (path, _, google) = &requests[2];
    assert_eq!(path, "/v1beta/models/gemini-2.5-flash:generateContent");
    assert_eq!(
        google,
        &json!({
            "systemInstruction": {"parts": [{"text": "You are Bit."}]},
            "contents": [{"role": "user", "parts": [{"text": "Hi?"}]}],
            "generationConfig": {"responseMimeType": "application/json", "responseJsonSchema": schema()},
        })
    );
    // None of them was offered a tool.
    assert!(requests
        .iter()
        .all(|(_, _, body)| body.get("tools").is_none()));
}

#[tokio::test]
async fn an_answer_from_another_model_or_cut_short_isnt_used() {
    let fake = Fake {
        content: "{}".into(),
        reported: Some("gpt-5-mini".into()),
        ..Default::default()
    };
    let base = serve(fake).await;
    let error = answer(
        Api::OpenAi,
        &base,
        KEY,
        "gpt-5",
        "",
        "",
        &schema(),
        CHECK_TIMEOUT,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        "OpenAI didn't confirm the model 'gpt-5' (reported: gpt-5-mini), so I didn't use its answer."
    );
    // A dated version is the same model.
    let dated = serve(Fake {
        content: "{}".into(),
        reported: Some("claude-opus-4-5-20251101".into()),
        ..Default::default()
    })
    .await;
    assert_eq!(
        answer(
            Api::Anthropic,
            &dated,
            KEY,
            "claude-opus-4-5",
            "",
            "",
            &schema(),
            CHECK_TIMEOUT
        )
        .await
        .unwrap(),
        "{}"
    );
    for (api, stop, said) in [
        (
            Api::OpenAi,
            "incomplete",
            "OpenAI stopped before it finished (max_output_tokens).",
        ),
        (Api::Anthropic, "refusal", "Anthropic declined to answer."),
        (
            Api::Anthropic,
            "max_tokens",
            "Anthropic ran out of room before it finished.",
        ),
        (
            Api::Google,
            "SAFETY",
            "Google stopped before it finished (SAFETY).",
        ),
    ] {
        let base = serve(Fake {
            content: "{}".into(),
            stop: Some(stop.into()),
            ..Default::default()
        })
        .await;
        let error = answer(api, &base, KEY, "m", "", "", &schema(), CHECK_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(error, said);
    }
}

#[tokio::test]
async fn keys_are_saved_read_and_removed_in_the_store() {
    let store = Arc::new(Memory::default());
    let setup = Setup {
        secrets: store.clone(),
        ..Default::default()
    };
    assert_eq!(setup.key(Api::Google).await.unwrap(), None);
    setup.save(Api::Google, Some(KEY.into())).await.unwrap();
    assert_eq!(setup.key(Api::Google).await.unwrap().as_deref(), Some(KEY));
    assert_eq!(
        store
            .keys
            .lock()
            .unwrap()
            .get("google-api-key")
            .map(String::as_str),
        Some(KEY)
    );
    setup.save(Api::Google, None).await.unwrap();
    assert_eq!(setup.key(Api::Google).await.unwrap(), None);
    // Removing a key that isn't there is fine.
    setup.save(Api::Google, None).await.unwrap();
}
