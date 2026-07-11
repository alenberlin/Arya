//! Speech-model management for the setup UI.
//!
//! Whisper *weights* aren't bundled (they're 78–574 MB); only the engine is.
//! They download from Hugging Face — lazily on first dictation, or explicitly
//! from the "Speech models" card via [`download_speech_model`], which streams
//! `speech:download-progress` events so the card can show a real progress bar
//! instead of a spinner that looks frozen for a 574 MB file.

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use super::models;

/// One catalog model plus whether it's already on disk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelStatus {
    pub id: String,
    pub file_name: String,
    pub approx_bytes: u64,
    pub downloaded: bool,
}

fn models_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models"))
}

/// The speech-model catalog with per-model download state, so the picker can
/// mark which are ready and offer a Download button for the rest.
#[tauri::command]
pub fn speech_models_status(app: AppHandle) -> Result<Vec<SpeechModelStatus>, String> {
    let dir = models_dir(&app)?;
    Ok(models::CATALOG
        .iter()
        .map(|m| SpeechModelStatus {
            id: m.id.to_string(),
            file_name: m.file_name.to_string(),
            approx_bytes: m.approx_bytes,
            downloaded: dir.join(m.file_name).exists(),
        })
        .collect())
}

/// Download one catalog model, emitting `speech:download-progress`
/// `{ id, received, total, done }` as it streams. Idempotent: an already-present
/// model returns immediately with a `done` event.
#[tauri::command]
pub async fn download_speech_model(app: AppHandle, id: String) -> Result<(), String> {
    let spec = models::find(&id).ok_or_else(|| format!("unknown model: {id}"))?;
    let dir = models_dir(&app)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let target = dir.join(spec.file_name);
    let emit_done = || {
        let _ = app.emit(
            "speech:download-progress",
            json!({ "id": id, "received": spec.approx_bytes, "total": spec.approx_bytes, "done": true }),
        );
    };
    if target.exists() {
        emit_done();
        return Ok(());
    }
    let progress_app = app.clone();
    let progress_id = id.clone();
    models::download_verified_with_progress(
        spec.url,
        &target,
        spec.sha256,
        spec.id,
        move |received, total| {
            let _ = progress_app.emit(
                "speech:download-progress",
                json!({ "id": progress_id, "received": received, "total": total, "done": false }),
            );
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    emit_done();
    Ok(())
}

/// Delete a downloaded model to reclaim disk. Idempotent.
#[tauri::command]
pub async fn delete_speech_model(app: AppHandle, id: String) -> Result<(), String> {
    let spec = models::find(&id).ok_or_else(|| format!("unknown model: {id}"))?;
    let target = models_dir(&app)?.join(spec.file_name);
    match tokio::fs::remove_file(&target).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
