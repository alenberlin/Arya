//! The saved-audio-first processing pipeline: WAV on disk -> turns ->
//! transcript rows -> generated note. Every step is retryable from the
//! artifacts; nothing depends on in-memory state from recording time.

use std::{collections::HashMap, path::Path};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use super::diarize;
use super::generate::{FallbackGenerator, NoteGenerator, OllamaGenerator, TurnText};
use crate::audio::turns::{detect_turns, slice_turn, TurnConfig, TurnSpan};
use crate::audio::wav_file;
use crate::speech::{engine_cache, models, SpeechEngine, TranscribeOptions};

/// Max samples fed to ASR in one call (8 minutes at 16 kHz), so a very long
/// turn cannot blow up memory or model context.
const MAX_CHUNK_SAMPLES: usize = 16_000 * 480;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingEvent {
    pub note_id: String,
    pub status: String,
    pub error: Option<String>,
}

fn emit(app: &AppHandle, note_id: &str, status: &str, error: Option<String>) {
    #[cfg(debug_assertions)]
    eprintln!("processing: note={note_id} status={status} error={error:?}");
    let _ = app.emit(
        "note:processing",
        ProcessingEvent {
            note_id: note_id.to_string(),
            status: status.to_string(),
            error,
        },
    );
}

/// Runs (or re-runs) processing for a note from its saved audio. Existing
/// transcript turns are reused when present so a generation-stage failure
/// does not re-transcribe.
pub fn process_note(app: &AppHandle, pool: &SqlitePool, note_id: &str) -> Result<(), String> {
    let result = run(app, pool, note_id);
    if let Err(message) = &result {
        set_status_blocking(pool, note_id, "failed", Some(message.clone()));
        emit(app, note_id, "failed", Some(message.clone()));
    }
    result
}

fn run(app: &AppHandle, pool: &SqlitePool, note_id: &str) -> Result<(), String> {
    let existing_turns = fetch_turn_texts(pool, note_id)?;
    let turns = if existing_turns.is_empty() {
        set_status_blocking(pool, note_id, "transcribing", None);
        emit(app, note_id, "transcribing", None);
        let artifacts = final_artifacts(pool, note_id)?;
        if artifacts.files.is_empty() {
            return Err("no audio artifact for note".to_string());
        }
        let meeting_intent = artifacts.source_mode == "microphone-and-system";
        let mut turns: Vec<TurnText> = Vec::new();
        let mut samples_by_turn: Vec<Vec<f32>> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for (source, path) in &artifacts.files {
            match transcribe_artifact(app, Path::new(path), source) {
                Ok(source_turns) => {
                    for (turn, samples) in source_turns {
                        turns.push(turn);
                        samples_by_turn.push(samples);
                    }
                }
                Err(message) => {
                    // A silent or unreadable source (e.g. the system track
                    // when the TCC grant is missing) must not fail the note
                    // as long as another source produced speech.
                    eprintln!("processing: source {source} skipped: {message}");
                    errors.push(format!("{source}: {message}"));
                }
            }
        }
        if turns.is_empty() {
            return Err(errors.join("; "));
        }
        // Speaker labels are a meeting feature. Keep the user intent even when
        // system audio was blank and removed: the mic may still contain more
        // than one voice, and unknown voices must not collapse to "Me".
        if meeting_intent || artifacts.files.iter().any(|(source, _)| source == "system") {
            if let Err(message) = assign_speakers(app, pool, &mut turns, &samples_by_turn) {
                eprintln!("processing: diarization skipped: {message}");
            }
        }
        let mut order: Vec<usize> = (0..turns.len()).collect();
        order.sort_by_key(|i| turns[*i].start_ms);
        let turns: Vec<TurnText> = order.into_iter().map(|i| turns[i].clone()).collect();
        store_turns(pool, note_id, &turns)?;
        turns
    } else {
        let mut turns = existing_turns;
        if let Err(message) = refresh_existing_speaker_labels(app, pool, note_id, &mut turns) {
            eprintln!("processing: existing-turn diarization skipped: {message}");
        }
        turns
    };

    set_status_blocking(pool, note_id, "generating", None);
    emit(app, note_id, "generating", None);
    let manual_notes = fetch_manual_notes(pool, note_id)?;
    let settings = super::settings::load_generation_settings(app);
    let generated = match settings.model.as_deref() {
        Some(model) => {
            OllamaGenerator::new(settings.ollama_url.clone(), model).generate(&turns, &manual_notes)
        }
        None => FallbackGenerator.generate(&turns, &manual_notes),
    };

    tauri::async_runtime::block_on(async {
        sqlx::query(
            "UPDATE notes SET title = ?2, body_md = ?3, processing_status = 'ready',
             processing_error = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
        )
        .bind(note_id)
        .bind(&generated.title)
        .bind(&generated.body_md)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    })?;
    emit(app, note_id, "ready", None);
    Ok(())
}

