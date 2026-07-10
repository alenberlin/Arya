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
    ollama_chat_ex(
        client,
        base_url,
        model,
        system,
        user,
        temperature,
        timeout,
        None,
        None,
    )
}

/// Like [`ollama_chat`], with two extras for structured, large-input tasks:
///
/// - `num_ctx`: the context window in tokens. Ollama's default is small, and
///   when a prompt overflows it llama-server drops the middle (keeping only a
///   few leading tokens), which can silently discard the instruction. Setting it
///   generously keeps a big brain dump intact.
/// - `format`: a JSON Schema value that grammar-constrains the model to emit
///   valid, correctly-shaped JSON. Local models routinely produce *almost*-valid
///   JSON on longer outputs (trailing commas, unquoted keys, unescaped control
///   chars); constraining the decoder eliminates the parse failures entirely and
///   is typically faster than free-form generation.
#[allow(clippy::too_many_arguments)]
pub fn ollama_chat_ex(
    client: &reqwest::blocking::Client,
    base_url: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
    num_ctx: Option<u32>,
    format: Option<serde_json::Value>,
) -> Option<String> {
    let mut options = serde_json::json!({ "temperature": temperature });
    if let Some(n) = num_ctx {
        options["num_ctx"] = serde_json::Value::from(n);
    }
    let mut body = serde_json::json!({
        "model": model,
        "stream": false,
        // Hybrid-reasoning models (Qwen3, this app's Gemma default, etc.) burn
        // thousands of hidden "thinking" tokens by default even on trivial
        // instructions, routinely exceeding callers' 20-45s timeouts. All of
        // this app's tasks are short deterministic rewrites, not problems that
        // benefit from chain-of-thought, so thinking is off unconditionally.
        "think": false,
        "options": options,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    if let Some(fmt) = format {
        body["format"] = fmt;
    }
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
