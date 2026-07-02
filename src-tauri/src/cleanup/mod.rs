//! Dictation text cleanup.
//!
//! Contract (mirrors the PRD): cleanup preserves the speaker's words. It may
//! fix casing, punctuation, fillers, and apply dictionary replacements, but
//! never summarizes or rewrites. Two backends:
//!   - [`MechanicalCleaner`]: deterministic rules, always available, offline.
//!   - [`OllamaCleaner`]: local LLM polish; falls back to mechanical output
//!     on any error so dictation never blocks on a model.

pub mod mechanical;
pub mod ollama;

use serde::{Deserialize, Serialize};

/// Writing style applied during cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DictationStyle {
    #[default]
    Standard,
    CasualLowercase,
    Formal,
}

/// Where the text is going, so layout can adapt (email greeting/sign-off
/// spacing, for example). Detected from the frontmost app at paste time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TargetContext {
    #[default]
    Generic,
    Email,
}

/// A user dictionary entry: replace `pattern` (case-insensitive whole word)
/// with `replacement`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub pattern: String,
    pub replacement: String,
}

#[derive(Debug, Clone)]
pub struct CleanupRequest {
    pub raw: String,
    pub style: DictationStyle,
    pub context: TargetContext,
    pub dictionary: Vec<DictionaryEntry>,
}

pub trait TextCleaner: Send + Sync {
    fn clean(&self, request: &CleanupRequest) -> String;
}
