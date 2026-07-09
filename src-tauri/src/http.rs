//! Shared HTTP clients. Constructing a reqwest client builds a fresh connection
//! pool + TLS config, so the local-service callers (Ollama, the Arya proxy)
//! reuse one pooled blocking client instead of paying that on every request.

use std::sync::OnceLock;
use std::time::Duration;

/// A process-wide blocking HTTP client with a pooled connection. Callers set
/// their own per-request timeouts.
pub fn blocking_client() -> reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::blocking::Client::new).clone()
}

#[derive(serde::Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatMessage,
}
#[derive(serde::Deserialize)]
struct OllamaChatMessage {
    content: String,
}

/// One-shot Ollama chat completion (`/api/chat`, non-streaming). Returns the
/// assistant message content trimmed, or `None` on any failure — server down,
/// model missing, timeout, or a malformed/empty response — so the note
/// generator, dictation cleaner, and translator all fall back gracefully off
/// one shared client instead of three copies of this boilerplate.
pub fn ollama_chat(
    client: &reqwest::blocking::Client,
    base_url: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> Option<String> {
    let body = serde_json::json!({
        "model": model,
        "stream": false,
        // Hybrid-reasoning models (Qwen3, this app's Gemma default, etc.) burn
        // thousands of hidden "thinking" tokens by default even on trivial
        // instructions, routinely exceeding callers' 20-45s timeouts. All of
        // this app's tasks are short deterministic rewrites, not problems that
        // benefit from chain-of-thought, so thinking is off unconditionally.
        "think": false,
        "options": { "temperature": temperature },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    let response = client
        .post(format!("{base_url}/api/chat"))
        .timeout(timeout)
        .json(&body)
        .send()
        .ok()?
        .error_for_status()
        .ok()?;
    let parsed: OllamaChatResponse = response.json().ok()?;
    let content = parsed.message.content.trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}
