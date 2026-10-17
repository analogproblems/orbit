use super::*;
use axum::{extract::State, routing::get, routing::post, Json, Router};
use std::sync::{Arc, Mutex};

/// What a fake Ollama has and says.
#[derive(Clone, Default)]
pub(crate) struct Fake {
    pub models: Vec<String>,
    /// The answer's content.
    pub content: String,
    /// The model it says answered; None says the one asked for.
    pub reported: Option<String>,
    /// An error to answer every chat with.
    pub error: Option<String>,
    /// How long it takes to answer a chat.
    pub delay: Duration,
    /// The chats it was sent.
    pub chats: Arc<Mutex<Vec<Value>>>,
}

/// Starts a fake Ollama on this computer and returns its address.
pub(crate) async fn serve(fake: Fake) -> String {
    async fn tags(State(f): State<Fake>) -> Json<Value> {
        Json(
            json!({"models": f.models.iter().map(|m| json!({"name": m, "model": m})).collect::<Vec<_>>()}),
        )
    }
    async fn chat(
        State(f): State<Fake>,
        Json(body): Json<Value>,
    ) -> (axum::http::StatusCode, Json<Value>) {
        f.chats.lock().unwrap().push(body.clone());
        tokio::time::sleep(f.delay).await;
        if let Some(error) = &f.error {
            return (
                axum::http::StatusCode::NOT_FOUND,
                Json(json!({"error": error})),
            );
        }
        let model = f
            .reported
            .clone()
            .unwrap_or_else(|| body["model"].as_str().unwrap_or_default().to_string());
        (
            axum::http::StatusCode::OK,
            Json(json!({
                "model": model,
                "message": {"role": "assistant", "content": f.content},
                "done": true,
            })),
        )
    }
    let app = Router::new()
        .route("/api/tags", get(tags))
        .route("/api/chat", post(chat))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    url
}

#[test]
fn only_plain_web_addresses_are_used() {
    for url in [
        DEFAULT_URL,
        "https://ollama.local/",
        "http://192.168.1.20:11434",
    ] {
        assert!(validate_url(url).is_ok(), "{url}");
    }
    for url in [
        "localhost:11434",
        "file:///etc/passwd",
        "http://",
        "http://user:pass@host",
        "http://host/ path",
        "http://host\\x",
        "ftp://host",
    ] {
        assert!(validate_url(url).is_err(), "{url}");
    }
    assert!(validate_url(&format!("http://{}", "a".repeat(MAX_URL))).is_err());
    assert!(validate_model("llama3.2:3b").is_ok());
    assert!(validate_model("bad\nname").is_err());
}

#[tokio::test]
async fn lists_models_and_says_when_nothing_answers() {
    let url = serve(Fake {
        models: vec!["llama3.2:3b".into(), "qwen3:8b".into()],
        ..Fake::default()
    })
    .await;
    assert_eq!(models(&url).await.unwrap(), ["llama3.2:3b", "qwen3:8b"]);
    // A trailing slash is fine.
    assert_eq!(models(&format!("{url}/")).await.unwrap().len(), 2);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = models(&gone).await.unwrap_err();
    assert!(
        error.starts_with(&format!("Nothing answered at {gone}.")),
        "{error}"
    );
}

#[tokio::test]
async fn answers_in_the_shape_asked_for_from_the_model_asked_for() {
    let fake = Fake {
        content: "{\"answer\": \"Hi.\"}".into(),
        ..Fake::default()
    };
    let chats = fake.chats.clone();
    let url = serve(fake).await;
    let schema = json!({"type": "object"});
    let text = answer(
        &url,
        "llama3.2:3b",
        "Be Bit.",
        "Hello?",
        &schema,
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(text, "{\"answer\": \"Hi.\"}");
    let sent = chats.lock().unwrap()[0].clone();
    assert_eq!(sent["model"], "llama3.2:3b");
    assert_eq!(sent["stream"], false);
    assert_eq!(sent["format"], schema);
    assert_eq!(
        sent["messages"],
        json!([
            {"role": "system", "content": "Be Bit."},
            {"role": "user", "content": "Hello?"},
        ])
    );

    let url = serve(Fake {
        reported: Some("other".into()),
        ..Fake::default()
    })
    .await;
    let error = answer(&url, "llama3.2:3b", "", "", &schema, Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(
        error.contains("didn't confirm the model 'llama3.2:3b' (reported: other)"),
        "{error}"
    );

    let url = serve(Fake {
        error: Some("model \"gone\" not found, try pulling it first".into()),
        ..Fake::default()
    })
    .await;
    assert_eq!(
        answer(&url, "gone", "", "", &schema, Duration::from_secs(5))
            .await
            .unwrap_err(),
        "Ollama said: model \"gone\" not found, try pulling it first"
    );
}