type TurnWithSamples = (TurnText, Vec<f32>);

fn transcribe_artifact(
    app: &AppHandle,
    wav_path: &Path,
    source: &str,
) -> Result<Vec<TurnWithSamples>, String> {
    let clip = wav_file::load_normalized(wav_path).map_err(|e| e.to_string())?;
    let spans = detect_turns(&clip, &TurnConfig::default());
    if spans.is_empty() {
        return Err("no speech detected".to_string());
    }

    let model_path = default_model_path(app)?;
    let engine = engine_cache::get_or_load(&model_path).map_err(|e| e.to_string())?;
    let options = TranscribeOptions { language: None };

    let mut turns = Vec::new();
    for span in &spans {
        let sliced = slice_turn(&clip, span, 150);
        let mut text = String::new();
        for chunk in sliced.samples.chunks(MAX_CHUNK_SAMPLES) {
            let piece = engine
                .transcribe(
                    &crate::speech::AudioClip {
                        samples: chunk.to_vec(),
                    },
                    &options,
                )
                .map_err(|e| e.to_string())?;
            if !piece.text.trim().is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(piece.text.trim());
            }
        }
        if !text.is_empty() {
            turns.push((
                TurnText {
                    start_ms: span.start_ms,
                    end_ms: span.end_ms,
                    source: source.to_string(),
                    speaker: None,
                    text,
                },
                sliced.samples,
            ));
        }
    }
    if turns.is_empty() {
        return Err("transcription produced no text".to_string());
    }
    Ok(turns)
}

pub fn default_model_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    // Return an error instead of panicking: this runs on the detached note-
    // processing / live-preview threads, and a catalog rename must surface as a
    // failed note rather than a silent thread panic that kills processing.
    let spec = models::find("whisper-base.en").ok_or("model catalog is missing whisper-base.en")?;
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let target = models_dir.join(spec.file_name);
    if !target.exists() {
        tauri::async_runtime::block_on(models::ensure_model(spec, &models_dir))
            .map_err(|e| e.to_string())?;
    }
    Ok(target)
}

