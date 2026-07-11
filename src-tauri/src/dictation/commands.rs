//! Tauri commands for dictation settings, history, and the dictionary.

use std::sync::Arc;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager, State};

use super::profiles;
use super::service::DictationService;
use super::settings::{self, DictationSettings};
use crate::cleanup::Polish;
use crate::paste;

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub raw_text: String,
    pub clean_text: String,
    /// The translated text when translation was on; None otherwise.
    pub translated_text: Option<String>,
    pub target_lang: Option<String>,
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

/// Lists the models installed in the user's Ollama, so settings can offer a
/// translation-model picker. Best-effort: an empty list if Ollama isn't up.
#[tauri::command]
pub async fn list_ollama_models(
    service: State<'_, Arc<DictationService>>,
) -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct Tags {
        models: Vec<TagModel>,
    }
    #[derive(serde::Deserialize)]
    struct TagModel {
        name: String,
    }
    let url = format!("{}/api/tags", service.settings().ollama_url);
    let tags: Tags = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(tags.models.into_iter().map(|m| m.name).collect())
}

/// Reachability of the user's Ollama, for the setup UI's "Local models" card.
/// A fresh install with no Ollama gets `reachable: false` so the UI can show
/// install guidance instead of a silently empty model list.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub reachable: bool,
    pub model_count: usize,
    pub url: String,
}

/// Ping Ollama's `/api/tags`. Never errors — an unreachable Ollama is a normal
/// state (the user just hasn't installed it yet), reported as `reachable:false`.
#[tauri::command]
pub async fn ollama_status(
    service: State<'_, Arc<DictationService>>,
) -> Result<OllamaStatus, String> {
    #[derive(serde::Deserialize)]
    struct Tags {
        models: Vec<serde_json::Value>,
    }
    let url = service.settings().ollama_url;
    let endpoint = format!("{url}/api/tags");
    let reachable = reqwest::Client::new()
        .get(&endpoint)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success());
    match reachable {
        Some(resp) => {
            let model_count = resp
                .json::<Tags>()
                .await
                .map(|t| t.models.len())
                .unwrap_or(0);
            Ok(OllamaStatus {
                reachable: true,
                model_count,
                url,
            })
        }
        None => Ok(OllamaStatus {
            reachable: false,
            model_count: 0,
            url,
        }),
    }
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

/// An on-demand translation of a saved dictation (F8/M9).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DictationTranslation {
    pub id: String,
    pub dictation_id: String,
    pub lang: String,
    pub text: String,
    pub model: String,
    pub created_at: String,
}

