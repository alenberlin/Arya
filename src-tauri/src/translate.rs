//! Translating dictated text into another language.
//!
//! Runs as a step after cleanup in the dictation pipeline: the cleaned (English)
//! text is translated to the chosen target before it's pasted and stored. This
//! is a semantic rewrite — unlike [`crate::cleanup`], which preserves words —
//! so it's its own module. Two backends:
//!   - [`OllamaTranslator`]: local, private (mirrors `cleanup::ollama`).
//!   - [`AryaTranslator`]: cloud via the user's own provider key.
//!
//! Any failure returns `None` so the caller falls back to the untranslated
//! source — a dictation is never lost.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Where translation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum TranslateProvider {
    /// Local Ollama — private, on-device.
    #[default]
    Local,
    /// Cloud via the user's own provider key.
    Cloud,
}

/// Default local model when none is otherwise configured. The user must have it
/// pulled in Ollama; if not, translation falls back to the source text.
pub const DEFAULT_LOCAL_MODEL: &str = "iaprofesseur/SuperGemma4-26b-uncensored-Q4:latest";

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
        TranslateProvider::Cloud => Some(Box::new(AryaTranslator::new(Duration::from_secs(20)))),
    }
}

fn system_prompt(target: &str) -> String {
    format!(
        "You are a translation engine. Translate the user's text into {target}. \
         Output ONLY the translation — no quotes, no commentary, no notes. \
         Preserve meaning, tone, and line breaks."
    )
}

/// Local translation via the unified provider router (Ollama).
pub struct OllamaTranslator {
    base_url: String,
    model: String,
    timeout: Duration,
}

impl OllamaTranslator {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            timeout,
        }
    }
}

impl Translator for OllamaTranslator {
    fn translate(&self, text: &str, target: &str) -> Option<String> {
        crate::provider::chat(
            &format!("ollama:{}", self.model),
            &self.base_url,
            &system_prompt(target),
            text,
            0.2,
            self.timeout,
        )
        .ok()
        .and_then(non_empty)
    }
}

/// Cloud translation via the provider directly with the user's own key.
pub struct AryaTranslator {
    timeout: Duration,
}

impl AryaTranslator {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Translator for AryaTranslator {
    fn translate(&self, text: &str, target: &str) -> Option<String> {
        let qualified = crate::provider::default_model()?;
        match crate::provider::chat(
            &qualified,
            &crate::provider::ollama_url(),
            &system_prompt(target),
            text,
            0.2,
            self.timeout,
        ) {
            Ok(content) => non_empty(content),
            Err(e) => {
                // The source text is still kept (never lost), but surface the
                // reason instead of failing completely silently.
                warn!(error = %e, "arya cloud translation failed, falling back to source");
                None
            }
        }
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
