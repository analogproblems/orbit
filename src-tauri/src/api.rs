//! OpenAI, Anthropic and Google answering Bit's questions with an API key
//! of yours. Keys live in your system's password store (Keychain on macOS,
//! Credential Manager on Windows, the Secret Service on Linux), never in
//! Orbit's settings or state. Orbit only lists a provider's models and sends
//! it one question at a time, with what Orbit knows, asking for an answer in
//! Bit's shape and nothing else: no tools.
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// How Orbit is known to your system's password store.
pub const SERVICE: &str = "dev.orbit.review";
/// The longest key, in characters.
pub const MAX_KEY: usize = 400;
/// How long listing the models may take.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// The longest answer Anthropic may write, thinking included.
const ANTHROPIC_MAX_TOKENS: u32 = 16000;

/// A provider Bit can use with an API key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Api {
    OpenAi,
    Anthropic,
    Google,
}
impl Api {
    pub const ALL: [Api; 3] = [Api::OpenAi, Api::Anthropic, Api::Google];
    /// Its name in settings: `buddy.answerer` and the commands that save a key.
    pub fn key(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }
    /// Its name in messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "the OpenAI API",
            Self::Anthropic => "the Anthropic API",
            Self::Google => "the Gemini API",
        }
    }
    /// Who makes it, in messages.
    pub fn company(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Google => "Google",
        }
    }
    /// Where it answers.
    pub fn base(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Google => "https://generativelanguage.googleapis.com",
        }
    }
    /// Its key's name in your password store.
    fn secret(self) -> String {
        format!("{}-api-key", self.key())
    }
}

/// Where API keys are kept.
pub trait Secrets: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&self, name: &str, value: &str) -> Result<(), String>;
    fn delete(&self, name: &str) -> Result<(), String>;
}

/// Your system's password store.
pub struct Keychain;
impl Keychain {
    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, name).map_err(store_error)
    }
}
impl Secrets for Keychain {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        match Self::entry(name)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(e)),
        }
    }
    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        Self::entry(name)?.set_password(value).map_err(store_error)
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(e)),
        }
    }
}
fn store_error(e: keyring::Error) -> String {
    format!("Orbit couldn't use your system's password store: {e}")
}

/// Where keys are kept and where each provider answers; tests use a fake
/// store and fake providers.
#[derive(Clone)]
pub struct Setup {
    pub secrets: Arc<dyn Secrets>,
    /// Addresses in `Api::ALL`'s order.
    pub bases: [String; 3],
}
impl Default for Setup {
    fn default() -> Self {
        Self {
            secrets: Arc::new(Keychain),
            bases: Api::ALL.map(|a| a.base().to_string()),
        }
    }
}
impl Setup {
    pub fn base(&self, api: Api) -> &str {
        &self.bases[Api::ALL.iter().position(|a| *a == api).unwrap_or(0)]
    }
    /// The saved key for `api`, if any. The password store may block, so
    /// it is read off the async threads.
    pub async fn key(&self, api: Api) -> Result<Option<String>, String> {
        let secrets = self.secrets.clone();
        tokio::task::spawn_blocking(move || secrets.get(&api.secret()))
            .await
            .map_err(|e| e.to_string())?
    }
    /// Saves `key` for `api`, or removes the saved one when it is None.
    pub async fn save(&self, api: Api, key: Option<String>) -> Result<(), String> {
        let secrets = self.secrets.clone();
        tokio::task::spawn_blocking(move || match key {
            Some(key) => secrets.set(&api.secret(), &key),
            None => secrets.delete(&api.secret()),
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Checks a key as you pasted it: one line, with no spaces.
pub fn validate_key(key: &str) -> Result<(), String> {
    if key.chars().count() < 8 || key.chars().count() > MAX_KEY {
        return Err("That doesn't look like an API key. Paste the whole key.".into());
    }
    if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("An API key has no spaces or line breaks. Paste just the key.".into());
    }
    Ok(())
}

/// Checks a model name a provider could have.
pub fn validate_model(model: &str) -> Result<(), String> {
    if model.chars().count() > 200 || model.chars().any(|c| c.is_control()) {
        return Err(format!(
            "'{model}' isn't a model name a provider could have."
        ));
    }
    Ok(())
}

/// Providers are reached through your proxy, if you have one; an address
/// on this computer is reached directly. Redirects aren't followed: the key
/// travels in a header reqwest would carry on to another site.
fn client(base: &str, timeout: Duration) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none());
    if base.starts_with("http://127.0.0.1:") || base.starts_with("http://localhost:") {
        builder = builder.no_proxy();
    }
    builder.build().map_err(|e| e.to_string())
}

/// A request to `api` with its key in the header it uses.
fn request(
    api: Api,
    base: &str,
    key: &str,
    get: bool,
    path: &str,
    timeout: Duration,
) -> Result<reqwest::RequestBuilder, String> {
    let client = client(base, timeout)?;
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let request = if get {
        client.get(url)
    } else {
        client.post(url)
    };
    Ok(match api {
        Api::OpenAi => request.bearer_auth(key),
        Api::Anthropic => request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        Api::Google => request.header("x-goog-api-key", key),
    })
}

/// Why a request to `api` failed, in words. The key is never part of it.
fn unreachable(api: Api, e: reqwest::Error) -> String {
    if e.is_timeout() {
        format!("{} took too long to answer.", api.company())
    } else {
        format!(
            "Orbit couldn't reach {}. Check this computer's internet connection.",
            api.label()
        )
    }
}

/// The provider's error message, or its status.
async fn failed(api: Api, response: reqwest::Response) -> String {
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        return format!(
            "{} didn't accept your key. Check it on {}'s website, or save a new one in Settings, under Buddy.",
            api.company(),
            api.company()
        );
    }
    let body: Value = response.json().await.unwrap_or_default();
    match body["error"]["message"].as_str() {
        Some(error) => format!("{} said: {}", api.company(), error.trim()),
        None => format!("{} answered with {status}.", api.company()),
    }
}

