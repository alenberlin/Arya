//! Voice-profile enrollment: record a few seconds of mic audio, compute a
//! speaker embedding, and upsert it so meeting diarization can name speakers.

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

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