/// Labels turns with speakers: embeddings per qualifying turn, per-source
/// clustering, profile matching for names. Unknown clusters are anonymous
/// `Speaker N` labels; "Me" is only printed when it comes from an enrolled
/// speaker profile.
fn assign_speakers(
    app: &AppHandle,
    pool: &SqlitePool,
    turns: &mut [TurnText],
    samples_by_turn: &[Vec<f32>],
) -> Result<(), String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let model_path = tauri::async_runtime::block_on(crate::speech::models::ensure_model(
        &diarize::SPEAKER_MODEL,
        &models_dir,
    ))
    .map_err(|e| e.to_string())?;

    let extractor = diarize::get_or_load_extractor(&model_path.to_string_lossy())?;

    let profiles =
        tauri::async_runtime::block_on(diarize::load_profiles(pool)).map_err(|e| e.to_string())?;

    // Embeddings for turns long enough to carry identity, grouped by source.
    let min_samples =
        (crate::speech::AudioClip::SAMPLE_RATE as u64 * diarize::MIN_EMBED_MS / 1000) as usize;
    let mut anonymous = 0usize;
    for source in ["system", "microphone"] {
        let indices: Vec<usize> = (0..turns.len())
            .filter(|i| turns[*i].source == source)
            .collect();
        if indices.is_empty() {
            continue;
        }
        let mut embeddings: Vec<Vec<f32>> = Vec::new();
        let mut embedded_indices: Vec<usize> = Vec::new();
        for &i in &indices {
            if samples_by_turn[i].len() >= min_samples {
                match extractor
                    .lock()
                    .expect("extractor lock")
                    .compute_speaker_embedding(
                        samples_by_turn[i].clone(),
                        crate::speech::AudioClip::SAMPLE_RATE,
                    ) {
                    Ok(embedding) => {
                        embeddings.push(embedding);
                        embedded_indices.push(i);
                    }
                    Err(e) => eprintln!("diarize: embedding failed: {e}"),
                }
            }
        }
        if embeddings.is_empty() {
            // No usable embeddings: preserve the fact this was a meeting, but
            // don't pretend to know identity.
            let fallback = next_anonymous_speaker(&mut anonymous);
            for &i in &indices {
                turns[i].speaker = Some(fallback.clone());
            }
            continue;
        }
        let labels = diarize::cluster_embeddings(&embeddings, diarize::SAME_SPEAKER_THRESHOLD);
        let cluster_centroids = diarize::centroids(&embeddings, &labels);
        #[cfg(debug_assertions)]
        {
            let pairwise: Vec<String> = embeddings
                .iter()
                .enumerate()
                .flat_map(|(i, a)| {
                    embeddings
                        .iter()
                        .skip(i + 1)
                        .map(move |b| format!("{:.2}", diarize::cosine_similarity(a, b)))
                })
                .collect();
            eprintln!(
                "diarize: source={source} embeddings={} clusters={} pairwise=[{}]",
                embeddings.len(),
                cluster_centroids.len(),
                pairwise.join(",")
            );
        }
        let cluster_names = name_clusters(&cluster_centroids, &profiles, &mut anonymous);
        // Short turns without embeddings inherit their source's dominant label.
        let dominant = {
            let mut counts = vec![0usize; cluster_names.len()];
            for label in &labels {
                counts[*label] += 1;
            }
            counts
                .iter()
                .enumerate()
                .max_by_key(|(_, c)| **c)
                .map(|(i, _)| cluster_names[i].clone())
        };
        for (position, &i) in embedded_indices.iter().enumerate() {
            turns[i].speaker = Some(cluster_names[labels[position]].clone());
        }
        for &i in &indices {
            if turns[i].speaker.is_none() {
                turns[i].speaker = dominant.clone();
            }
        }
    }
    Ok(())
}

fn next_anonymous_speaker(anonymous: &mut usize) -> String {
    *anonymous += 1;
    format!("Speaker {anonymous}")
}

fn name_clusters(
    centroids: &[Vec<f32>],
    profiles: &[diarize::Profile],
    anonymous: &mut usize,
) -> Vec<String> {
    centroids
        .iter()
        .map(|centroid| {
            match diarize::match_profile(centroid, profiles, diarize::SAME_SPEAKER_THRESHOLD) {
                Some(name) => name.to_string(),
                None => next_anonymous_speaker(anonymous),
            }
        })
        .collect()
}

