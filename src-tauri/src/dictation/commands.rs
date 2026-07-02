//! Tauri commands for dictation settings, history, and the dictionary.

use std::sync::Arc;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

use super::service::DictationService;
use super::settings::{self, DictationSettings};
use crate::paste;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub raw_text: String,
    pub clean_text: String,
    pub app_bundle_id: Option<String>,
    pub duration_ms: i64,
    pub asr_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryItem {
    pub id: String,
    pub pattern: String,
    pub replacement: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStatus {
    pub accessibility_trusted: bool,
    pub recording: bool,
    pub input_devices: Vec<String>,
}

#[tauri::command]
pub fn get_dictation_settings(service: State<'_, Arc<DictationService>>) -> DictationSettings {
    service.settings()
}

#[tauri::command]
pub fn set_dictation_settings(
    app: AppHandle,
    service: State<'_, Arc<DictationService>>,
    settings: DictationSettings,
) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let previous = service.settings();
    settings::save(&config_dir, &settings).map_err(|e| e.to_string())?;
    service.update_settings(settings.clone());
    if previous.shortcut != settings.shortcut || previous.mode != settings.mode {
        super::hotkey::register(&app, &settings).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn dictation_status(service: State<'_, Arc<DictationService>>) -> DictationStatus {
    DictationStatus {
        accessibility_trusted: paste::accessibility_trusted(),
        recording: service.is_recording(),
        input_devices: crate::audio::input_device_names(),
    }
}

#[tauri::command]
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    paste::prompt_accessibility();
}

#[tauri::command]
pub async fn list_dictation_history(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<HistoryItem>, String> {
    sqlx::query_as::<_, HistoryItem>(
        "SELECT id, raw_text, clean_text, app_bundle_id, duration_ms, asr_ms, created_at
         FROM dictation_history ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_dictation_history_item(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM dictation_history WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_dictionary_entries(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<DictionaryItem>, String> {
    sqlx::query_as::<_, DictionaryItem>(
        "SELECT id, pattern, replacement FROM dictionary_entries ORDER BY pattern",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_dictionary_entry(
    pool: State<'_, SqlitePool>,
    pattern: String,
    replacement: String,
) -> Result<DictionaryItem, String> {
    let pattern = pattern.trim().to_string();
    let replacement = replacement.trim().to_string();
    if pattern.is_empty() || replacement.is_empty() {
        return Err("pattern and replacement are required".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO dictionary_entries (id, pattern, replacement, created_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(&id)
    .bind(&pattern)
    .bind(&replacement)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(DictionaryItem {
        id,
        pattern,
        replacement,
    })
}

/// Dev-only: drives one full dictation cycle without the hotkey, so the
/// pipeline can be exercised end to end in automated runtime checks.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_run_dictation(
    app: AppHandle,
    service: State<'_, Arc<DictationService>>,
    pool: State<'_, SqlitePool>,
    duration_ms: u64,
) {
    let service = service.inner().clone();
    let pool = pool.inner().clone();
    std::thread::spawn(move || {
        service.begin(&app);
        std::thread::sleep(std::time::Duration::from_millis(duration_ms));
        service.finish(&app, pool);
    });
}

#[tauri::command]
pub async fn delete_dictionary_entry(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM dictionary_entries WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
