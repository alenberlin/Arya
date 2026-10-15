//! Voice-profile enrollment: record a few seconds of mic audio, compute a
//! speaker embedding, and upsert it so meeting diarization can name speakers.

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, serde::Serialize, sqlx::FromRow, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerProfileInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// Records `seconds` of mic audio and enrolls it as a named voice profile.
#[specta::specta]
#[tauri::command]
pub async fn enroll_speaker_profile(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    name: String,
    seconds: Option<u32>,
) -> crate::error::AryaResult<SpeakerProfileInfo> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("a name is required".into());
    }
    let seconds = seconds.unwrap_or(6).clamp(3, 20);
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || enroll_blocking(&app, &pool, &name, seconds))
        .await
        .map_err(crate::error::AryaError::from_display)?
}

/// Blocking enrollment body (shared with dev hooks): capture, embed, upsert.
pub fn enroll_blocking(
    app: &AppHandle,
    pool: &SqlitePool,
    name: &str,
    seconds: u32,
) -> crate::error::AryaResult<SpeakerProfileInfo> {
    {
        let handle =
            crate::audio::start_capture(None).map_err(crate::error::AryaError::from_display)?;
        // Sleep in small slices, draining the bounded callback pool each time: a
        // single long sleep would fill the 64-block pool in under a second and
        // silently drop the rest of the enrollment audio.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds as u64);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = handle.level(); // level() drains the queued callback blocks
        }
        let clip = handle
            .stop()
            .map_err(crate::error::AryaError::from_display)?;
        if clip.duration_secs() < 2.0 {
            return Err(crate::error::AryaError::message("recording too short"));
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
            return Err(crate::error::AryaError::message(
                "not enough speech in the enrollment recording",
            ));
        }
        let models_dir = app
            .path()
            .app_data_dir()
            .map_err(crate::error::AryaError::from_display)?
            .join("models");
        let model_path = tauri::async_runtime::block_on(crate::speech::models::ensure_model(
            &super::diarize::SPEAKER_MODEL,
            &models_dir,
        ))
        .map_err(crate::error::AryaError::from_display)?;
        let extractor = super::diarize::get_or_load_extractor(&model_path.to_string_lossy())?;
        let embedding = extractor
            .lock()
            .expect("extractor lock")
            .compute_speaker_embedding(clip.samples, crate::speech::AudioClip::SAMPLE_RATE)
            .map_err(crate::error::AryaError::from_display)?;
        let blob = super::diarize::f32_to_blob(&embedding);
        let id = uuid::Uuid::new_v4().to_string();
        // RETURNING gives the row that actually won (the original id + created_at
        // on a name conflict, the fresh row otherwise), so re-enrolling the same
        // name never reports a bogus id or an empty created_at.
        tauri::async_runtime::block_on(async {
            sqlx::query_as::<_, SpeakerProfileInfo>(
                "INSERT INTO speaker_profiles (id, name, embedding, created_at)
                 VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(name) DO UPDATE SET embedding = excluded.embedding
                 RETURNING id, name, created_at",
            )
            .bind(&id)
            .bind(name)
            .bind(&blob)
            .fetch_one(pool)
            .await
            .map_err(crate::error::AryaError::from_display)
        })
    }
}

#[specta::specta]
#[tauri::command]
pub async fn list_speaker_profiles(
    pool: State<'_, SqlitePool>,
) -> crate::error::AryaResult<Vec<SpeakerProfileInfo>> {
    sqlx::query_as::<_, SpeakerProfileInfo>(
        "SELECT id, name, created_at FROM speaker_profiles ORDER BY name",
    )
    .fetch_all(&*pool)
    .await
    .map_err(crate::error::AryaError::from_display)
}

#[specta::specta]
#[tauri::command]
pub async fn delete_speaker_profile(
    pool: State<'_, SqlitePool>,
    id: String,
) -> crate::error::AryaResult<()> {
    sqlx::query("DELETE FROM speaker_profiles WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(crate::error::AryaError::from_display)
}
