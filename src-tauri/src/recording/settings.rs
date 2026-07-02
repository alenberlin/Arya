//! Note-generation settings (local model choice), persisted as JSON.

use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GenerationSettings {
    /// Ollama model used for note generation; None means the deterministic
    /// fallback formatter.
    pub model: Option<String>,
    pub ollama_url: String,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            model: None,
            ollama_url: "http://127.0.0.1:11434".into(),
        }
    }
}

pub fn load_generation_settings(app: &tauri::AppHandle) -> GenerationSettings {
    let Ok(dir) = app.path().app_config_dir() else {
        return GenerationSettings::default();
    };
    std::fs::read_to_string(dir.join("generation-settings.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_generation_settings(
    app: &tauri::AppHandle,
    settings: &GenerationSettings,
) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("generation-settings.json"),
        serde_json::to_string_pretty(settings).expect("settings serialize"),
    )
    .map_err(|e| e.to_string())
}
