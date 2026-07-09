//! Tauri commands for the recording lifecycle (start/pause/resume/finish),
//! processing retry, live preview, and generation settings. DB rows are the
//! source of truth for session state; the recorder worker holds only the live
//! stream. Crash recovery lives in [`super::recovery`], voice enrollment in
//! [`super::enroll`].

use std::sync::Mutex;

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager, State};

use super::recorder::{Recorder, RecorderState, RecorderStatus, StartSpec};
use super::settings::{save_generation_settings, GenerationSettings};
use crate::audio::system_capture::SystemCapture;
use crate::audio::wav_file;

const BLANK_SYSTEM_AUDIO_MESSAGE: &str =
    "System audio captured no samples. Check macOS System Audio Recording permission and make sure meeting audio is playing through this Mac.";

/// Holds the live system-audio helper while a meeting-mode recording runs.
#[derive(Default)]
pub struct SystemCaptureSlot(pub Mutex<Option<SystemCapture>>);

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
    source_mode: Option<String>,
) -> Result<String, String> {
    start_recording_inner(&app, &pool, &recorder, note_id, source_mode).await
}

pub async fn start_recording_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    recorder: &Recorder,
    note_id: Option<String>,
    source_mode: Option<String>,
) -> Result<String, String> {
    let want_system = source_mode.as_deref() == Some("microphone-and-system");
    let helper_dir = if want_system {
        Some(helper_bin_dir(app)?)
    } else {
        None
    };
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
    let partial = wav_file::partial_path_for(&final_path);
    let requested_mode = if want_system {
        "microphone-and-system"
    } else {
        "microphone-only"
    };
    let system_partial = want_system.then(|| dir.join("system.partial.wav"));

    // Persist recoverability metadata BEFORE capture starts. If the app dies
    // after the recorder creates bytes but before the final status update below,
    // recovery can still find the session by its `starting` row and partial path.
    insert_starting_session(
        pool,
        &session_id,
        &note_id,
        requested_mode,
        &partial,
        system_partial.as_deref(),
    )
    .await?;

    let (sample_rate, channels) = match recorder.start(StartSpec {
        session_id: session_id.clone(),
        note_id: note_id.clone(),
        final_path: final_path.clone(),
        device: None,
    }) {
        Ok(spec) => spec,
        Err(e) => {
            cleanup_failed_start(pool, &session_id, &dir).await;
            return Err(e);
        }
    };

    // Meeting mode: start the out-of-process system tap alongside the mic.
    // Failures degrade to mic-only with a visible warning, never a dead note.
    let mut system_started = false;
    if let Some(helper_dir) = helper_dir.as_ref() {
        match SystemCapture::start(helper_dir, &dir) {
            Ok(mut capture) => match capture.wait_ready(std::time::Duration::from_secs(8)) {
                Ok(()) => {
                    let slot = app.state::<SystemCaptureSlot>();
                    *slot.0.lock().expect("system capture slot") = Some(capture);
                    system_started = true;
                }
                Err(message) => {
                    let _ = app.emit("recording:system-audio-unavailable", message.clone());
                    eprintln!("system audio unavailable: {message}");
                    remove_system_artifact(pool, &session_id).await?;
                }
            },
            Err(message) => {
                let _ = app.emit("recording:system-audio-unavailable", message.clone());
                eprintln!("system audio unavailable: {message}");
                remove_system_artifact(pool, &session_id).await?;
            }
        }
    }
    let mode_label = if system_started {
        "microphone-and-system"
    } else {
        "microphone-only"
    };

    if let Err(e) =
        mark_session_recording(pool, &session_id, mode_label, sample_rate, channels).await
    {
        let _ = recorder.finish();
        if let Some(capture) = app
            .state::<SystemCaptureSlot>()
            .0
            .lock()
            .expect("system capture slot")
            .take()
        {
            let _ = capture.stop();
        }
        cleanup_failed_start(pool, &session_id, &dir).await;
        return Err(e);
    }
    // Calendar context: title the note from the live event, keep attendees.
    if let Some(event) = crate::calendar::current_or_upcoming_event(10) {
        let context = serde_json::json!({
            "title": event.title,
            "attendees": event.attendees,
        });
        let _ = sqlx::query(
            "UPDATE notes SET title = ?2, calendar_context = ?3 WHERE id = ?1 AND title = 'New recording'",
        )
        .bind(&note_id)
        .bind(&event.title)
        .bind(context.to_string())
        .execute(pool)
        .await;
    }
    spawn_live_preview(app.clone(), recorder.clone(), note_id.clone());

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
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    recorder: State<'_, Recorder>,
) -> Result<(), String> {
    let status = recorder.status();
    recorder.pause()?;
    if let Some(capture) = app
        .state::<SystemCaptureSlot>()
        .0
        .lock()
        .expect("slot")
        .as_ref()
    {
        capture.pause();
    }
    if let Some(session_id) = status.session_id {
        update_session_status(&pool, &session_id, "paused", recorder.elapsed_ms()).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_recording(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    recorder: State<'_, Recorder>,
) -> Result<(), String> {
    recorder.resume()?;
    if let Some(capture) = app
        .state::<SystemCaptureSlot>()
        .0
        .lock()
        .expect("slot")
        .as_ref()
    {
        capture.resume();
    }
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

    // Stop the system helper (if running) and promote its file.
    let capture = app
        .state::<SystemCaptureSlot>()
        .0
        .lock()
        .expect("slot")
        .take();
    if let Some(capture) = capture {
        match capture.stop() {
            Ok(partial) => {
                let system_final = partial.with_file_name("system.wav");
                if std::fs::rename(&partial, &system_final).is_ok() {
                    let size = std::fs::metadata(&system_final)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    if system_artifact_has_audio(size) {
                        sqlx::query(
                            "UPDATE audio_artifacts SET path = ?2, status = 'final', size_bytes = ?3
                             WHERE session_id = ?1 AND source = 'system'",
                        )
                        .bind(&session_id)
                        .bind(system_final.to_string_lossy().to_string())
                        .bind(size as i64)
                        .execute(pool)
                        .await
                        .map_err(|e| e.to_string())?;
                    } else {
                        let _ = app.emit(
                            "recording:system-audio-unavailable",
                            BLANK_SYSTEM_AUDIO_MESSAGE,
                        );
                        eprintln!("system audio unavailable: {BLANK_SYSTEM_AUDIO_MESSAGE}");
                        let _ = std::fs::remove_file(&system_final);
                        remove_system_artifact(pool, &session_id).await?;
                    }
                }
            }
            Err(message) => eprintln!("system capture stop failed: {message}"),
        }
    }

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

async fn insert_starting_session(
    pool: &SqlitePool,
    session_id: &str,
    note_id: &str,
    source_mode: &str,
    microphone_partial: &std::path::Path,
    system_partial: Option<&std::path::Path>,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO recording_sessions
             (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
         VALUES (?1, ?2, 'starting', ?3, 0, 0,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(session_id)
    .bind(note_id)
    .bind(source_mode)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO audio_artifacts (id, session_id, source, path, status, created_at)
         VALUES (?1, ?2, 'microphone', ?3, 'partial', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(microphone_partial.to_string_lossy().to_string())
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if let Some(path) = system_partial {
        sqlx::query(
            "INSERT INTO audio_artifacts (id, session_id, source, path, status, created_at)
             VALUES (?1, ?2, 'system', ?3, 'partial', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(path.to_string_lossy().to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

async fn mark_session_recording(
    pool: &SqlitePool,
    session_id: &str,
    source_mode: &str,
    sample_rate: u32,
    channels: u16,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE recording_sessions
         SET status = 'recording', source_mode = ?2, sample_rate = ?3, channels = ?4,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(session_id)
    .bind(source_mode)
    .bind(sample_rate as i64)
    .bind(channels as i64)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

async fn remove_system_artifact(pool: &SqlitePool, session_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM audio_artifacts WHERE session_id = ?1 AND source = 'system'")
        .bind(session_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn system_artifact_has_audio(size_bytes: u64) -> bool {
    size_bytes > wav_file::HEADER_LEN
}

async fn cleanup_failed_start(pool: &SqlitePool, session_id: &str, dir: &std::path::Path) {
    let _ = sqlx::query("DELETE FROM recording_sessions WHERE id = ?1")
        .bind(session_id)
        .execute(pool)
        .await;
    let _ = std::fs::remove_dir_all(dir);
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

#[tauri::command]
pub fn get_generation_settings(app: AppHandle) -> GenerationSettings {
    super::settings::load_generation_settings(&app)
}

#[tauri::command]
pub fn set_generation_settings(app: AppHandle, settings: GenerationSettings) -> Result<(), String> {
    save_generation_settings(&app, &settings)
}

pub(super) fn spawn_processing(app: AppHandle, pool: SqlitePool, note_id: String) {
    std::thread::Builder::new()
        .name("arya-note-processing".into())
        .spawn(move || {
            let _ = super::processing::process_note(&app, &pool, &note_id);
        })
        .expect("spawn processing thread");
}

#[tauri::command]
pub fn calendar_access_status() -> crate::calendar::CalendarAccess {
    crate::calendar::access_status()
}

#[tauri::command]
pub async fn request_calendar_access() -> Result<crate::calendar::CalendarAccess, String> {
    tauri::async_runtime::spawn_blocking(crate::calendar::request_access)
        .await
        .map_err(|e| e.to_string())
}

fn helper_bin_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("bin"))
}

/// Ephemeral live transcript preview: every ~7s, transcribe the newest mic
/// audio and emit it. Never persisted, never fatal; stops when recording does.
fn spawn_live_preview(app: AppHandle, recorder: Recorder, note_id: String) {
    std::thread::Builder::new()
        .name("arya-live-preview".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(7));
                let status = recorder.status();
                if status.state == RecorderState::Idle
                    || status.note_id.as_deref() != Some(&note_id)
                {
                    break;
                }
                if status.state == RecorderState::Paused {
                    continue;
                }
                let Some((raw, rate, channels)) = recorder.take_preview() else {
                    continue;
                };
                if raw.len() < (rate as usize * channels as usize * 2) {
                    continue; // under two seconds; wait for more
                }
                let mono = crate::audio::resample::downmix_interleaved(&raw, channels);
                let Ok(samples) = crate::audio::resample::resample_to_16k(&mono, rate) else {
                    continue;
                };
                let Ok(model_path) = super::processing::default_model_path(&app) else {
                    continue;
                };
                let Ok(engine) = crate::speech::engine_cache::get_or_load(&model_path) else {
                    continue;
                };
                use crate::speech::SpeechEngine;
                let clip = crate::speech::AudioClip { samples };
                if let Ok(transcript) =
                    engine.transcribe(&clip, &crate::speech::TranscribeOptions::default())
                {
                    let text = transcript.text.trim().to_string();
                    if !text.is_empty() {
                        #[cfg(debug_assertions)]
                        eprintln!("live preview: {text}");
                        let _ = app.emit(
                            "note:live-preview",
                            serde_json::json!({ "noteId": note_id, "text": text }),
                        );
                    }
                }
            }
        })
        .expect("spawn live preview thread");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn starting_session_with_partial_bytes_is_recoverable() {
        let pool = test_pool().await;
        let note = crate::notes::insert_note(&pool, "Crash window")
            .await
            .unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let dir = std::env::temp_dir().join(format!("arya-starting-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mic_partial = dir.join("microphone.partial.wav");

        insert_starting_session(
            &pool,
            &session_id,
            &note.id,
            "microphone-only",
            &mic_partial,
            None,
        )
        .await
        .unwrap();
        std::fs::write(&mic_partial, vec![0u8; 44 + 32_001]).unwrap();

        let recoverable = super::super::recovery::scan_recoverable_inner(&pool)
            .await
            .unwrap();
        assert!(
            recoverable.iter().any(|r| r.session_id == session_id),
            "a crash after capture starts but before the status update must be recoverable"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn header_only_system_audio_is_blank() {
        assert!(!system_artifact_has_audio(wav_file::HEADER_LEN));
        assert!(!system_artifact_has_audio(0));
        assert!(system_artifact_has_audio(wav_file::HEADER_LEN + 2));
    }
}
