//! Tauri commands for the recording lifecycle, processing retry, and crash
//! recovery. DB rows are the source of truth for session state; the recorder
//! worker holds only the live stream.

use std::sync::Mutex;

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager, State};

use super::recorder::{Recorder, RecorderState, RecorderStatus, StartSpec};
use super::settings::{save_generation_settings, GenerationSettings};
use crate::audio::system_capture::SystemCapture;
use crate::audio::wav_file;

/// Holds the live system-audio helper while a meeting-mode recording runs.
#[derive(Default)]
pub struct SystemCaptureSlot(pub Mutex<Option<SystemCapture>>);

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

    // Meeting mode: start the out-of-process system tap alongside the mic.
    // Failures degrade to mic-only with a visible warning, never a dead note.
    let mut system_started = false;
    if want_system {
        match SystemCapture::start(&helper_bin_dir(app)?, &dir) {
            Ok(mut capture) => match capture.wait_ready(std::time::Duration::from_secs(8)) {
                Ok(()) => {
                    let slot = app.state::<SystemCaptureSlot>();
                    *slot.0.lock().expect("system capture slot") = Some(capture);
                    system_started = true;
                }
                Err(message) => {
                    let _ = app.emit("recording:system-audio-unavailable", message.clone());
                    eprintln!("system audio unavailable: {message}");
                }
            },
            Err(message) => {
                let _ = app.emit("recording:system-audio-unavailable", message.clone());
                eprintln!("system audio unavailable: {message}");
            }
        }
    }
    let mode_label = if system_started {
        "microphone-and-system"
    } else {
        "microphone-only"
    };

    sqlx::query(
        "INSERT INTO recording_sessions
             (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
         VALUES (?1, ?2, 'recording', ?5, ?3, ?4,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(&session_id)
    .bind(&note_id)
    .bind(sample_rate as i64)
    .bind(channels as i64)
    .bind(mode_label)
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
    if system_started {
        sqlx::query(
            "INSERT INTO audio_artifacts (id, session_id, source, path, status, created_at)
             VALUES (?1, ?2, 'system', ?3, 'partial', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&session_id)
        .bind(dir.join("system.partial.wav").to_string_lossy().to_string())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
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
                    sqlx::query(
                        "UPDATE audio_artifacts SET path = ?2, status = 'final', size_bytes = ?3
                         WHERE session_id = ?1 AND source = 'system'",
                    )
                    .bind(&session_id)
                    .bind(system_final.to_string_lossy().to_string())
                    .bind(
                        std::fs::metadata(&system_final)
                            .map(|m| m.len() as i64)
                            .unwrap_or(0),
                    )
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
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
    // A meeting-mode session has two partial artifacts (microphone, system).
    // Recover every one that carries data; drop only silent/empty tracks.
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT s.note_id, a.source, a.path FROM recording_sessions s
         JOIN audio_artifacts a ON a.session_id = s.id AND a.status = 'partial'
         WHERE s.id = ?1",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let note_id = rows
        .first()
        .map(|(note_id, _, _)| note_id.clone())
        .ok_or_else(|| "no partial artifacts for session".to_string())?;

    let mut recovered_any = false;
    for (_, source, partial_path) in &rows {
        let partial = std::path::PathBuf::from(partial_path);
        // An empty/header-only track (repair returns Empty) is skipped, not
        // fatal, so one silent source can't block recovering the other.
        if wav_file::repair_header(&partial).is_err() {
            continue;
        }
        let final_path = partial.with_file_name(format!("{source}.wav"));
        if std::fs::rename(&partial, &final_path).is_err() {
            continue;
        }
        recovered_any = true;
        sqlx::query(
            "UPDATE audio_artifacts SET path = ?3, status = 'final', size_bytes = ?4
             WHERE session_id = ?1 AND source = ?2",
        )
        .bind(session_id)
        .bind(source)
        .bind(final_path.to_string_lossy().to_string())
        .bind(
            std::fs::metadata(&final_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0),
        )
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    if !recovered_any {
        return Err("no recoverable audio in this session".to_string());
    }
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

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerProfileInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// Records `seconds` of mic audio and enrolls it as a named voice profile.
#[tauri::command]
pub async fn enroll_speaker_profile(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    name: String,
    seconds: Option<u32>,
) -> Result<SpeakerProfileInfo, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("a name is required".into());
    }
    let seconds = seconds.unwrap_or(6).clamp(3, 20);
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || enroll_blocking(&app, &pool, &name, seconds))
        .await
        .map_err(|e| e.to_string())?
}

/// Blocking enrollment body (shared with dev hooks): capture, embed, upsert.
pub fn enroll_blocking(
    app: &AppHandle,
    pool: &SqlitePool,
    name: &str,
    seconds: u32,
) -> Result<SpeakerProfileInfo, String> {
    {
        let handle = crate::audio::start_capture(None).map_err(|e| e.to_string())?;
        std::thread::sleep(std::time::Duration::from_secs(seconds as u64));
        let clip = handle.stop().map_err(|e| e.to_string())?;
        if clip.duration_secs() < 2.0 {
            return Err("recording too short".to_string());
        }
        // Embed speech only: concatenate the energetic spans so the profile
        // matches what turn slices look like (no silence dilution).
        let spans =
            crate::audio::turns::detect_turns(&clip, &crate::audio::turns::TurnConfig::default());
        let clip = if spans.is_empty() {
            clip
        } else {
            let mut samples = Vec::new();
            for span in &spans {
                samples
                    .extend_from_slice(&crate::audio::turns::slice_turn(&clip, span, 100).samples);
            }
            crate::speech::AudioClip { samples }
        };
        if clip.duration_secs() < 1.5 {
            return Err("not enough speech in the enrollment recording".to_string());
        }
        let models_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("models");
        let model_path = tauri::async_runtime::block_on(crate::speech::models::ensure_model(
            &super::diarize::SPEAKER_MODEL,
            &models_dir,
        ))
        .map_err(|e| e.to_string())?;
        let extractor = super::diarize::get_or_load_extractor(&model_path.to_string_lossy())?;
        let embedding = extractor
            .lock()
            .expect("extractor lock")
            .compute_speaker_embedding(clip.samples, crate::speech::AudioClip::SAMPLE_RATE)
            .map_err(|e| e.to_string())?;
        let blob = super::diarize::f32_to_blob(&embedding);
        let id = uuid::Uuid::new_v4().to_string();
        tauri::async_runtime::block_on(async {
            sqlx::query(
                "INSERT INTO speaker_profiles (id, name, embedding, created_at)
                 VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(name) DO UPDATE SET embedding = excluded.embedding",
            )
            .bind(&id)
            .bind(name)
            .bind(&blob)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())
        })?;
        Ok(SpeakerProfileInfo {
            id,
            name: name.to_string(),
            created_at: String::new(),
        })
    }
}

#[tauri::command]
pub async fn list_speaker_profiles(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<SpeakerProfileInfo>, String> {
    sqlx::query_as::<_, SpeakerProfileInfo>(
        "SELECT id, name, created_at FROM speaker_profiles ORDER BY name",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_speaker_profile(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    sqlx::query("DELETE FROM speaker_profiles WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
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
