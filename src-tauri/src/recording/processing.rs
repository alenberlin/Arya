//! The saved-audio-first processing pipeline: WAV on disk -> turns ->
//! transcript rows -> generated note. Every step is retryable from the
//! artifacts; nothing depends on in-memory state from recording time.

use std::path::Path;

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use super::diarize;
use super::generate::{FallbackGenerator, NoteGenerator, OllamaGenerator, TurnText};
use crate::audio::turns::{detect_turns, slice_turn, TurnConfig};
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
        if artifacts.is_empty() {
            return Err("no audio artifact for note".to_string());
        }
        let mut turns: Vec<TurnText> = Vec::new();
        let mut samples_by_turn: Vec<Vec<f32>> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let multi_source = artifacts.len() > 1;
        for (source, path) in &artifacts {
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
        // Speaker labels only make sense for meetings (multi-source intent).
        if multi_source {
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
        existing_turns
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
    let spec = models::find("whisper-base.en").expect("catalog has base.en");
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
/// clustering, profile matching for names. Mic turns fall back to the best
/// profile match or "Me"; system clusters get profiles or "Speaker N".
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

    let mut extractor =
        sherpa_rs::speaker_id::EmbeddingExtractor::new(sherpa_rs::speaker_id::ExtractorConfig {
            model: model_path.to_string_lossy().to_string(),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;

    let profiles =
        tauri::async_runtime::block_on(diarize::load_profiles(pool)).map_err(|e| e.to_string())?;

    // Embeddings for turns long enough to carry identity, grouped by source.
    let min_samples =
        (crate::speech::AudioClip::SAMPLE_RATE as u64 * diarize::MIN_EMBED_MS / 1000) as usize;
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
                match extractor.compute_speaker_embedding(
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
            // No usable embeddings: mic keeps "Me", system keeps "Them".
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
        let mut cluster_names: Vec<String> = Vec::with_capacity(cluster_centroids.len());
        let mut anonymous = 0usize;
        for centroid in &cluster_centroids {
            match diarize::match_profile(centroid, &profiles, diarize::SAME_SPEAKER_THRESHOLD) {
                Some(name) => cluster_names.push(name.to_string()),
                None if source == "microphone" => cluster_names.push("Me".to_string()),
                None => {
                    anonymous += 1;
                    cluster_names.push(format!("Speaker {anonymous}"));
                }
            }
        }
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

fn final_artifacts(pool: &SqlitePool, note_id: &str) -> Result<Vec<(String, String)>, String> {
    tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (String, String)>(
            "SELECT a.source, a.path FROM audio_artifacts a
             JOIN recording_sessions s ON s.id = a.session_id
             WHERE s.note_id = ?1 AND a.status = 'final'
               AND s.id = (SELECT id FROM recording_sessions
                           WHERE note_id = ?1 AND status = 'finished'
                           ORDER BY started_at DESC LIMIT 1)
             ORDER BY a.source",
        )
        .bind(note_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
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
    tauri::async_runtime::block_on(async {
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
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
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
