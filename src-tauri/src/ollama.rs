//! Ollama, on this computer or another one on your network, answering Bit's
//! questions through its HTTP API. Orbit only lists its models and sends it
//! one question at a time, with what Orbit knows; it never pulls or changes
//! models.
use serde_json::{json, Value};
use std::time::Duration;

/// Where Ollama listens unless you say otherwise.
pub const DEFAULT_URL: &str = "http://localhost:11434";
/// The longest address, in characters.
pub const MAX_URL: usize = 200;
/// The longest model name, in characters.
pub const MAX_MODEL: usize = 200;
/// How long listing the models may take.
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Checks an Ollama address: `http://` or `https://`, a host, and nothing
/// that could hide another request.
pub fn validate_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or(
            "Ollama's address starts with http:// or https://, such as http://localhost:11434.",
        )?;
    if url.chars().count() > MAX_URL {
        return Err(format!("Keep Ollama's address to {MAX_URL} characters."));
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty()
        || host.contains('@')
        || url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err(format!(
            "'{url}' isn't an address Orbit can use for Ollama."
        ));
    }
    Ok(())
}

/// Checks a model name Ollama could have.
pub fn validate_model(model: &str) -> Result<(), String> {
    if model.chars().count() > MAX_MODEL || model.chars().any(char::is_control) {
        return Err(format!("'{model}' isn't a model name Ollama could have."));
    }
    Ok(())
}

/// Ollama is on this computer or your network, so it is reached directly,
/// never through a proxy.
fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

fn endpoint(url: &str, path: &str) -> String {
    format!("{}{path}", url.trim_end_matches('/'))
}

/// Why a request to Ollama at `url` failed, in words.
fn unreachable(url: &str, e: reqwest::Error) -> String {
    if e.is_timeout() {
        format!("Ollama at {url} took too long to answer.")
    } else if e.is_connect() {
        format!("Nothing answered at {url}. Start Ollama there, or check its address in Settings.")
    } else {
        format!("Orbit couldn't reach Ollama at {url}: {e}")
    }
}

/// Ollama's error message, or its status.
async fn failed(response: reqwest::Response) -> String {
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    match body["error"].as_str() {
        Some(error) => format!("Ollama said: {}", error.trim()),
        None => format!("Ollama answered with {status}."),
    }
}

/// The models Ollama at `url` has, by name.
pub async fn models(url: &str) -> Result<Vec<String>, String> {
    let response = client(CHECK_TIMEOUT)?
        .get(endpoint(url, "/api/tags"))
        .send()
        .await
        .map_err(|e| unreachable(url, e))?;
    if !response.status().is_success() {
        return Err(failed(response).await);
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| format!("Something answered at {url}, but not as Ollama does."))?;
    let models = body["models"].as_array().ok_or(format!(
        "Something answered at {url}, but not as Ollama does."
    ))?;
    Ok(models
        .iter()
        .filter_map(|m| m["name"].as_str().or(m["model"].as_str()))
        .filter(|name| validate_model(name).is_ok() && !name.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Ollama's answer to `prompt` from `model`, as JSON text in the shape of
/// `schema`. Ollama must say the answer came from `model`.
pub async fn answer(
    url: &str,
    model: &str,
    instructions: &str,
    prompt: &str,
    schema: &Value,
    timeout: Duration,
) -> Result<String, String> {
    let response = client(timeout)?
        .post(endpoint(url, "/api/chat"))
        .json(&json!({
            "model": model,
            "messages": [
                {"role": "system", "content": instructions},
                {"role": "user", "content": prompt},
            ],
            "stream": false,
            "format": schema,
        }))
        .send()
        .await
        .map_err(|e| unreachable(url, e))?;
    if !response.status().is_success() {
        return Err(failed(response).await);
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| "Ollama's answer wasn't in the shape asked for.".to_string())?;
    let reported = body["model"].as_str().unwrap_or_default();
    if reported != model {
        return Err(format!(
            "Ollama didn't confirm the model '{model}' (reported: {}), so I didn't use its answer.",
            if reported.is_empty() {
                "none"
            } else {
                reported
            }
        ));
    }
    body["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Ollama's answer wasn't in the shape asked for.".into())
}

#[cfg(test)]
#[path = "ollama_tests.rs"]
pub(crate) mod tests;
