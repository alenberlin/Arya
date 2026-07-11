//! Direct cloud LLM calls using the user's own API key (see [`crate::keys`]).
//!
//! This is the open-source / local build's cloud path: there is no Arya proxy
//! in front, so we talk to the provider's public API directly with the key the
//! user pasted into Account → Cloud API keys. Hosted builds (with the metering
//! proxy configured) keep using the proxy path in the callers; this module is
//! only reached when [`crate::account::tokens::proxy_configured`] is false.
//!
//! One entry point — [`chat`] — takes a provider-qualified model id
//! (`anthropic:claude-sonnet-5`, `openai:gpt-5.2`) so the caller doesn't have
//! to know which vendor it's talking to.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::keys::{self, Provider};

/// Anthropic caps `max_tokens` per model; this comfortably covers the
/// note/dictation transforms that call in here without risking truncation.
const MAX_TOKENS: u32 = 8192;

/// Run a single-turn chat completion against the cloud provider named in
/// `qualified_model`. Returns the assistant's text, or a user-facing `Err`
/// (missing key, network failure, empty response) — the caller always keeps
/// the user's source text, so a failure here is never destructive.
pub fn chat(
    qualified_model: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> Result<String, String> {
    let (provider, model) = resolve(qualified_model)?;
    let key = keys::get(provider)
        .ok_or_else(|| format!("no {} API key set — add one in Account.", provider.id()))?;
    let client = crate::http::blocking_client();
    match provider {
        Provider::OpenAi => openai_chat(&client, &key, &model, system, user, temperature, timeout),
        Provider::Anthropic => {
            anthropic_chat(&client, &key, &model, system, user, temperature, timeout)
        }
    }
}

/// A sensible default cloud model for whichever key the user has (Anthropic
/// preferred). `None` when no key is set. Used by the dictation/notes cloud
/// features, which don't expose a model picker of their own.
pub fn default_model() -> Option<String> {
    if keys::get(Provider::Anthropic).is_some() {
        Some("anthropic:claude-sonnet-5".to_string())
    } else if keys::get(Provider::OpenAi).is_some() {
        Some("openai:gpt-5.2".to_string())
    } else {
        None
    }
}

/// Resolve a model string to `(provider, bare_model)`. An explicit
/// `provider:model` prefix wins; a bare id falls back to whichever key the user
/// has (Anthropic preferred), so a caller with a hard-coded default still works.
fn resolve(qualified: &str) -> Result<(Provider, String), String> {
    if let Some((prefix, model)) = qualified.split_once(':') {
        if let Some(provider) = Provider::parse(prefix) {
            return Ok((provider, model.to_string()));
        }
    }
    if keys::get(Provider::Anthropic).is_some() {
        return Ok((Provider::Anthropic, qualified.to_string()));
    }
    if keys::get(Provider::OpenAi).is_some() {
        return Ok((Provider::OpenAi, qualified.to_string()));
    }
    Err("no cloud API key set — add one in Account.".to_string())
}

// ---- OpenAI (chat/completions) --------------------------------------------

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}

fn openai_chat(
    client: &reqwest::blocking::Client,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "temperature": temperature,
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    let parsed: OpenAiResponse = client
        .post("https://api.openai.com/v1/chat/completions")
        .timeout(timeout)
        .header("authorization", format!("Bearer {key}"))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(map_status_err)?
        .json()
        .map_err(|e| e.to_string())?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "the model returned nothing".to_string())
}

// ---- Anthropic (messages) -------------------------------------------------

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicBlock>,
}
#[derive(Deserialize)]
struct AnthropicBlock {
    #[serde(default)]
    text: String,
}

fn anthropic_chat(
    client: &reqwest::blocking::Client,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "temperature": temperature,
        "system": system,
        "messages": [ { "role": "user", "content": user } ],
    });
    let parsed: AnthropicResponse = client
        .post("https://api.anthropic.com/v1/messages")
        .timeout(timeout)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(map_status_err)?
        .json()
        .map_err(|e| e.to_string())?;
    let text = parsed
        .content
        .into_iter()
        .map(|b| b.text)
        .collect::<String>()
        .trim()
        .to_string();
    if text.is_empty() {
        Err("the model returned nothing".to_string())
    } else {
        Ok(text)
    }
}

/// Turn an HTTP error status into a message that points at the likely cause
/// (a bad or unfunded key is by far the most common), without echoing the key.
fn map_status_err(e: reqwest::Error) -> String {
    match e.status().map(|s| s.as_u16()) {
        Some(401) => "the cloud provider rejected the API key (401) — check it in Account.".into(),
        Some(429) => "the cloud provider is rate-limiting or the key is out of quota (429).".into(),
        Some(code) => format!("the cloud provider returned an error ({code})."),
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_explicit_prefix_wins() {
        assert!(matches!(
            resolve("openai:gpt-5.2"),
            Ok((Provider::OpenAi, m)) if m == "gpt-5.2"
        ));
        assert!(matches!(
            resolve("anthropic:claude-sonnet-5"),
            Ok((Provider::Anthropic, m)) if m == "claude-sonnet-5"
        ));
    }
}