async fn send(api: Api, request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request.send().await.map_err(|e| unreachable(api, e))?;
    if !response.status().is_success() {
        return Err(failed(api, response).await);
    }
    response
        .json()
        .await
        .map_err(|_| format!("{}'s answer wasn't in the shape asked for.", api.company()))
}

/// The models `api` offers your key that can write Bit's answers, by name.
pub async fn models(api: Api, base: &str, key: &str) -> Result<Vec<String>, String> {
    let path = match api {
        Api::OpenAi => "/v1/models",
        Api::Anthropic => "/v1/models?limit=1000",
        Api::Google => "/v1beta/models?pageSize=1000",
    };
    let body = send(api, request(api, base, key, true, path, CHECK_TIMEOUT)?).await?;
    let names: Vec<String> = match api {
        Api::OpenAi => {
            let mut names: Vec<String> = list(&body["data"])
                .filter_map(|m| m["id"].as_str())
                .filter(|id| openai_answers_in_form(id))
                .map(str::to_owned)
                .collect();
            names.sort();
            names
        }
        // Newest first, as Anthropic lists them, leaving out any it says
        // can't answer in a set form.
        Api::Anthropic => list(&body["data"])
            .filter(|m| m["capabilities"]["structured_outputs"]["supported"] != false)
            .filter_map(|m| m["id"].as_str())
            .map(str::to_owned)
            .collect(),
        Api::Google => list(&body["models"])
            .filter(|m| {
                list(&m["supportedGenerationMethods"])
                    .any(|g| g.as_str() == Some("generateContent"))
            })
            .filter_map(|m| m["name"].as_str())
            .map(|name| name.strip_prefix("models/").unwrap_or(name))
            .filter(|name| gemini_answers_in_form(name))
            .map(str::to_owned)
            .collect(),
    };
    Ok(names
        .into_iter()
        .filter(|name| !name.is_empty() && validate_model(name).is_ok())
        .collect())
}
fn list(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}
/// Words in the names of models made for audio, images, search, embeddings
/// or other work than writing an answer.
const NOT_FOR_ANSWERS: [&str; 10] = [
    "audio",
    "realtime",
    "tts",
    "transcribe",
    "image",
    "search",
    "instruct",
    "embedding",
    "live",
    "moderation",
];
/// The leading number in `text`, if it starts with one.
fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}
/// Whether an OpenAI model can write Bit's answer through the Responses
/// API in a strict JSON schema: GPT-4o (after its first version), GPT-4.1,
/// GPT-5 and later, o1, and o3 and later. OpenAI doesn't list what each
/// model supports, so this goes by name; GPT-3.5, GPT-4 and GPT-4 Turbo,
/// o1-mini and o1-preview can't.
fn openai_answers_in_form(id: &str) -> bool {
    if NOT_FOR_ANSWERS.iter().any(|w| id.contains(w)) || id == "gpt-4o-2024-05-13" {
        return false;
    }
    if let Some(rest) = id.strip_prefix("gpt-") {
        return rest.starts_with("4o")
            || rest.starts_with("4.1")
            || leading_number(rest).is_some_and(|n| n >= 5);
    }
    match id.strip_prefix('o').and_then(|rest| {
        leading_number(rest).map(|n| (n, rest.contains("mini") || rest.contains("preview")))
    }) {
        Some((1, older)) => !older,
        Some((n, _)) => n >= 3,
        None => false,
    }
}
/// Whether a Google model can write Bit's answer in a JSON schema, with
/// instructions: Gemini 2 and later, not Gemma.
fn gemini_answers_in_form(name: &str) -> bool {
    name.strip_prefix("gemini-")
        .and_then(leading_number)
        .is_some_and(|n| n >= 2)
        && !NOT_FOR_ANSWERS.iter().any(|w| name.contains(w))
}

