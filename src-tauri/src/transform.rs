//! Applying a free-form instruction to text via a local (Ollama) or cloud LLM.
//!
//! The generalization of [`crate::translate`]: the inline `@node + instruction`
//! action (F15) and the "Sort" brain-dump reorganizer (F16) both run through
//! here. **Local Ollama is the default** (private, offline); cloud via the Arya
//! API is opt-in. The system prompt forbids inventing content, so a transform
//! reorganizes/rephrases/translates only what it is given — never fabricates.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::translate::{TranslateProvider, DEFAULT_LOCAL_MODEL};

/// The guard: apply only the instruction, output only the result, invent nothing.
const SYSTEM_PROMPT: &str = "You transform the user's text according to their \
    instruction. Apply ONLY the instruction. Output ONLY the resulting text — no \
    preamble, no surrounding quotes, no commentary. Never add facts that are not \
    present in the text; you may reorganize, rephrase, summarize, or translate \
    what is given.";

const CLOUD_PROVIDER: &str = "anthropic";
const DEFAULT_CLOUD_MODEL: &str = "claude-sonnet-5";

fn user_message(instruction: &str, text: &str) -> String {
    format!("Instruction: {instruction}\n\nText:\n{text}")
}

pub(crate) fn ollama_url() -> String {
    std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into())
}

/// Run the transform on a blocking thread. `Err` carries a user-facing reason;
/// the caller keeps the source text either way, so nothing is ever lost.
fn run(
    provider: TranslateProvider,
    model: Option<String>,
    text: &str,
    instruction: &str,
) -> Result<String, String> {
    let client = crate::http::blocking_client();
    let user = user_message(instruction, text);
    match provider {
        TranslateProvider::Local => {
            let model = model.unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string());
            run_local(&client, &ollama_url(), &model, &user)
        }
        TranslateProvider::Cloud => run_cloud(&client, model, &user),
    }
}

/// Local transform via Ollama's chat endpoint (extracted so it's testable with
/// an injected URL).
fn run_local(
    client: &reqwest::blocking::Client,
    url: &str,
    model: &str,
    user: &str,
) -> Result<String, String> {
    crate::http::ollama_chat(
        client,
        url,
        model,
        SYSTEM_PROMPT,
        user,
        0.3,
        Duration::from_secs(45),
    )
    .ok_or_else(|| "the local model did not respond — is Ollama running?".to_string())
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}
#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}
#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

/// Cloud transform via the Arya API's OpenAI-shaped proxy (same path the
/// dictation cloud translator uses). Requires a signed-in session token.
fn run_cloud(
    client: &reqwest::blocking::Client,
    model: Option<String>,
    user: &str,
) -> Result<String, String> {
    let token = crate::account::tokens::current_token()
        .ok_or_else(|| "cloud transform needs you to be signed in".to_string())?;
    let base_url = std::env::var("ARYA_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8477".into());
    let model = model.unwrap_or_else(|| DEFAULT_CLOUD_MODEL.to_string());
    // Send the bare model id; the proxy qualifies it from the URL provider.
    let bare_model = model
        .strip_prefix(&format!("{CLOUD_PROVIDER}:"))
        .unwrap_or(&model);
    let body = json!({
        "model": bare_model,
        "stream": false,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user },
        ],
    });
    let parsed: ChatResponse = client
        .post(format!("{base_url}/v1/{CLOUD_PROVIDER}/chat/completions"))
        .timeout(Duration::from_secs(45))
        .header("authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "the cloud model returned nothing".to_string())
}

/// Apply an instruction to `source_text` and return the result. Local by default
/// (private); pass `provider: "cloud"` to route through the Arya API.
#[tauri::command]
pub async fn ai_transform(
    source_text: String,
    instruction: String,
    provider: Option<TranslateProvider>,
    model: Option<String>,
) -> Result<String, String> {
    if source_text.trim().is_empty() {
        return Err("there is nothing to transform".into());
    }
    if instruction.trim().is_empty() {
        return Err("an instruction is required".into());
    }
    let provider = provider.unwrap_or_default();
    tokio::task::spawn_blocking(move || run(provider, model, &source_text, &instruction))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_includes_instruction_and_text() {
        let m = user_message("translate to German", "hello world");
        assert!(m.contains("translate to German"));
        assert!(m.contains("hello world"));
        assert!(m.starts_with("Instruction:"));
    }

    #[test]
    fn local_transform_errors_cleanly_when_ollama_is_down() {
        // Port 1 is never an Ollama server: the call must fail with a clear
        // message (not panic) so the caller keeps the source text.
        let client = crate::http::blocking_client();
        let out = run_local(&client, "http://127.0.0.1:1", "nope", "some text");
        assert!(out.is_err());
    }

    #[test]
    fn system_prompt_forbids_inventing() {
        assert!(SYSTEM_PROMPT.contains("Never add facts"));
    }
}
