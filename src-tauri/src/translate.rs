//! Translating dictated text into another language.
//!
//! Runs as a step after cleanup in the dictation pipeline: the cleaned (English)
//! text is translated to the chosen target before it's pasted and stored. This
//! is a semantic rewrite — unlike [`crate::cleanup`], which preserves words —
//! so it's its own module. Two backends:
//!   - [`OllamaTranslator`]: local, private (mirrors `cleanup::ollama`).
//!   - [`AryaTranslator`]: cloud via the Arya API proxy
//!     (`POST /v1/anthropic/chat/completions`, OpenAI-shaped).
//!
//! Any failure returns `None` so the caller falls back to the untranslated
//! source — a dictation is never lost.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Where translation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TranslateProvider {
    /// Local Ollama — private, on-device.
    #[default]
    Local,
    /// Cloud via the Arya API (Claude).
    Cloud,
}

/// Default local model when none is otherwise configured. The user must have it
/// pulled in Ollama; if not, translation falls back to the source text.
pub const DEFAULT_LOCAL_MODEL: &str = "llama3.2";
/// Default cloud model id (Arya API catalog).
const DEFAULT_CLOUD_MODEL: &str = "anthropic:claude-sonnet-5";
const CLOUD_PROVIDER: &str = "anthropic";

pub trait Translator: Send + Sync {
    /// Translate `text` into `target` (a language name or code). Returns `None`
    /// on any failure so the caller can fall back to the source text.
    fn translate(&self, text: &str, target: &str) -> Option<String>;
}

/// Build the translator for a provider, or `None` if it isn't available (e.g.
/// cloud with no session token).
pub fn make_translator(
    provider: TranslateProvider,
    ollama_url: &str,
    ollama_model: &str,
) -> Option<Box<dyn Translator>> {
    match provider {
        TranslateProvider::Local => Some(Box::new(OllamaTranslator::new(
            ollama_url,
            ollama_model,
            Duration::from_secs(20),
        ))),
        TranslateProvider::Cloud => {
            AryaTranslator::from_env().map(|t| Box::new(t) as Box<dyn Translator>)
        }
    }
}

fn system_prompt(target: &str) -> String {
    format!(
        "You are a translation engine. Translate the user's text into {target}. \
         Output ONLY the translation — no quotes, no commentary, no notes. \
         Preserve meaning, tone, and line breaks."
    )
}

/// Local translation via Ollama's chat endpoint.
pub struct OllamaTranslator {
    base_url: String,
    model: String,
    timeout: Duration,
    client: reqwest::blocking::Client,
}

impl OllamaTranslator {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            timeout,
            client: reqwest::blocking::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}
#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

impl Translator for OllamaTranslator {
    fn translate(&self, text: &str, target: &str) -> Option<String> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "options": { "temperature": 0.2 },
            "messages": [
                { "role": "system", "content": system_prompt(target) },
                { "role": "user", "content": text },
            ],
        });
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .timeout(self.timeout)
            .json(&body)
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        let parsed: OllamaResponse = response.json().ok()?;
        non_empty(parsed.message.content)
    }
}

/// Cloud translation via the Arya API's OpenAI-shaped chat proxy.
pub struct AryaTranslator {
    base_url: String,
    token: String,
    model: String,
    timeout: Duration,
    client: reqwest::blocking::Client,
}

impl AryaTranslator {
    /// Build from the app's configured API URL and session token, or `None` when
    /// there is no token (so the caller falls back).
    pub fn from_env() -> Option<Self> {
        let token = crate::account::tokens::current_token()?;
        let base_url =
            std::env::var("ARYA_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8477".into());
        Some(Self {
            base_url,
            token,
            model: DEFAULT_CLOUD_MODEL.into(),
            timeout: Duration::from_secs(20),
            client: reqwest::blocking::Client::new(),
        })
    }
}

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

impl Translator for AryaTranslator {
    fn translate(&self, text: &str, target: &str) -> Option<String> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "messages": [
                { "role": "system", "content": system_prompt(target) },
                { "role": "user", "content": text },
            ],
        });
        let url = format!("{}/v1/{CLOUD_PROVIDER}/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .timeout(self.timeout)
            .header("authorization", format!("Bearer {}", self.token))
            .json(&body)
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        let parsed: OpenAiResponse = response.json().ok()?;
        non_empty(parsed.choices.into_iter().next()?.message.content)
    }
}

fn non_empty(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_unreachable_server_yields_none() {
        // Port 1 is never an Ollama server; a failed call must return None so
        // the pipeline falls back to the source text.
        let t = OllamaTranslator::new("http://127.0.0.1:1", "any", Duration::from_millis(200));
        assert_eq!(t.translate("hello", "German"), None);
    }

    #[test]
    fn system_prompt_names_the_target() {
        assert!(system_prompt("German").contains("German"));
        assert!(system_prompt("Spanish").contains("Translate"));
    }
}
