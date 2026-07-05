//! Streaming (online) ASR via sherpa-onnx's online transducer recognizer.
//!
//! Unlike the batch [`whisper`](super::whisper) engine, which transcribes a
//! whole clip after the fact, this feeds audio *as it arrives* and finalizes
//! near-instantly at end-of-utterance — the architecture that makes the pill's
//! words appear the moment you stop speaking. It binds the C API directly
//! (`sherpa-rs` exposes only offline recognizers); the online model/stream
//! symbols come from `sherpa_rs_sys`, which is already compiled and linked.
//!
//! Today it drives the pill's live preview (fed cumulative snapshots via
//! [`feed_up_to`](StreamingSpeechEngine::feed_up_to)); the final inserted text
//! still comes from the batch whisper path. Swapping the final transcription to
//! `finalize()` is a follow-up that needs on-device microphone verification.

use std::ffi::{CStr, CString};
use std::mem;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use sherpa_rs::sherpa_rs_sys as sys;

use super::SpeechError;

/// A streaming speech-to-text engine: audio is pushed incrementally and the
/// transcript is available at any time, with a fast finalize at utterance end.
pub trait StreamingSpeechEngine: Send + Sync {
    /// Clear all state to begin a fresh utterance.
    fn reset(&self);
    /// Feed a chunk of mono 16 kHz samples (in `[-1, 1]`), advancing by its len.
    fn accept(&self, samples: &[f32]);
    /// Feed everything up to `samples.len()` that hasn't been fed yet. Safe to
    /// call repeatedly with a growing prefix (a capture snapshot) — each sample
    /// is fed exactly once, in order.
    fn feed_up_to(&self, samples: &[f32]);
    /// The best-effort transcript decoded so far (for a live preview).
    fn partial(&self) -> String;
    /// Signal end of utterance, drain the decoder, and return the final
    /// transcript — resetting internal state for the next utterance.
    fn finalize(&self) -> String;
}

/// sherpa-onnx online transducer (streaming zipformer) engine.
pub struct SherpaStreamingEngine {
    recognizer: *const sys::SherpaOnnxOnlineRecognizer,
    /// The current utterance's stream; swapped for a fresh one on reset/finalize.
    stream: Mutex<*const sys::SherpaOnnxOnlineStream>,
    /// Samples fed into the current stream, so cumulative `feed_up_to` snapshots
    /// only push the new tail. Only touched while holding the stream lock.
    fed: AtomicUsize,
    sample_rate: i32,
}

// The recognizer is immutable after construction and every stream access is
// serialized through the mutex, so sharing across threads is sound.
unsafe impl Send for SherpaStreamingEngine {}
unsafe impl Sync for SherpaStreamingEngine {}

impl SherpaStreamingEngine {
    /// Load a streaming zipformer transducer from its four artifacts (encoder,
    /// decoder, joiner, and the tokens table). Expensive; reuse the engine.
    pub fn load(
        encoder: &Path,
        decoder: &Path,
        joiner: &Path,
        tokens: &Path,
    ) -> Result<Self, SpeechError> {
        let encoder = path_cstring(encoder)?;
        let decoder = path_cstring(decoder)?;
        let joiner = path_cstring(joiner)?;
        let tokens = path_cstring(tokens)?;
        let provider = CString::new("cpu").expect("static str");
        let decoding = CString::new("greedy_search").expect("static str");

        // The C config carries many optional pointer fields; zero-init leaves
        // them null (sherpa treats null as "unset") and we set only what we use.
        // The CStrings above must outlive this call — sherpa copies them during
        // create — so they stay owned in this scope.
        let recognizer = unsafe {
            let mut config: sys::SherpaOnnxOnlineRecognizerConfig = mem::zeroed();
            config.feat_config = sys::SherpaOnnxFeatureConfig {
                sample_rate: SAMPLE_RATE,
                feature_dim: 80,
            };
            config.model_config = sys::SherpaOnnxOnlineModelConfig {
                transducer: sys::SherpaOnnxOnlineTransducerModelConfig {
                    encoder: encoder.as_ptr(),
                    decoder: decoder.as_ptr(),
                    joiner: joiner.as_ptr(),
                },
                num_threads: 2,
                provider: provider.as_ptr(),
                debug: 0,
                tokens: tokens.as_ptr(),
                paraformer: mem::zeroed(),
                zipformer2_ctc: mem::zeroed(),
                model_type: ptr::null(),
                modeling_unit: ptr::null(),
                bpe_vocab: ptr::null(),
                tokens_buf: ptr::null(),
                tokens_buf_size: 0,
                nemo_ctc: mem::zeroed(),
            };
            config.decoding_method = decoding.as_ptr();
            config.max_active_paths = 0;
            config.enable_endpoint = 0;
            sys::SherpaOnnxCreateOnlineRecognizer(&config)
        };
        if recognizer.is_null() {
            return Err(SpeechError::ModelLoad(
                "failed to create sherpa online recognizer".into(),
            ));
        }

        let stream = unsafe { sys::SherpaOnnxCreateOnlineStream(recognizer) };
        if stream.is_null() {
            unsafe { sys::SherpaOnnxDestroyOnlineRecognizer(recognizer) };
            return Err(SpeechError::ModelLoad(
                "failed to create sherpa online stream".into(),
            ));
        }

        Ok(Self {
            recognizer,
            stream: Mutex::new(stream),
            fed: AtomicUsize::new(0),
            sample_rate: SAMPLE_RATE,
        })
    }
}