/// Store (or replace) a dictation's translation for one language.
async fn upsert_dictation_translation(
    pool: &SqlitePool,
    dictation_id: &str,
    lang: &str,
    text: &str,
    model: &str,
) -> Result<DictationTranslation, sqlx::Error> {
    sqlx::query_as::<_, DictationTranslation>(
        "INSERT INTO dictation_translations (id, dictation_id, lang, text, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(dictation_id, lang)
             DO UPDATE SET text = excluded.text, model = excluded.model,
                           created_at = excluded.created_at
         RETURNING id, dictation_id, lang, text, model, created_at",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(dictation_id)
    .bind(lang)
    .bind(text)
    .bind(model)
    .fetch_one(pool)
    .await
}

async fn fetch_dictation_translations(
    pool: &SqlitePool,
    dictation_id: &str,
) -> Result<Vec<DictationTranslation>, sqlx::Error> {
    sqlx::query_as::<_, DictationTranslation>(
        "SELECT id, dictation_id, lang, text, model, created_at
         FROM dictation_translations WHERE dictation_id = ?1 ORDER BY created_at",
    )
    .bind(dictation_id)
    .fetch_all(pool)
    .await
}

/// Translate a saved dictation into `target_lang` and store it alongside the
/// original (non-destructive; one row per language). Uses the same local/cloud
/// translator as capture-time translation.
#[tauri::command]
pub async fn translate_dictation(
    pool: State<'_, SqlitePool>,
    service: State<'_, Arc<DictationService>>,
    id: String,
    target_lang: String,
) -> Result<DictationTranslation, String> {
    let source: String =
        sqlx::query_scalar("SELECT clean_text FROM dictation_history WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&*pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "dictation not found".to_string())?;
    if source.trim().is_empty() {
        return Err("this dictation has no text to translate".into());
    }
    let settings = service.settings();
    let provider = settings.translate_provider;
    let url = settings.ollama_url.clone();
    let model = settings
        .translate_model
        .clone()
        .or_else(|| settings.cleanup_model.clone())
        .unwrap_or_else(|| crate::translate::DEFAULT_LOCAL_MODEL.to_string());
    let target = target_lang.clone();
    let model_for_call = model.clone();
    let translated = tokio::task::spawn_blocking(move || {
        crate::translate::make_translator(provider, &url, &model_for_call)
            .and_then(|t| t.translate(&source, &target))
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "translation failed — is the model available?".to_string())?;
    upsert_dictation_translation(&pool, &id, &target_lang, &translated, &model)
        .await
        .map_err(|e| e.to_string())
}

/// List a dictation's on-demand translations (F8/M9).
#[tauri::command]
pub async fn list_dictation_translations(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<Vec<DictationTranslation>, String> {
    fetch_dictation_translations(&pool, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Every on-demand dictation translation, so the history renders in one load.
#[tauri::command]
pub async fn list_all_dictation_translations(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<DictationTranslation>, String> {
    sqlx::query_as::<_, DictationTranslation>(
        "SELECT id, dictation_id, lang, text, model, created_at
         FROM dictation_translations ORDER BY dictation_id, created_at",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_dictation_history(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<HistoryItem>, String> {
    sqlx::query_as::<_, HistoryItem>(
        "SELECT id, raw_text, clean_text, translated_text, target_lang, app_bundle_id,
                duration_ms, asr_ms, created_at
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
pub async fn clear_dictation_history(pool: State<'_, SqlitePool>) -> Result<(), String> {
    sqlx::query("DELETE FROM dictation_history")
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Turns a dictation into a meeting-minutes note: creates a note whose
/// transcript is the dictation text and whose body is generated by the same
/// on-device pipeline the meeting recorder uses. Returns the new note id.
#[tauri::command]
pub async fn convert_dictation_to_note(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<String, String> {
    use crate::recording::generate::{
        FallbackGenerator, GeneratedNote, NoteGenerator, OllamaGenerator, TurnText,
    };

    let clean: String =
        sqlx::query_scalar("SELECT clean_text FROM dictation_history WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&*pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "dictation not found".to_string())?;
    if clean.trim().is_empty() {
        return Err("this dictation has no text to convert".into());
    }

    let note = crate::notes::insert_note(&pool, "Meeting minutes")
        .await
        .map_err(|e| e.to_string())?;

    // Keep the dictation text as the note's transcript so it's reviewable.
    sqlx::query(
        "INSERT INTO transcript_turns (id, note_id, source, turn_index, start_ms, end_ms, text)
         VALUES (?1, ?2, 'dictation', 0, 0, 0, ?3)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&note.id)
    .bind(&clean)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    // Generate minutes off the async runtime — the model call blocks.
    let settings = crate::recording::settings::load_generation_settings(&app);
    let turns = vec![TurnText {
        start_ms: 0,
        end_ms: 0,
        source: "dictation".into(),
        speaker: None,
        text: clean,
    }];
    let generated: GeneratedNote = tauri::async_runtime::spawn_blocking(move || {
        let generator: Box<dyn NoteGenerator> = match settings.model.as_deref() {
            Some(model) => Box::new(OllamaGenerator::new(settings.ollama_url.clone(), model)),
            None => Box::new(FallbackGenerator),
        };
        generator.generate(&turns, "")
    })
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE notes SET title = ?2, body_md = ?3, processing_status = 'ready',
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(&note.id)
    .bind(&generated.title)
    .bind(&generated.body_md)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(note.id)
}

/// Converts a dictation straight into a plain note: its cleaned text becomes the
/// note body verbatim, titled from the first line — no meeting-minutes model
/// pass (fast, offline). The note then joins the connected brain like any other.
/// Returns the new note id.
#[tauri::command]
pub async fn convert_dictation_to_plain_note(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<String, String> {
    let clean: String =
        sqlx::query_scalar("SELECT clean_text FROM dictation_history WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&*pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "dictation not found".to_string())?;
    if clean.trim().is_empty() {
        return Err("this dictation has no text to convert".into());
    }

    let title = crate::notes::title_from_text(&clean);
    let note = crate::notes::insert_note(&pool, &title)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE notes SET body_md = ?2, processing_status = 'ready',
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(&note.id)
    .bind(&clean)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(note.id)
}

/// Stops a hands-free dictation (invoked from the pill's Stop button) and
/// resets the right-Shift gesture state machine.
#[tauri::command]
pub fn dictation_stop(
    app: AppHandle,
    service: State<'_, Arc<DictationService>>,
    pool: State<'_, SqlitePool>,
    tap_state: State<'_, Arc<std::sync::Mutex<super::keytap::TapState>>>,
) {
    let service = service.inner().clone();
    service.finish(&app, pool.inner().clone());
    if let Ok(mut s) = tap_state.lock() {
        s.reset();
    }
    let _ = app.emit("dictation:hands-free", false);
}

/// Cancels the current dictation (the pill's ✕): discards the capture without
/// transcribing or pasting, and resets the right-Shift gesture state machine.
#[tauri::command]
pub fn dictation_cancel(
    app: AppHandle,
    service: State<'_, Arc<DictationService>>,
    tap_state: State<'_, Arc<std::sync::Mutex<super::keytap::TapState>>>,
) {
    service.abort_recording(&app);
    if let Ok(mut s) = tap_state.lock() {
        s.reset();
    }
    let _ = app.emit("dictation:hands-free", false);
}

/// Sets a one-off polish level for the current dictation (the pill's polish
/// chip). Not persisted; consumed by the next transcription.
#[tauri::command]
pub fn dictation_set_session_polish(service: State<'_, Arc<DictationService>>, polish: Polish) {
    service.set_session_polish(polish);
}

/// Pins the given polish (with the current tone) as the default for the app the
/// current dictation is targeting.
#[tauri::command]
pub fn dictation_pin_app(
    app: AppHandle,
    service: State<'_, Arc<DictationService>>,
    polish: Polish,
) -> Result<(), String> {
    if let Some(overrides) = service.pin_current_app(polish) {
        let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
        profiles::save(&config_dir, &overrides).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Removes the pin for the app the current dictation is targeting.
#[tauri::command]
pub fn dictation_unpin_app(
    app: AppHandle,
    service: State<'_, Arc<DictationService>>,
) -> Result<(), String> {
    if let Some(overrides) = service.unpin_current_app() {
        let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
        profiles::save(&config_dir, &overrides).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Downloads (if needed) and loads the streaming model so the live preview is
/// ready. Long-running: the download awaits and the model load runs on a
/// blocking thread.
#[tauri::command]
pub async fn dictation_prepare_streaming(app: AppHandle) -> Result<(), String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let paths = crate::speech::models::ensure_streaming_model(&models_dir)
        .await
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::speech::streaming::cached(
            &paths.encoder,
            &paths.decoder,
            &paths.joiner,
            &paths.tokens,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    async fn dictation(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO dictation_history (id, raw_text, clean_text, duration_ms, asr_ms, created_at)
             VALUES (?1, 'r', 'hello', 0, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dictation_translations_stack_per_language_and_cascade() {
        let pool = test_pool().await;
        dictation(&pool, "d1").await;
        upsert_dictation_translation(&pool, "d1", "German", "hallo", "m")
            .await
            .unwrap();
        upsert_dictation_translation(&pool, "d1", "French", "bonjour", "m")
            .await
            .unwrap();
        assert_eq!(
            fetch_dictation_translations(&pool, "d1")
                .await
                .unwrap()
                .len(),
            2
        );

        // Re-translating a language replaces rather than duplicating.
        upsert_dictation_translation(&pool, "d1", "German", "hallo!", "m")
            .await
            .unwrap();
        let all = fetch_dictation_translations(&pool, "d1").await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|t| t.lang == "German" && t.text == "hallo!"));

        // Deleting the dictation cascades its translations away.
        sqlx::query("DELETE FROM dictation_history WHERE id = 'd1'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(fetch_dictation_translations(&pool, "d1")
            .await
            .unwrap()
            .is_empty());
    }
}
