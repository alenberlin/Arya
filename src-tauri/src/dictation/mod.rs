//! Dictation: hold a key, speak, release; cleaned text lands in the
//! foreground app.
//!
//! Pipeline: capture (worker thread, cpal) -> on-device ASR (whisper) ->
//! cleanup (Ollama if available, mechanical rules otherwise) -> paste.
//! Everything is offline-capable; no network is required anywhere.

pub mod capture_worker;
pub mod commands;
pub mod hotkey;
pub mod keytap;
pub mod panel;
pub mod profiles;
pub mod service;
pub mod settings;

use serde::Serialize;

/// UI-facing dictation lifecycle states, emitted as `dictation:state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DictationState {
    Idle,
    /// First-use model download in progress.
    PreparingModel,
    Recording,
    Processing,
    Pasting,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationEvent {
    pub state: DictationState,
    /// Present on Error; human-readable.
    pub message: Option<String>,
    /// Present when a dictation completed: the pasted text.
    pub text: Option<String>,
}

/// Minimum hold duration; shorter presses are grazes and abort silently.
pub const MIN_HOLD_MS: u128 = 160;