impl StreamingSpeechEngine for SherpaStreamingEngine {
    fn reset(&self) {
        let mut guard = self.stream.lock().expect("stream lock");
        unsafe {
            sys::SherpaOnnxDestroyOnlineStream(*guard);
            *guard = sys::SherpaOnnxCreateOnlineStream(self.recognizer);
        }
        self.fed.store(0, Ordering::SeqCst);
    }

    fn accept(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let guard = self.stream.lock().expect("stream lock");
        unsafe { feed(self.recognizer, *guard, self.sample_rate, samples) };
        self.fed.fetch_add(samples.len(), Ordering::SeqCst);
    }

    fn feed_up_to(&self, samples: &[f32]) {
        let guard = self.stream.lock().expect("stream lock");
        let start = self.fed.load(Ordering::SeqCst);
        if samples.len() <= start {
            return;
        }
        unsafe { feed(self.recognizer, *guard, self.sample_rate, &samples[start..]) };
        self.fed.store(samples.len(), Ordering::SeqCst);
    }

    fn partial(&self) -> String {
        let guard = self.stream.lock().expect("stream lock");
        unsafe { result_text(self.recognizer, *guard) }
    }

    fn finalize(&self) -> String {
        let mut guard = self.stream.lock().expect("stream lock");
        let text = unsafe {
            sys::SherpaOnnxOnlineStreamInputFinished(*guard);
            while sys::SherpaOnnxIsOnlineStreamReady(self.recognizer, *guard) == 1 {
                sys::SherpaOnnxDecodeOnlineStream(self.recognizer, *guard);
            }
            let text = result_text(self.recognizer, *guard);
            // A stream can't accept audio after InputFinished, so start a fresh
            // one for the next utterance.
            sys::SherpaOnnxDestroyOnlineStream(*guard);
            *guard = sys::SherpaOnnxCreateOnlineStream(self.recognizer);
            text
        };
        self.fed.store(0, Ordering::SeqCst);
        text
    }
}

impl Drop for SherpaStreamingEngine {
    fn drop(&mut self) {
        unsafe {
            sys::SherpaOnnxDestroyOnlineStream(*self.stream.lock().expect("stream lock"));
            sys::SherpaOnnxDestroyOnlineRecognizer(self.recognizer);
        }
    }
}

const SAMPLE_RATE: i32 = 16_000;

/// Process-wide cache: loading a streaming model is expensive, and a single
/// engine (its stream reset per utterance) serves every dictation.
static CACHE: OnceLock<Mutex<Option<Arc<SherpaStreamingEngine>>>> = OnceLock::new();

/// The cached streaming engine, loading it from the given artifacts on first
/// use. Subsequent calls ignore the paths and return the same engine.
pub fn cached(
    encoder: &Path,
    decoder: &Path,
    joiner: &Path,
    tokens: &Path,
) -> Result<Arc<SherpaStreamingEngine>, SpeechError> {
    let cell = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().expect("streaming cache lock");
    if let Some(engine) = guard.as_ref() {
        return Ok(engine.clone());
    }
    let engine = Arc::new(SherpaStreamingEngine::load(
        encoder, decoder, joiner, tokens,
    )?);
    *guard = Some(engine.clone());
    Ok(engine)
}

/// The cached engine if it's already loaded, without loading it. Lets the
/// dictation hot path decide the backend synchronously and cheaply.
pub fn current() -> Option<Arc<SherpaStreamingEngine>> {
    CACHE.get()?.lock().expect("streaming cache lock").clone()
}

/// Feed samples into `stream` and run the decoder until it's caught up. Caller
/// must hold the stream lock.
unsafe fn feed(
    recognizer: *const sys::SherpaOnnxOnlineRecognizer,
    stream: *const sys::SherpaOnnxOnlineStream,
    sample_rate: i32,
    samples: &[f32],
) {
    sys::SherpaOnnxOnlineStreamAcceptWaveform(
        stream,
        sample_rate,
        samples.as_ptr(),
        samples.len() as i32,
    );
    while sys::SherpaOnnxIsOnlineStreamReady(recognizer, stream) == 1 {
        sys::SherpaOnnxDecodeOnlineStream(recognizer, stream);
    }
}

/// Read the recognizer's current result text and free the C result.
unsafe fn result_text(
    recognizer: *const sys::SherpaOnnxOnlineRecognizer,
    stream: *const sys::SherpaOnnxOnlineStream,
) -> String {
    let result = sys::SherpaOnnxGetOnlineStreamResult(recognizer, stream);
    if result.is_null() {
        return String::new();
    }
    let text = if (*result).text.is_null() {
        String::new()
    } else {
        CStr::from_ptr((*result).text)
            .to_string_lossy()
            .into_owned()
    };
    sys::SherpaOnnxDestroyOnlineRecognizerResult(result);
    text
}

fn path_cstring(path: &Path) -> Result<CString, SpeechError> {
    let text = path
        .to_str()
        .ok_or_else(|| SpeechError::ModelLoad("model path is not valid UTF-8".into()))?;
    CString::new(text).map_err(|_| SpeechError::ModelLoad("model path contains a NUL byte".into()))
}
