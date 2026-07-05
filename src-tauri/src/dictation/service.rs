//! The dictation orchestrator: ties hotkey edges to capture, ASR, cleanup,
//! paste, and history.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Instant;

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use super::capture_worker::CaptureWorker;
use super::profiles::{self, AppProfile};
use super::settings::DictationSettings;
use super::{DictationEvent, DictationState, MIN_HOLD_MS};
use crate::cleanup::mechanical::{MechanicalCleaner, RawCleaner};
use crate::cleanup::ollama::OllamaCleaner;
use crate::cleanup::{
    CleanupRequest, DictationStyle, DictionaryEntry, Polish, TargetContext, TextCleaner,
};
use crate::paste::{self, TargetApp};
use crate::speech::streaming::{self, SherpaStreamingEngine, StreamingSpeechEngine};
use crate::speech::whisper::WhisperEngine;
use crate::speech::{engine_cache, models, AudioClip, SpeechEngine, TranscribeOptions};
use crate::translate;

/// Emitted to the pill at `begin`: where the text will land and the resolved
/// profile (polish + tone) for that app, plus whether it's pinned.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetEvent {
    name: Option<String>,
    bundle_id: Option<String>,
    polish: Polish,
    style: DictationStyle,
    pinned: bool,
}

pub struct DictationService {
    worker: CaptureWorker,
    settings: Mutex<DictationSettings>,
    recording_since: Mutex<Option<Instant>>,
    /// Captured at hotkey-down: the app the user was in, i.e. the paste target.
    target_app: Mutex<Option<TargetApp>>,
    busy: AtomicBool,
    /// Bumped on every `begin`; the live-transcription ticker exits when the
    /// generation it was spawned for is superseded, so a stale ticker can't
    /// stream partials into a later recording.
    recording_gen: AtomicU64,
    /// One-off polish override for the current dictation, set from the pill and
    /// consumed by the pipeline; cleared on each new recording.
    session_polish: Mutex<Option<Polish>>,
    /// Pinned per-app profiles (bundle id → profile).
    app_profiles: Mutex<profiles::Overrides>,
    /// The streaming engine chosen for the current dictation — `Some` when this
    /// dictation will transcribe via streaming rather than whisper.
    active_stream: Mutex<Option<Arc<SherpaStreamingEngine>>>,
    /// The live-feed ticker's join handle (streaming only), so finalize and the
    /// next `begin` can join it before touching the shared cached engine.
    partial_handle: Mutex<Option<JoinHandle<()>>>,
}

/// How often the live-transcription ticker re-transcribes the audio so far.
const PARTIAL_INTERVAL_MS: u64 = 1200;
/// Don't attempt a partial until there's at least this much audio.
const PARTIAL_MIN_SECS: f64 = 0.4;

static LEVEL_TICKER: OnceLock<()> = OnceLock::new();

/// Bumped on every `show_hud`. A scheduled hide only fires if the epoch is
/// unchanged when it wakes — so a rapid abort→show (the first half of a
/// double-tap, or a new dictation started before the last one's hide delay
/// elapses) can't hide a pill that a newer show just brought up.
static HUD_EPOCH: AtomicU64 = AtomicU64::new(0);

impl DictationService {
    pub fn new(settings: DictationSettings, app_profiles: profiles::Overrides) -> Self {
        Self {
            worker: CaptureWorker::spawn(),
            settings: Mutex::new(settings),
            recording_since: Mutex::new(None),
            target_app: Mutex::new(None),
            busy: AtomicBool::new(false),
            recording_gen: AtomicU64::new(0),
            session_polish: Mutex::new(None),
            app_profiles: Mutex::new(app_profiles),
            active_stream: Mutex::new(None),
            partial_handle: Mutex::new(None),
        }
    }

    pub fn settings(&self) -> DictationSettings {
        self.settings.lock().expect("settings lock").clone()
    }

    pub fn update_settings(&self, settings: DictationSettings) {
        *self.settings.lock().expect("settings lock") = settings;
    }

    /// Set a one-off polish level for the current dictation (from the pill).
    pub fn set_session_polish(&self, polish: Polish) {
        *self.session_polish.lock().expect("polish lock") = Some(polish);
    }

