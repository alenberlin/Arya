//! Dictation text cleanup.
//!
//! Contract: Raw and Clean preserve the speaker's words (fix casing,
//! punctuation, fillers, and dictionary replacements; never rewrite). The
//! "Polished" level goes further — it rewrites for grammatical correctness in
//! the speaker's own language, while still never answering, summarizing, or
//! adding content. Two backends:
//!   - [`MechanicalCleaner`]: deterministic word-preserving rules (Raw/Clean),
//!     always available, offline.
//!   - [`OllamaCleaner`]: local-LLM grammatical rewrite for Polished; falls
//!     back to the mechanical output on any error so dictation never blocks.

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

/// How much cleanup a dictation gets. Orthogonal to [`DictationStyle`]: this
/// picks the *engine* (verbatim / deterministic / local-LLM), style picks the
/// *voice*. Surfaced on the dictation pill as Raw / Clean / Polished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Polish {
    /// Verbatim: dictionary replacements only, no filler/casing/punctuation
    /// edits. For code, terminals, or anywhere exact words matter.
    Raw,
    /// Deterministic mechanical cleanup — the fast, offline default.
    #[default]
    Clean,
    /// Local-LLM rewrite into grammatically correct writing in the speaker's
    /// language; falls back to [`Clean`](Polish::Clean) when no cleanup model
    /// is configured.
    Polished,
}

/// The interpersonal register of the Polished rewrite (F6). Orthogonal to
/// [`DictationStyle`] (casing/format) — this shapes *voice*, and only the
/// local-LLM Polished level uses it; Raw/Clean ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PolishedTone {
    #[default]
    Neutral,
    Polite,
    Friendly,
    Professional,
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
    /// The Polished rewrite's tone (F6); ignored by Raw/Clean.
    pub tone: PolishedTone,
    pub context: TargetContext,
    pub dictionary: Vec<DictionaryEntry>,
}

pub trait TextCleaner: Send + Sync {
    fn clean(&self, request: &CleanupRequest) -> String;
}