fn refresh_existing_speaker_labels(
    app: &AppHandle,
    pool: &SqlitePool,
    note_id: &str,
    turns: &mut [TurnText],
) -> Result<(), String> {
    let artifacts = final_artifacts(pool, note_id)?;
    let meeting_intent = artifacts.source_mode == "microphone-and-system";
    if !meeting_intent && !artifacts.files.iter().any(|(source, _)| source == "system") {
        return Ok(());
    }
    if artifacts.files.is_empty() {
        return Ok(());
    }

    let samples_by_turn = samples_for_existing_turns(&artifacts.files, turns)?;
    assign_speakers(app, pool, turns, &samples_by_turn)?;
    update_turn_speakers(pool, note_id, turns)
}

fn samples_for_existing_turns(
    artifact_files: &[(String, String)],
    turns: &[TurnText],
) -> Result<Vec<Vec<f32>>, String> {
    let mut clips = HashMap::new();
    for (source, path) in artifact_files {
        let clip = wav_file::load_normalized(Path::new(path)).map_err(|e| e.to_string())?;
        clips.insert(source.as_str(), clip);
    }

    Ok(turns
        .iter()
        .map(|turn| {
            clips
                .get(turn.source.as_str())
                .map(|clip| {
                    slice_turn(
                        clip,
                        &TurnSpan {
                            start_ms: turn.start_ms,
                            end_ms: turn.end_ms,
                        },
                        150,
                    )
                    .samples
                })
                .unwrap_or_default()
        })
        .collect())
}

