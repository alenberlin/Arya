//! The dictation orchestrator: ties hotkey edges to capture, ASR, cleanup,
//! paste, and history.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use super::capture_worker::CaptureWorker;
use super::settings::DictationSettings;
use super::{DictationEvent, DictationState, MIN_HOLD_MS};
use crate::cleanup::mechanical::MechanicalCleaner;
use crate::cleanup::ollama::OllamaCleaner;
use crate::cleanup::{CleanupRequest, DictionaryEntry, TargetContext, TextCleaner};
use crate::paste::{self, TargetApp};
use crate::speech::whisper::WhisperEngine;
use crate::speech::{models, SpeechEngine, TranscribeOptions};

pub struct DictationService {
    worker: CaptureWorker,
    settings: Mutex<DictationSettings>,
    /// (model_id, engine) — reloaded when the configured model changes.
    engine: Mutex<Option<(String, Arc<WhisperEngine>)>>,
    recording_since: Mutex<Option<Instant>>,
    /// Captured at hotkey-down: the app the user was in, i.e. the paste target.
    target_app: Mutex<Option<TargetApp>>,
    busy: AtomicBool,
}

static LEVEL_TICKER: OnceLock<()> = OnceLock::new();

impl DictationService {
    pub fn new(settings: DictationSettings) -> Self {
        Self {
            worker: CaptureWorker::spawn(),
            settings: Mutex::new(settings),
            engine: Mutex::new(None),
            recording_since: Mutex::new(None),
            target_app: Mutex::new(None),
            busy: AtomicBool::new(false),
        }
    }

    pub fn settings(&self) -> DictationSettings {
        self.settings.lock().expect("settings lock").clone()
    }

    pub fn update_settings(&self, settings: DictationSettings) {
        *self.settings.lock().expect("settings lock") = settings;
    }

    pub fn is_recording(&self) -> bool {
        self.recording_since.lock().expect("since lock").is_some()
    }

    /// Hotkey down (or toggle on): begin capturing.
    pub fn begin(&self, app: &AppHandle) {
        if self.is_recording() || self.busy.load(Ordering::SeqCst) {
            return;
        }
        let settings = self.settings();
        *self.target_app.lock().expect("target lock") = Some(paste::frontmost_app());
        match self.worker.start(settings.microphone.clone()) {
            Ok(()) => {
                *self.recording_since.lock().expect("since lock") = Some(Instant::now());
                emit_state(app, DictationState::Recording, None, None);
                show_hud(app);
                self.spawn_level_ticker(app.clone());
            }
            Err(message) => {
                emit_state(app, DictationState::Error, Some(message), None);
            }
        }
    }

    /// Hotkey up (or toggle off): stop, transcribe, clean, paste.
    pub fn finish(self: &Arc<Self>, app: &AppHandle, pool: SqlitePool) {
        let started = match self.recording_since.lock().expect("since lock").take() {
            Some(instant) => instant,
            None => return,
        };
        if started.elapsed().as_millis() < MIN_HOLD_MS {
            // Graze: too short to be intentional.
            self.worker.abort();
            emit_state(app, DictationState::Idle, None, None);
            hide_hud_soon(app, 400);
            return;
        }
        let clip = match self.worker.stop() {
            Ok(clip) => clip,
            Err(message) => {
                emit_state(app, DictationState::Error, Some(message), None);
                hide_hud_soon(app, 1500);
                return;
            }
        };
        if clip.duration_secs() < 0.3 {
            emit_state(app, DictationState::Idle, None, None);
            hide_hud_soon(app, 400);
            return;
        }

        self.busy.store(true, Ordering::SeqCst);
        emit_state(app, DictationState::Processing, None, None);
        let service = Arc::clone(self);
        let app = app.clone();
        std::thread::Builder::new()
            .name("arya-dictation-pipeline".into())
            .spawn(move || {
                let result = service.run_pipeline(&app, &pool, clip);
                service.busy.store(false, Ordering::SeqCst);
                match result {
                    Ok(text) => {
                        emit_state(&app, DictationState::Idle, None, Some(text));
                        hide_hud_soon(&app, 900);
                    }
                    Err(message) => {
                        emit_state(&app, DictationState::Error, Some(message), None);
                        hide_hud_soon(&app, 2500);
                    }
                }
            })
            .expect("spawn dictation pipeline");
    }

