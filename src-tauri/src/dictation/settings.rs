//! Dictation settings, persisted as JSON in the app config directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cleanup::{DictationStyle, Polish};
use crate::translate::TranslateProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationMode {
    #[default]
    PushToTalk,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DictationSettings {
    /// Hotkey in tauri-plugin-global-shortcut syntax, e.g. "ctrl+alt+d".
    pub shortcut: String,
    pub mode: ActivationMode,
    pub style: DictationStyle,
    /// How much cleanup to apply (verbatim / mechanical / local-LLM).
    pub polish: Polish,
    /// ISO 639-1 hint for ASR; None lets the model detect.
    pub language: Option<String>,
    /// Input device name; None uses the system default.
    pub microphone: Option<String>,
    /// Speech model id from the catalog.
    pub speech_model: String,
    /// Use the streaming (online) ASR engine to drive the pill's live preview
    /// instead of re-transcribing with whisper. Opt-in; downloads a streaming
    /// model on first use. The final inserted text still comes from whisper.
    pub streaming: bool,
    /// Ollama model for cleanup; None means mechanical rules only.
    pub cleanup_model: Option<String>,
    pub ollama_url: String,
    /// Target language for translation (e.g. "German"). None = off.
    pub translate: Option<String>,
    /// Which engine performs translation.
    pub translate_provider: TranslateProvider,
    /// Ollama model used for local translation; None falls back to the cleanup
    /// model or a built-in default.
    pub translate_model: Option<String>,
}

/// Sentinel `shortcut` value selecting the low-level right-Shift trigger
/// (hold = push-to-talk, double-tap = hands-free) instead of a global-shortcut
/// accelerator.
pub const RIGHT_SHIFT_TRIGGER: &str = "right shift";

impl DictationSettings {
    /// Whether the right-Shift event tap (rather than a global shortcut) drives
    /// dictation.
    pub fn uses_right_shift(&self) -> bool {
        let s = self.shortcut.trim();
        s.eq_ignore_ascii_case(RIGHT_SHIFT_TRIGGER) || s.eq_ignore_ascii_case("right-shift")
    }
}

impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            shortcut: RIGHT_SHIFT_TRIGGER.into(),
            mode: ActivationMode::PushToTalk,
            style: DictationStyle::Standard,
            polish: Polish::Clean,
            language: None,
            microphone: None,
            speech_model: "whisper-base.en".into(),
            streaming: false,
            cleanup_model: None,
            ollama_url: "http://127.0.0.1:11434".into(),
            translate: None,
            translate_provider: TranslateProvider::Local,
            translate_model: None,
        }
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("dictation-settings.json")
}

pub fn load(config_dir: &Path) -> DictationSettings {
    let path = settings_path(config_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, settings: &DictationSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let raw = serde_json::to_string_pretty(settings).expect("settings serialize");
    std::fs::write(settings_path(config_dir), raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_survives_unknown_fields() {
        let dir = std::env::temp_dir().join(format!("arya-settings-{}", uuid::Uuid::new_v4()));
        let settings = DictationSettings {
            shortcut: "ctrl+alt+space".into(),
            mode: ActivationMode::Toggle,
            polish: Polish::Polished,
            ..Default::default()
        };
        save(&dir, &settings).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.shortcut, "ctrl+alt+space");
        assert_eq!(loaded.mode, ActivationMode::Toggle);
        assert_eq!(loaded.polish, Polish::Polished);

        // Forward compatibility: unknown fields are ignored, missing fields
        // take defaults.
        std::fs::write(
            settings_path(&dir),
            r#"{"shortcut":"f19","futureField":true}"#,
        )
        .unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.shortcut, "f19");
        assert_eq!(loaded.speech_model, "whisper-base.en");
        // A settings file predating `polish` loads as the default (Clean).
        assert_eq!(loaded.polish, Polish::Clean);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("arya-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(settings_path(&dir), "{not json").unwrap();
        assert_eq!(load(&dir).shortcut, RIGHT_SHIFT_TRIGGER);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_right_shift_trigger() {
        let mut s = DictationSettings::default();
        assert!(s.uses_right_shift(), "right shift is the default");
        s.shortcut = "Right-Shift".into();
        assert!(s.uses_right_shift());
        s.shortcut = "ctrl+alt+d".into();
        assert!(!s.uses_right_shift());
    }
}