pub fn set_status_blocking(pool: &SqlitePool, note_id: &str, status: &str, error: Option<String>) {
    let _ = tauri::async_runtime::block_on(async {
        sqlx::query(
            "UPDATE notes SET processing_status = ?2, processing_error = ?3,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        )
        .bind(note_id)
        .bind(status)
        .bind(error)
        .execute(pool)
        .await
    });
}

struct FinalArtifacts {
    source_mode: String,
    files: Vec<(String, String)>,
}

fn final_artifacts(pool: &SqlitePool, note_id: &str) -> Result<FinalArtifacts, String> {
    tauri::async_runtime::block_on(async {
        let Some((session_id, source_mode)) = sqlx::query_as::<_, (String, String)>(
            "SELECT id, source_mode FROM recording_sessions
             WHERE note_id = ?1 AND status = 'finished'
             ORDER BY started_at DESC LIMIT 1",
        )
        .bind(note_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        else {
            return Ok(FinalArtifacts {
                source_mode: "microphone-only".to_string(),
                files: Vec::new(),
            });
        };
        let files = sqlx::query_as::<_, (String, String)>(
            "SELECT source, path FROM audio_artifacts
             WHERE session_id = ?1 AND status = 'final'
             ORDER BY source",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(FinalArtifacts { source_mode, files })
    })
}

fn fetch_turn_texts(pool: &SqlitePool, note_id: &str) -> Result<Vec<TurnText>, String> {
    tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (i64, i64, String, Option<String>, String)>(
            "SELECT start_ms, end_ms, source, speaker, text FROM transcript_turns WHERE note_id = ?1 ORDER BY turn_index",
        )
        .bind(note_id)
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(start_ms, end_ms, source, speaker, text)| TurnText {
                    start_ms: start_ms as u64,
                    end_ms: end_ms as u64,
                    source,
                    speaker,
                    text,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
    })
}

fn store_turns(pool: &SqlitePool, note_id: &str, turns: &[TurnText]) -> Result<(), String> {
    // All-or-nothing: a crash or a mid-loop failure must not leave a *prefix*
    // of turns, because retry sees any existing turns and skips
    // re-transcription (run()), which would silently truncate the note.
    tauri::async_runtime::block_on(async {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        for (index, turn) in turns.iter().enumerate() {
            sqlx::query(
                "INSERT INTO transcript_turns (id, note_id, source, turn_index, start_ms, end_ms, speaker, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(note_id)
            .bind(&turn.source)
            .bind(index as i64)
            .bind(turn.start_ms as i64)
            .bind(turn.end_ms as i64)
            .bind(&turn.speaker)
            .bind(&turn.text)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    })
}

fn update_turn_speakers(
    pool: &SqlitePool,
    note_id: &str,
    turns: &[TurnText],
) -> Result<(), String> {
    tauri::async_runtime::block_on(async {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        for (index, turn) in turns.iter().enumerate() {
            sqlx::query(
                "UPDATE transcript_turns SET speaker = ?3
                 WHERE note_id = ?1 AND turn_index = ?2",
            )
            .bind(note_id)
            .bind(index as i64)
            .bind(&turn.speaker)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    })
}

fn fetch_manual_notes(pool: &SqlitePool, note_id: &str) -> Result<String, String> {
    tauri::async_runtime::block_on(async {
        sqlx::query_scalar::<_, String>("SELECT manual_notes FROM notes WHERE id = ?1")
            .bind(note_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn unknown_clusters_get_anonymous_speakers_not_me() {
        let mut anonymous = 0;
        let names = name_clusters(
            &[vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
            &[],
            &mut anonymous,
        );

        assert_eq!(names, vec!["Speaker 1", "Speaker 2"]);
    }

    #[test]
    fn profile_match_uses_profile_name_and_anonymous_for_rest() {
        let profiles = vec![diarize::Profile {
            name: "Me".to_string(),
            embedding: vec![1.0, 0.0],
        }];
        let mut anonymous = 0;
        let names = name_clusters(&[vec![1.0, 0.0], vec![0.0, 1.0]], &profiles, &mut anonymous);

        assert_eq!(names, vec!["Me", "Speaker 1"]);
    }

    #[test]
    fn final_artifacts_preserve_meeting_intent_when_system_audio_was_removed() {
        let pool = tauri::async_runtime::block_on(test_pool());
        let note =
            tauri::async_runtime::block_on(crate::notes::insert_note(&pool, "Meeting")).unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let mic_path = "/tmp/arya-test-microphone.wav";

        tauri::async_runtime::block_on(async {
            sqlx::query(
                "INSERT INTO recording_sessions
                 (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
             VALUES (?1, ?2, 'finished', 'microphone-and-system', 16000, 1,
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            )
            .bind(&session_id)
            .bind(&note.id)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO audio_artifacts
                 (id, session_id, source, path, status, size_bytes, created_at)
             VALUES (?1, ?2, 'microphone', ?3, 'final', 128, '2026-01-01T00:00:00Z')",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&session_id)
            .bind(mic_path)
            .execute(&pool)
            .await
            .unwrap();
        });

        let artifacts = final_artifacts(&pool, &note.id).unwrap();

        assert_eq!(artifacts.source_mode, "microphone-and-system");
        assert_eq!(
            artifacts.files,
            vec![("microphone".to_string(), mic_path.to_string())]
        );
    }

    #[test]
    fn update_turn_speakers_persists_refreshed_labels() {
        let pool = tauri::async_runtime::block_on(test_pool());
        let note =
            tauri::async_runtime::block_on(crate::notes::insert_note(&pool, "Meeting")).unwrap();
        let mut turns = vec![
            TurnText {
                start_ms: 0,
                end_ms: 1_500,
                source: "microphone".to_string(),
                speaker: Some("Me".to_string()),
                text: "hello".to_string(),
            },
            TurnText {
                start_ms: 2_000,
                end_ms: 3_500,
                source: "microphone".to_string(),
                speaker: Some("Me".to_string()),
                text: "hi back".to_string(),
            },
        ];
        store_turns(&pool, &note.id, &turns).unwrap();

        turns[0].speaker = Some("Speaker 1".to_string());
        turns[1].speaker = Some("Speaker 2".to_string());
        update_turn_speakers(&pool, &note.id, &turns).unwrap();

        let refreshed = fetch_turn_texts(&pool, &note.id).unwrap();

        assert_eq!(
            refreshed
                .into_iter()
                .map(|turn| turn.speaker)
                .collect::<Vec<_>>(),
            vec![Some("Speaker 1".to_string()), Some("Speaker 2".to_string())]
        );
    }
}