    /// Resolve the effective profile (pin → email default → global) for a
    /// target app.
    pub fn resolve_for(&self, bundle_id: Option<&str>) -> AppProfile {
        let settings = self.settings();
        let pinned = self.app_profiles.lock().expect("profiles lock");
        profiles::resolve(bundle_id, &settings, &pinned)
    }

    /// Pin `polish` (with the current resolved tone) as the default for the
    /// current target app. Returns the updated set to persist, or None when
    /// there's no target bundle id.
    pub fn pin_current_app(&self, polish: Polish) -> Option<profiles::Overrides> {
        let bundle = self
            .target_app
            .lock()
            .expect("target lock")
            .clone()?
            .bundle_id?;
        let style = self.resolve_for(Some(&bundle)).style;
        let mut pinned = self.app_profiles.lock().expect("profiles lock");
        pinned.insert(bundle, AppProfile { polish, style });
        Some(pinned.clone())
    }

    /// Remove the pin for the current target app. Returns the updated set, or
    /// None when nothing was pinned.
    pub fn unpin_current_app(&self) -> Option<profiles::Overrides> {
        let bundle = self
            .target_app
            .lock()
            .expect("target lock")
            .clone()?
            .bundle_id?;
        let mut pinned = self.app_profiles.lock().expect("profiles lock");
        pinned.remove(&bundle).map(|_| pinned.clone())
    }

    pub fn is_recording(&self) -> bool {
        self.recording_since.lock().expect("since lock").is_some()
    }

    /// Hotkey down (or toggle on): begin capturing.
    pub fn begin(self: &Arc<Self>, app: &AppHandle) {
        if self.is_recording() || self.busy.load(Ordering::SeqCst) {
            return;
        }
        // A prior streaming dictation's feed ticker must be fully stopped before
        // we reset the shared engine for this one.
        self.join_partial_ticker();
        let settings = self.settings();
        let target = paste::frontmost_app();
        // Resolve the per-app profile and tell the pill where the text will land
        // and how it'll be written (polish + tone), plus whether it's pinned.
        let profile = self.resolve_for(target.bundle_id.as_deref());
        let pinned = target.bundle_id.as_deref().is_some_and(|id| {
            self.app_profiles
                .lock()
                .expect("profiles lock")
                .contains_key(id)
        });
        let _ = app.emit(
            "dictation:target",
            &TargetEvent {
                name: target.name.clone(),
                bundle_id: target.bundle_id.clone(),
                polish: profile.polish,
                style: profile.style,
                pinned,
            },
        );
        // Fresh dictation: drop any stale one-off polish.
        *self.session_polish.lock().expect("polish lock") = None;
        *self.target_app.lock().expect("target lock") = Some(target);
        match self.worker.start(settings.microphone.clone()) {
            Ok(()) => {
                let gen = self.recording_gen.fetch_add(1, Ordering::SeqCst) + 1;
                *self.recording_since.lock().expect("since lock") = Some(Instant::now());
                emit_state(app, DictationState::Recording, None, None);
                show_hud(app);
                self.spawn_level_ticker(app.clone());
                self.spawn_partials(app, gen, &settings);
            }
            Err(message) => {
                emit_state(app, DictationState::Error, Some(message), None);
            }
        }
    }

    /// Decide the live-preview backend for this dictation and start its ticker.
    /// Streaming is used only when it's already loaded (a cheap, synchronous
    /// check) so `finish` agrees with `begin`; otherwise whisper drives the
    /// preview and — if streaming is enabled but not yet loaded — it's warmed in
    /// the background for next time.
    fn spawn_partials(
        self: &Arc<Self>,
        app: &AppHandle,
        generation: u64,
        settings: &DictationSettings,
    ) {
        match settings.streaming.then(streaming::current).flatten() {
            Some(engine) => {
                *self.active_stream.lock().expect("stream lock") = Some(engine.clone());
                let handle = self.spawn_streaming_ticker(app.clone(), generation, engine);
                *self.partial_handle.lock().expect("handle lock") = Some(handle);
            }
            None => {
                *self.active_stream.lock().expect("stream lock") = None;
                self.spawn_whisper_ticker(app.clone(), generation, settings.clone());
                if settings.streaming {
                    spawn_streaming_prepare(app.clone());
                }
            }
        }
    }

