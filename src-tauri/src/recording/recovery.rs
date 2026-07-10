//! Crash recovery: find sessions that were live when the app died, repair their
//! torn partial WAVs, promote them to final, and re-run processing. Disk is the
//! source of truth — a session only recovers when its partial file has audio.

use sqlx::SqlitePool;
use tauri::{AppHandle, State};

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
	         WHERE s.status IN ('starting', 'recording', 'paused')
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
    let note_id = recover_artifacts(pool, session_id).await?;
    super::commands::spawn_processing(app.clone(), pool.clone(), note_id.clone());
    Ok(note_id)
}

/// The pure recovery core (no AppHandle): repair + promote every partial
/// artifact of a crashed session, mark it finished, return the note id.
/// Separated so the multi-artifact data-loss fix is covered by a fast,
/// deterministic test instead of a flaky live-audio E2E.
pub async fn recover_artifacts(pool: &SqlitePool, session_id: &str) -> Result<String, String> {
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
    // Decide success by whether final audio now exists — by us OR a concurrent
    // recover call (double-click) that beat us to the renames. That makes a
    // second caller idempotent: it returns the note id instead of a spurious
    // "no recoverable audio" error when the first already promoted the tracks.
    let final_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audio_artifacts WHERE session_id = ?1 AND status = 'final'",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    if final_count == 0 {
        return Err("no recoverable audio in this session".to_string());
    }
    sqlx::query("UPDATE recording_sessions SET status = 'finished', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(note_id)
}

/// Discards a crashed session's audio and marks it discarded.
#[tauri::command]
pub async fn discard_recording(
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<(), String> {
    discard_recording_inner(&pool, &session_id).await
}

pub async fn discard_recording_inner(pool: &SqlitePool, session_id: &str) -> Result<(), String> {
    let paths =
        sqlx::query_scalar::<_, String>("SELECT path FROM audio_artifacts WHERE session_id = ?1")
            .bind(session_id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    for path in paths {
        let _ = std::fs::remove_file(&path);
    }
    sqlx::query("UPDATE recording_sessions SET status = 'discarded', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    /// Writes a canonical 16-bit PCM WAV, then zeroes the RIFF/data size
    /// fields the way a `kill -9` leaves a torn file.
    fn crashed_partial(path: &std::path::Path, seconds: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(16_000 * seconds) {
            // A loud tone so the bytes are unambiguously real audio.
            let v = ((i as f32 * 0.05).sin() * 12_000.0) as i16;
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();
        let mut bytes = std::fs::read(path).unwrap();
        bytes[4..8].copy_from_slice(&[0, 0, 0, 0]);
        bytes[40..44].copy_from_slice(&[0, 0, 0, 0]);
        std::fs::write(path, bytes).unwrap();
    }

    async fn seed_session(pool: &SqlitePool, dir: &std::path::Path, sources: &[&str]) -> String {
        let note_id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO notes (id, title, created_at) VALUES (?1, 'New recording', '2026-01-01T00:00:00Z')")
            .bind(&note_id).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO recording_sessions (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
             VALUES (?1, ?2, 'recording', 'microphone-and-system', 16000, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        ).bind(&session_id).bind(&note_id).execute(pool).await.unwrap();
        for source in sources {
            let partial = dir.join(format!("{source}.partial.wav"));
            crashed_partial(&partial, 2);
            sqlx::query("INSERT INTO audio_artifacts (id, session_id, source, path, status, created_at) VALUES (?1, ?2, ?3, ?4, 'partial', '2026-01-01T00:00:00Z')")
                .bind(uuid::Uuid::new_v4().to_string()).bind(&session_id).bind(*source)
                .bind(partial.to_string_lossy().to_string()).execute(pool).await.unwrap();
        }
        session_id
    }

    /// The C2 regression guard: a crashed meeting session's mic AND system
    /// tracks must both be promoted to final (the data-loss bug that dropped
    /// the system track).
    #[tokio::test]
    async fn recovery_promotes_all_source_artifacts() {
        let pool = test_pool().await;
        let dir = std::env::temp_dir().join(format!("arya-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session_id = seed_session(&pool, &dir, &["microphone", "system"]).await;

        recover_artifacts(&pool, &session_id).await.unwrap();

        let finals = sqlx::query_as::<_, (String, String)>(
            "SELECT source, status FROM audio_artifacts WHERE session_id = ?1 ORDER BY source",
        )
        .bind(&session_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(finals.len(), 2);
        assert!(
            finals.iter().all(|(_, status)| status == "final"),
            "both tracks must be final: {finals:?}"
        );
        // Each promoted to <source>.wav with real bytes.
        assert!(dir.join("microphone.wav").exists());
        assert!(dir.join("system.wav").exists());
        let session_status =
            sqlx::query_scalar::<_, String>("SELECT status FROM recording_sessions WHERE id = ?1")
                .bind(&session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(session_status, "finished");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One silent/empty track must not block recovering the other.
    #[tokio::test]
    async fn recovery_skips_empty_track_but_keeps_the_good_one() {
        let pool = test_pool().await;
        let dir = std::env::temp_dir().join(format!("arya-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session_id = seed_session(&pool, &dir, &["microphone", "system"]).await;
        // Truncate the system track to a header-only (empty) file.
        std::fs::write(dir.join("system.partial.wav"), vec![0u8; 44]).unwrap();

        let note_id = recover_artifacts(&pool, &session_id).await.unwrap();
        assert!(!note_id.is_empty());

        let mic = sqlx::query_scalar::<_, String>(
            "SELECT status FROM audio_artifacts WHERE session_id = ?1 AND source = 'microphone'",
        )
        .bind(&session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let sys = sqlx::query_scalar::<_, String>(
            "SELECT status FROM audio_artifacts WHERE session_id = ?1 AND source = 'system'",
        )
        .bind(&session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mic, "final", "the good mic track must recover");
        assert_eq!(
            sys, "partial",
            "the empty system track stays partial (skipped)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn discard_removes_artifacts_and_marks_session_discarded() {
        let pool = test_pool().await;
        let dir = std::env::temp_dir().join(format!("arya-rec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session_id = seed_session(&pool, &dir, &["microphone", "system"]).await;

        discard_recording_inner(&pool, &session_id).await.unwrap();

        assert!(
            !dir.join("microphone.partial.wav").exists(),
            "microphone partial should be removed"
        );
        assert!(
            !dir.join("system.partial.wav").exists(),
            "system partial should be removed"
        );
        let status =
            sqlx::query_scalar::<_, String>("SELECT status FROM recording_sessions WHERE id = ?1")
                .bind(&session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "discarded");
        std::fs::remove_dir_all(&dir).ok();
    }
}
