//! The single model-routing entry point for the shell's AI rewrites.
//!
//! Callers pass a provider-qualified model id — `ollama:<model>` for the local
//! Ollama server (private, offline), or `openai:<model>` / `anthropic:<model>`
//! for the user's own cloud key — and [`chat`] routes to the right backend.
//! This replaces the per-caller "local vs cloud" branching that was duplicated
//! across the transform, translate, and dictation paths.

use std::time::Duration;

/// Runs a single-turn completion against whichever provider the
/// provider-qualified model names. `ollama_url` is only consulted for the local
/// path.
pub fn chat(
    qualified_model: &str,
    ollama_url: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
) -> Result<String, String> {
    if let Some(model) = qualified_model.strip_prefix("ollama:") {
        let client = crate::http::blocking_client();
        crate::http::ollama_chat(
            &client,
            ollama_url,
            model,
            system,
            user,
            temperature,
            timeout,
        )
        .ok_or_else(|| "the local model did not respond — is Ollama running?".to_string())
    } else {
        crate::cloud::chat(qualified_model, system, user, temperature, timeout)
    }
}

/// Like [`chat`], but passes `num_ctx` and a JSON-schema `format` through to
/// the local Ollama backend. Only the `ollama:` prefix is supported — these
/// extended options are Ollama-specific, so a cloud model is an explicit error.
#[allow(clippy::too_many_arguments)]
pub fn chat_ex(
    qualified_model: &str,
    ollama_url: &str,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
    num_ctx: Option<u32>,
    format: Option<serde_json::Value>,
) -> Result<String, String> {
    let Some(model) = qualified_model.strip_prefix("ollama:") else {
        return Err(
            "extended options (num_ctx/format) are only supported for local Ollama models".into(),
        );
    };
    let client = crate::http::blocking_client();
    crate::http::ollama_chat_ex(
        &client,
        ollama_url,
        model,
        system,
        user,
        temperature,
        timeout,
        num_ctx,
        format,
    )
    .ok_or_else(|| "the local model did not respond — is Ollama running?".to_string())
}

/// The Ollama base URL used by features that don't carry a per-user setting of
/// their own: an `OLLAMA_URL` override, or the default loopback server.
pub fn default_ollama_url() -> String {
    std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into())
}

/// A sensible default cloud model for whichever key the user has (Anthropic
/// preferred). `None` when no key is set.
pub fn default_model() -> Option<String> {
    crate::cloud::default_model()
}
