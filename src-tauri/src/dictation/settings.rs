//! Dictation settings, persisted as JSON in the app config directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cleanup::DictationStyle;

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
    /// ISO 639-1 hint for ASR; None lets the model detect.
    pub language: Option<String>,
    /// Input device name; None uses the system default.
    pub microphone: Option<String>,
    /// Speech model id from the catalog.
    pub speech_model: String,
    /// Ollama model for cleanup; None means mechanical rules only.
    pub cleanup_model: Option<String>,
    pub ollama_url: String,
}

impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            shortcut: "ctrl+alt+d".into(),
            mode: ActivationMode::PushToTalk,
            style: DictationStyle::Standard,
            language: None,
            microphone: None,
            speech_model: "whisper-base.en".into(),
            cleanup_model: None,
            ollama_url: "http://127.0.0.1:11434".into(),
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
            ..Default::default()
        };
        save(&dir, &settings).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.shortcut, "ctrl+alt+space");
        assert_eq!(loaded.mode, ActivationMode::Toggle);

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
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("arya-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(settings_path(&dir), "{not json").unwrap();
        assert_eq!(load(&dir).shortcut, "ctrl+alt+d");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