    fn run_pipeline(
        &self,
        app: &AppHandle,
        pool: &SqlitePool,
        clip: crate::speech::AudioClip,
    ) -> Result<String, String> {
        let settings = self.settings();
        let engine = self.ensure_engine(app, &settings)?;
        let options = TranscribeOptions {
            language: settings.language.clone(),
        };
        let started = Instant::now();
        let transcript = engine
            .transcribe(&clip, &options)
            .map_err(|e| e.to_string())?;
        let asr_ms = started.elapsed().as_millis() as i64;
        let raw = transcript.text.trim().to_string();
        if raw.is_empty() {
            return Err("nothing recognized".into());
        }

        let dictionary = fetch_dictionary_blocking(pool)?;
        let target = self
            .target_app
            .lock()
            .expect("target lock")
            .clone()
            .unwrap_or(TargetApp {
                bundle_id: None,
                name: None,
            });
        let context = if paste::is_email_app(target.bundle_id.as_deref()) {
            TargetContext::Email
        } else {
            TargetContext::Generic
        };
        let request = CleanupRequest {
            raw: raw.clone(),
            style: settings.style,
            context,
            dictionary,
        };
        let clean = match settings.cleanup_model.as_deref() {
            Some(model) => OllamaCleaner::new(
                settings.ollama_url.clone(),
                model,
                std::time::Duration::from_secs(20),
            )
            .clean(&request),
            None => MechanicalCleaner.clean(&request),
        };

        // History first: a failed paste must never lose the user's words.
        insert_history_blocking(
            pool,
            &raw,
            &clean,
            target.bundle_id.as_deref(),
            (clip.duration_secs() * 1000.0) as i64,
            asr_ms,
        )?;

        emit_state(app, DictationState::Pasting, None, None);
        paste::paste_text(&clean).map_err(|e| e.to_string())?;
        Ok(clean)
    }

    fn ensure_engine(
        &self,
        app: &AppHandle,
        settings: &DictationSettings,
    ) -> Result<Arc<WhisperEngine>, String> {
        {
            let guard = self.engine.lock().expect("engine lock");
            if let Some((id, engine)) = guard.as_ref() {
                if *id == settings.speech_model {
                    return Ok(Arc::clone(engine));
                }
            }
        }
        let spec = models::find(&settings.speech_model)
            .ok_or_else(|| format!("unknown speech model {}", settings.speech_model))?;
        let models_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("models");
        let target = models_dir.join(spec.file_name);
        if !target.exists() {
            emit_state(app, DictationState::PreparingModel, None, None);
            tauri::async_runtime::block_on(models::ensure_model(spec, &models_dir))
                .map_err(|e| e.to_string())?;
        }
        let engine = Arc::new(WhisperEngine::load(&target).map_err(|e| e.to_string())?);
        *self.engine.lock().expect("engine lock") =
            Some((settings.speech_model.clone(), Arc::clone(&engine)));
        Ok(engine)
    }

    /// One global ticker publishes live levels to the HUD while recording.
    fn spawn_level_ticker(&self, app: AppHandle) {
        let worker = self.worker.clone();
        LEVEL_TICKER.get_or_init(move || {
            std::thread::Builder::new()
                .name("arya-level-ticker".into())
                .spawn(move || loop {
                    let level = worker.level();
                    let _ = app.emit("dictation:level", level);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                })
                .expect("spawn level ticker");
        });
    }
}

fn emit_state(
    app: &AppHandle,
    state: DictationState,
    message: Option<String>,
    text: Option<String>,
) {
    #[cfg(debug_assertions)]
    eprintln!("dictation state: {state:?} message={message:?} text={text:?}");
    let _ = app.emit(
        "dictation:state",
        DictationEvent {
            state,
            message,
            text,
        },
    );
}

fn show_hud(app: &AppHandle) {
    if let Some(hud) = app.get_webview_window("hud") {
        let _ = hud.show();
    }
}

fn hide_hud_soon(app: &AppHandle, delay_ms: u64) {
    if let Some(hud) = app.get_webview_window("hud") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            let _ = hud.hide();
        });
    }
}

fn fetch_dictionary_blocking(pool: &SqlitePool) -> Result<Vec<DictionaryEntry>, String> {
    tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (String, String)>(
            "SELECT pattern, replacement FROM dictionary_entries ORDER BY pattern",
        )
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(pattern, replacement)| DictionaryEntry {
                    pattern,
                    replacement,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
    })
}

#[allow(clippy::too_many_arguments)]
fn insert_history_blocking(
    pool: &SqlitePool,
    raw: &str,
    clean: &str,
    bundle_id: Option<&str>,
    duration_ms: i64,
    asr_ms: i64,
) -> Result<(), String> {
    tauri::async_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO dictation_history
                 (id, raw_text, clean_text, app_bundle_id, duration_ms, asr_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(raw)
        .bind(clean)
        .bind(bundle_id)
        .bind(duration_ms)
        .bind(asr_ms)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
}