    /// Spawn the streaming live-feed ticker and return its handle, so finalize
    /// can join it before touching the shared engine.
    fn spawn_streaming_ticker(
        self: &Arc<Self>,
        app: AppHandle,
        generation: u64,
        engine: Arc<SherpaStreamingEngine>,
    ) -> JoinHandle<()> {
        let service = Arc::clone(self);
        std::thread::Builder::new()
            .name("arya-dictation-partial".into())
            .spawn(move || service.run_streaming_partials(&app, generation, &engine))
            .expect("spawn streaming ticker")
    }

    /// Spawn the whisper re-transcribe ticker (fallback preview). Detached: it
    /// shares no mutable state with finalize, so it needn't be joined.
    fn spawn_whisper_ticker(
        self: &Arc<Self>,
        app: AppHandle,
        generation: u64,
        settings: DictationSettings,
    ) {
        let service = Arc::clone(self);
        std::thread::Builder::new()
            .name("arya-dictation-partial".into())
            .spawn(move || service.run_whisper_partials(&app, generation, &settings))
            .expect("spawn whisper ticker");
    }

    /// Join and clear the streaming feed ticker so nothing is still touching the
    /// shared engine. No-op for the whisper path (its handle is never stored).
    fn join_partial_ticker(&self) {
        let handle = self.partial_handle.lock().expect("handle lock").take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    /// Live preview via the streaming engine: feed each capture snapshot's new
    /// tail and emit the running transcript.
    fn run_streaming_partials(
        &self,
        app: &AppHandle,
        generation: u64,
        engine: &SherpaStreamingEngine,
    ) {
        const STREAM_INTERVAL_MS: u64 = 120;
        engine.reset();
        while self.recording_gen.load(Ordering::SeqCst) == generation && self.is_recording() {
            std::thread::sleep(std::time::Duration::from_millis(STREAM_INTERVAL_MS));
            if self.recording_gen.load(Ordering::SeqCst) != generation || !self.is_recording() {
                break;
            }
            let Ok(clip) = self.worker.snapshot() else {
                continue;
            };
            engine.feed_up_to(&clip.samples);
            let text = engine.partial();
            let text = text.trim();
            // Guard again: don't paint a partial onto a newer session.
            if !text.is_empty() && self.recording_gen.load(Ordering::SeqCst) == generation {
                let _ = app.emit("dictation:partial", text.to_string());
            }
        }
    }

    /// Live preview via whisper: re-transcribe the audio-so-far on an interval.
    fn run_whisper_partials(&self, app: &AppHandle, generation: u64, settings: &DictationSettings) {
        let Ok(engine) = self.ensure_engine(app, settings) else {
            return;
        };
        let options = TranscribeOptions {
            language: settings.language.clone(),
        };
        while self.recording_gen.load(Ordering::SeqCst) == generation && self.is_recording() {
            std::thread::sleep(std::time::Duration::from_millis(PARTIAL_INTERVAL_MS));
            if self.recording_gen.load(Ordering::SeqCst) != generation || !self.is_recording() {
                break;
            }
            let Ok(clip) = self.worker.snapshot() else {
                continue;
            };
            if clip.duration_secs() < PARTIAL_MIN_SECS {
                continue;
            }
            if let Ok(transcript) = engine.transcribe(&clip, &options) {
                let text = transcript.text.trim().to_string();
                if !text.is_empty() && self.recording_gen.load(Ordering::SeqCst) == generation {
                    let _ = app.emit("dictation:partial", text);
                }
            }
        }
    }

    /// Discard the current capture without transcribing (a too-short tap).
    pub fn abort_recording(&self, app: &AppHandle) {
        if self
            .recording_since
            .lock()
            .expect("since lock")
            .take()
            .is_some()
        {
            self.worker.abort();
            // Stop the feed ticker and release the engine for this dictation.
            self.join_partial_ticker();
            *self.active_stream.lock().expect("stream lock") = None;
            emit_state(app, DictationState::Idle, None, None);
            hide_hud_soon(app, 250);
        }
    }

    /// Hotkey up (or toggle off): stop, transcribe, clean, paste.
    pub fn finish(self: &Arc<Self>, app: &AppHandle, pool: SqlitePool) {
        let started = match self.recording_since.lock().expect("since lock").take() {
            Some(instant) => instant,
            None => return,
        };
        // Take the backend chosen at `begin` and the feed ticker's handle; every
        // exit path below releases them so the shared engine is left idle.
        let stream = self.active_stream.lock().expect("stream lock").take();
        let handle = self.partial_handle.lock().expect("handle lock").take();

        if started.elapsed().as_millis() < MIN_HOLD_MS {
            // Graze: too short to be intentional.
            self.worker.abort();
            join_ticker(handle);
            emit_state(app, DictationState::Idle, None, None);
            hide_hud_soon(app, 400);
            return;
        }
        let clip = match self.worker.stop() {
            Ok(clip) => clip,
            Err(message) => {
                join_ticker(handle);
                emit_state(app, DictationState::Error, Some(message), None);
                hide_hud_soon(app, 1500);
                return;
            }
        };
        if clip.duration_secs() < 0.3 {
            join_ticker(handle);
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
                // The feed ticker must be fully stopped before we touch the
                // shared engine to finalize.
                join_ticker(handle);
                let result = match stream {
                    Some(engine) => service.run_streaming_pipeline(&app, &pool, clip, &engine),
                    None => service.run_pipeline(&app, &pool, clip),
                };
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

    /// Transcribe a clip with whisper. Returns the trimmed text (possibly empty)
    /// and the inference time in ms.
    fn transcribe_whisper(
        &self,
        app: &AppHandle,
        settings: &DictationSettings,
        clip: &AudioClip,
    ) -> Result<(String, i64), String> {
        let engine = self.ensure_engine(app, settings)?;
        let options = TranscribeOptions {
            language: settings.language.clone(),
        };
        let started = Instant::now();
        let transcript = engine
            .transcribe(clip, &options)
            .map_err(|e| e.to_string())?;
        let asr_ms = started.elapsed().as_millis() as i64;
        Ok((transcript.text.trim().to_string(), asr_ms))
    }

    /// The common tail: clean the raw transcript (per-app tone + one-off polish),
    /// record history, and paste. History is written before the paste so a
    /// failed paste never loses the user's words.
    fn deliver(
        &self,
        app: &AppHandle,
        pool: &SqlitePool,
        clip: &AudioClip,
        raw: String,
        asr_ms: i64,
    ) -> Result<String, String> {
        let settings = self.settings();
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
        // Per-app profile (pin → email default → global) sets the tone; a one-off
        // pill change overrides the polish for just this dictation.
        let profile = self.resolve_for(target.bundle_id.as_deref());
        let polish = self
            .session_polish
            .lock()
            .expect("polish lock")
            .take()
            .unwrap_or(profile.polish);
        let context = if paste::is_email_app(target.bundle_id.as_deref()) {
            TargetContext::Email
        } else {
            TargetContext::Generic
        };
        let request = CleanupRequest {
            raw: raw.clone(),
            style: profile.style,
            context,
            dictionary,
        };
        let clean = match polish {
            Polish::Raw => RawCleaner.clean(&request),
            Polish::Clean => MechanicalCleaner.clean(&request),
            Polish::Polished => match settings.cleanup_model.as_deref() {
                Some(model) => OllamaCleaner::new(
                    settings.ollama_url.clone(),
                    model,
                    std::time::Duration::from_secs(20),
                )
                .clean(&request),
                // Polished needs a configured local model; without one, fall
                // back to deterministic cleanup rather than silently no-op.
                None => MechanicalCleaner.clean(&request),
            },
        };

        // Optionally translate the cleaned text; on any failure keep the source
        // so a dictation is never lost.
        let (final_text, translated, target_lang) = match settings
            .translate
            .as_deref()
            .filter(|lang| !lang.is_empty())
        {
            Some(lang) => {
                let model = settings
                    .translate_model
                    .as_deref()
                    .or(settings.cleanup_model.as_deref())
                    .unwrap_or(translate::DEFAULT_LOCAL_MODEL);
                let translated = translate::make_translator(
                    settings.translate_provider,
                    &settings.ollama_url,
                    model,
                )
                .and_then(|t| t.translate(&clean, lang));
                match translated {
                    Some(t) => (t.clone(), Some(t), Some(lang.to_string())),
                    None => (clean.clone(), None, None),
                }
            }
            None => (clean.clone(), None, None),
        };

        insert_history_blocking(
            pool,
            &raw,
            &clean,
            translated.as_deref(),
            target_lang.as_deref(),
            target.bundle_id.as_deref(),
            (clip.duration_secs() * 1000.0) as i64,
            asr_ms,
        )?;

        emit_state(app, DictationState::Pasting, None, None);
        paste::paste_text(&final_text).map_err(|e| e.to_string())?;
        Ok(final_text)
    }

    /// The whisper pipeline: batch-transcribe the whole clip, then deliver.
    fn run_pipeline(
        &self,
        app: &AppHandle,
        pool: &SqlitePool,
        clip: AudioClip,
    ) -> Result<String, String> {
        let settings = self.settings();
        let (raw, asr_ms) = self.transcribe_whisper(app, &settings, &clip)?;
        if raw.is_empty() {
            return Err("nothing recognized".into());
        }
        self.deliver(app, pool, &clip, raw, asr_ms)
    }

    /// The streaming pipeline: the live ticker has already fed most of the audio,
    /// so feed the final tail and finalize (fast), then deliver. Falls back to
    /// whisper if streaming yields nothing, so a dictation is never dropped.
    fn run_streaming_pipeline(
        &self,
        app: &AppHandle,
        pool: &SqlitePool,
        clip: AudioClip,
        engine: &SherpaStreamingEngine,
    ) -> Result<String, String> {
        let started = Instant::now();
        engine.feed_up_to(&clip.samples);
        let raw = engine.finalize();
        let asr_ms = started.elapsed().as_millis() as i64;
        let raw = raw.trim().to_string();
        if !raw.is_empty() {
            return self.deliver(app, pool, &clip, raw, asr_ms);
        }
        // Streaming produced nothing (silence or a transient) — fall back to the
        // proven whisper path.
        let settings = self.settings();
        let (raw, asr_ms) = self.transcribe_whisper(app, &settings, &clip)?;
        if raw.is_empty() {
            return Err("nothing recognized".into());
        }
        self.deliver(app, pool, &clip, raw, asr_ms)
    }

    fn ensure_engine(
        &self,
        app: &AppHandle,
        settings: &DictationSettings,
    ) -> Result<Arc<WhisperEngine>, String> {
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
        // Share the process-wide engine cache with note transcription so a
        // model used by both is loaded into memory only once.
        engine_cache::get_or_load(&target).map_err(|e| e.to_string())
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

/// Join a live-feed ticker handle if present (streaming only), ignoring panics.
fn join_ticker(handle: Option<JoinHandle<()>>) {
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

/// Ensure the streaming model is present and the engine is loaded into the
/// cache, returning it. Blocking (downloads + model load); keep off the UI and
/// audio threads.
fn load_streaming_blocking(app: &AppHandle) -> Result<Arc<SherpaStreamingEngine>, String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let paths = tauri::async_runtime::block_on(models::ensure_streaming_model(&models_dir))
        .map_err(|e| e.to_string())?;
    streaming::cached(&paths.encoder, &paths.decoder, &paths.joiner, &paths.tokens)
        .map_err(|e| e.to_string())
}

/// Warm the streaming engine in the background (download + load) so a later
/// dictation can use it. Best-effort.
fn spawn_streaming_prepare(app: AppHandle) {
    std::thread::Builder::new()
        .name("arya-streaming-prepare".into())
        .spawn(move || {
            let _ = load_streaming_blocking(&app);
        })
        .expect("spawn streaming prepare");
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
    HUD_EPOCH.fetch_add(1, Ordering::SeqCst);
    if let Some(hud) = app.get_webview_window("hud") {
        let _ = hud.set_always_on_top(true);
        let _ = hud.show();
    }
}

fn hide_hud_soon(app: &AppHandle, delay_ms: u64) {
    let epoch = HUD_EPOCH.load(Ordering::SeqCst);
    if let Some(hud) = app.get_webview_window("hud") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            // A newer show since we were scheduled means the pill is wanted again
            // (double-tap, or a fresh dictation) — leave it up.
            if HUD_EPOCH.load(Ordering::SeqCst) == epoch {
                let _ = hud.hide();
            }
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
    translated: Option<&str>,
    target_lang: Option<&str>,
    bundle_id: Option<&str>,
    duration_ms: i64,
    asr_ms: i64,
) -> Result<(), String> {
    tauri::async_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO dictation_history
                 (id, raw_text, clean_text, translated_text, target_lang, app_bundle_id,
                  duration_ms, asr_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(raw)
        .bind(clean)
        .bind(translated)
        .bind(target_lang)
        .bind(bundle_id)
        .bind(duration_ms)
        .bind(asr_ms)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
}