/// Whether a provider's `reported` model is `asked`: the same name, or a
/// dated version of it, as providers report for a model's short name.
pub fn same_model(asked: &str, reported: &str) -> bool {
    let reported = reported.strip_prefix("models/").unwrap_or(reported);
    reported == asked
        || reported.strip_prefix(asked).is_some_and(|rest| {
            rest.len() > 1
                && rest.starts_with('-')
                && rest[1..].chars().all(|c| c.is_ascii_digit() || c == '-')
        })
}

/// The provider's answer to `prompt` from `model`, as JSON text in the
/// shape of `schema`. It must say the answer came from `model`.
#[allow(clippy::too_many_arguments)]
pub async fn answer(
    api: Api,
    base: &str,
    key: &str,
    model: &str,
    instructions: &str,
    prompt: &str,
    schema: &Value,
    timeout: Duration,
) -> Result<String, String> {
    let (path, body) = match api {
        Api::OpenAi => (
            "/v1/responses".to_string(),
            json!({
                "model": model,
                "instructions": instructions,
                "input": prompt,
                "text": {"format": {
                    "type": "json_schema",
                    "name": "bit_answer",
                    "schema": schema,
                    "strict": true,
                }},
                "store": false,
            }),
        ),
        Api::Anthropic => (
            "/v1/messages".to_string(),
            json!({
                "model": model,
                "max_tokens": ANTHROPIC_MAX_TOKENS,
                "system": instructions,
                "messages": [{"role": "user", "content": prompt}],
                "output_config": {"format": {"type": "json_schema", "schema": schema}},
            }),
        ),
        Api::Google => (
            format!("/v1beta/models/{model}:generateContent"),
            json!({
                "systemInstruction": {"parts": [{"text": instructions}]},
                "contents": [{"role": "user", "parts": [{"text": prompt}]}],
                "generationConfig": {
                    "responseMimeType": "application/json",
                    "responseJsonSchema": schema,
                },
            }),
        ),
    };
    let body = send(
        api,
        request(api, base, key, false, &path, timeout)?.json(&body),
    )
    .await?;
    let reported = match api {
        Api::Google => body["modelVersion"].as_str(),
        _ => body["model"].as_str(),
    }
    .unwrap_or_default();
    if !same_model(model, reported) {
        return Err(format!(
            "{} didn't confirm the model '{model}' (reported: {}), so I didn't use its answer.",
            api.company(),
            if reported.is_empty() {
                "none"
            } else {
                reported
            }
        ));
    }
    let shape = || format!("{}'s answer wasn't in the shape asked for.", api.company());
    let company = api.company();
    match api {
        Api::OpenAi => {
            if body["status"] != "completed" {
                let why = body["incomplete_details"]["reason"]
                    .as_str()
                    .unwrap_or("it didn't finish");
                return Err(format!("{company} stopped before it finished ({why})."));
            }
            let content: Vec<&Value> = list(&body["output"])
                .filter(|o| o["type"] == "message")
                .flat_map(|o| list(&o["content"]))
                .collect();
            if let Some(refusal) = content.iter().find_map(|c| c["refusal"].as_str()) {
                return Err(format!("{company} declined to answer: {}", refusal.trim()));
            }
            content
                .iter()
                .find(|c| c["type"] == "output_text")
                .and_then(|c| c["text"].as_str())
                .map(str::to_owned)
                .ok_or_else(shape)
        }
        Api::Anthropic => {
            match body["stop_reason"].as_str() {
                Some("refusal") => return Err(format!("{company} declined to answer.")),
                Some("max_tokens") => {
                    return Err(format!("{company} ran out of room before it finished."))
                }
                _ => {}
            }
            list(&body["content"])
                .find(|c| c["type"] == "text")
                .and_then(|c| c["text"].as_str())
                .map(str::to_owned)
                .ok_or_else(shape)
        }
        Api::Google => {
            let Some(candidate) = list(&body["candidates"]).next() else {
                let why = body["promptFeedback"]["blockReason"]
                    .as_str()
                    .unwrap_or("no reason given");
                return Err(format!("{company} declined to answer ({why})."));
            };
            match candidate["finishReason"].as_str() {
                None | Some("STOP") => {}
                Some(why) => return Err(format!("{company} stopped before it finished ({why}).")),
            }
            let text: String = list(&candidate["content"]["parts"])
                .filter(|p| p["thought"] != true)
                .filter_map(|p| p["text"].as_str())
                .collect();
            if text.is_empty() {
                Err(shape())
            } else {
                Ok(text)
            }
        }
    }
}

#[cfg(test)]
#[path = "api_tests.rs"]
pub(crate) mod tests;
