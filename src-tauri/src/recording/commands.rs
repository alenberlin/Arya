//! Tauri commands for the recording lifecycle, processing retry, and crash
//! recovery. DB rows are the source of truth for session state; the recorder
//! worker holds only the live stream.

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

use super::recorder::{Recorder, RecorderStatus, StartSpec};
use super::settings::{save_generation_settings, GenerationSettings};
use crate::audio::wav_file;

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableRecording {
    pub session_id: String,
    pub note_id: String,
    pub note_title: String,
    pub partial_path: String,
    pub size_bytes: i64,
    pub started_at: String,
}

fn recordings_dir(app: &AppHandle, session_id: &str) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("recordings")
        .join(session_id))
}

/// Starts recording into `note_id` (creating a fresh note when omitted).
/// Returns the note id.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    recorder: State<'_, Recorder>,
    note_id: Option<String>,
) -> Result<String, String> {
    start_recording_inner(&app, &pool, &recorder, note_id).await
}

pub async fn start_recording_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    recorder: &Recorder,
    note_id: Option<String>,
) -> Result<String, String> {
    let note_id = match note_id {
        Some(id) => id,
        None => {
            crate::notes::insert_note(pool, "New recording")
                .await
                .map_err(|e| e.to_string())?
                .id
        }
    };
    let session_id = uuid::Uuid::new_v4().to_string();
    let dir = recordings_dir(app, &session_id)?;
    let final_path = dir.join("microphone.wav");

    let (sample_rate, channels) = recorder.start(StartSpec {
        session_id: session_id.clone(),
        note_id: note_id.clone(),
        final_path: final_path.clone(),
        device: None,
    })?;

    sqlx::query(
        "INSERT INTO recording_sessions
             (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
         VALUES (?1, ?2, 'recording', 'microphone-only', ?3, ?4,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(&session_id)
    .bind(&note_id)
    .bind(sample_rate as i64)
    .bind(channels as i64)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    let partial = wav_file::partial_path_for(&final_path);
    sqlx::query(
        "INSERT INTO audio_artifacts (id, session_id, source, path, status, created_at)
         VALUES (?1, ?2, 'microphone', ?3, 'partial', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&session_id)
    .bind(partial.to_string_lossy().to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE notes SET processing_status = 'recording',
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
    )
    .bind(&note_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(note_id)
}

#[tauri::command]
pub async fn pause_recording(
    pool: State<'_, SqlitePool>,
    recorder: State<'_, Recorder>,
) -> Result<(), String> {
    let status = recorder.status();
    recorder.pause()?;
    if let Some(session_id) = status.session_id {
        update_session_status(&pool, &session_id, "paused", recorder.elapsed_ms()).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_recording(
    pool: State<'_, SqlitePool>,
    recorder: State<'_, Recorder>,
) -> Result<(), String> {
    recorder.resume()?;
    if let Some(session_id) = recorder.status().session_id {
        update_session_status(&pool, &session_id, "recording", recorder.elapsed_ms()).await?;
    }
    Ok(())
}

/// Stops recording, finalizes the WAV, and kicks off processing.
#[tauri::command]
pub async fn finish_recording(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    recorder: State<'_, Recorder>,
) -> Result<String, String> {
    finish_recording_inner(&app, &pool, &recorder).await
}

pub async fn finish_recording_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    recorder: &Recorder,
) -> Result<String, String> {
    let status = recorder.status();
    let (session_id, note_id) = match (status.session_id, status.note_id) {
        (Some(s), Some(n)) => (s, n),
        _ => return Err("not recording".into()),
    };
    let elapsed = recorder.elapsed_ms();
    let final_path = recorder.finish()?;

    sqlx::query(
        "UPDATE audio_artifacts SET path = ?2, status = 'final', size_bytes = ?3
         WHERE session_id = ?1 AND source = 'microphone'",
    )
    .bind(&session_id)
    .bind(final_path.to_string_lossy().to_string())
    .bind(
        std::fs::metadata(&final_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0),
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    update_session_status(pool, &session_id, "finished", elapsed).await?;

    spawn_processing(app.clone(), pool.clone(), note_id.clone());
    Ok(note_id)
}

#[tauri::command]
pub fn recording_status(recorder: State<'_, Recorder>) -> RecorderStatus {
    recorder.status()
}

/// Re-runs processing for a failed note from its saved artifacts.
#[tauri::command]
pub fn retry_processing(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    note_id: String,
) -> Result<(), String> {
    spawn_processing(app, pool.inner().clone(), note_id);
    Ok(())
}

/// Sessions that were live when the app died, with recoverable bytes on disk.
/// Disk wins: a session only counts when its partial file has audio data.
#[tauri::command]
pub async fn scan_recoverable_recordings(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<RecoverableRecording>, String> {
    scan_recoverable_inner(&pool).await
}

pub async fn scan_recoverable_inner(
    pool: &SqlitePool,
) -> Result<Vec<RecoverableRecording>, String> {
    let candidates = sqlx::query_as::<_, RecoverableRecording>(
        "SELECT s.id AS session_id, s.note_id, n.title AS note_title,
                a.path AS partial_path, 0 AS size_bytes, s.started_at
         FROM recording_sessions s
         JOIN notes n ON n.id = s.note_id
         JOIN audio_artifacts a ON a.session_id = s.id AND a.status = 'partial'
         WHERE s.status IN ('recording', 'paused')
         ORDER BY s.started_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(candidates
        .into_iter()
        .filter_map(|mut c| {
            let len = std::fs::metadata(&c.partial_path).ok()?.len();
            // 44-byte header + at least a second of audio to be worth it.
            if len > 44 + 32_000 {
                c.size_bytes = len as i64;
                Some(c)
            } else {
                None
            }
        })
        .collect())
}

/// Repairs the partial WAV, promotes it to final, and processes the note.
#[tauri::command]
pub async fn recover_recording(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<String, String> {
    recover_recording_inner(&app, &pool, &session_id).await
}

pub async fn recover_recording_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    session_id: &str,
) -> Result<String, String> {
    let (note_id, partial_path) = sqlx::query_as::<_, (String, String)>(
        "SELECT s.note_id, a.path FROM recording_sessions s
         JOIN audio_artifacts a ON a.session_id = s.id AND a.status = 'partial'
         WHERE s.id = ?1",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let partial = std::path::PathBuf::from(&partial_path);
    wav_file::repair_header(&partial).map_err(|e| e.to_string())?;
    let final_path = partial.with_file_name("microphone.wav");
    std::fs::rename(&partial, &final_path).map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE audio_artifacts SET path = ?2, status = 'final', size_bytes = ?3
         WHERE session_id = ?1 AND source = 'microphone'",
    )
    .bind(session_id)
    .bind(final_path.to_string_lossy().to_string())
    .bind(
        std::fs::metadata(&final_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0),
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE recording_sessions SET status = 'finished', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    spawn_processing(app.clone(), pool.clone(), note_id.clone());
    Ok(note_id)
}

/// Discards a crashed session's audio and marks it discarded.
#[tauri::command]
pub async fn discard_recording(
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<(), String> {
    let paths =
        sqlx::query_scalar::<_, String>("SELECT path FROM audio_artifacts WHERE session_id = ?1")
            .fetch_all(&*pool)
            .await
            .map_err(|e| e.to_string())?;
    for path in paths {
        let _ = std::fs::remove_file(&path);
    }
    sqlx::query("UPDATE recording_sessions SET status = 'discarded', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(&session_id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_generation_settings(app: AppHandle) -> GenerationSettings {
    super::settings::load_generation_settings(&app)
}

#[tauri::command]
pub fn set_generation_settings(app: AppHandle, settings: GenerationSettings) -> Result<(), String> {
    save_generation_settings(&app, &settings)
}

fn spawn_processing(app: AppHandle, pool: SqlitePool, note_id: String) {
    std::thread::Builder::new()
        .name("arya-note-processing".into())
        .spawn(move || {
            let _ = super::processing::process_note(&app, &pool, &note_id);
        })
        .expect("spawn processing thread");
}

async fn update_session_status(
    pool: &SqlitePool,
    session_id: &str,
    status: &str,
    elapsed_ms: u64,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE recording_sessions SET status = ?2, elapsed_ms = ?3,
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
    )
    .bind(session_id)
    .bind(status)
    .bind(elapsed_ms as i64)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}
