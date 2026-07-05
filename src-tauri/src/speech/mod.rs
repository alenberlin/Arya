//! On-device speech-to-text.
//!
//! Everything behind [`SpeechEngine`] so the backend is swappable: the
//! current implementation is whisper.cpp (Metal-accelerated via whisper-rs);
//! sherpa-onnx/Parakeet remains a candidate second backend and lands with
//! diarization (M6) if adopted.

pub mod engine_cache;
pub mod models;
pub mod streaming;
pub mod wav;
pub mod wer;
pub mod whisper;

use serde::Serialize;

/// Mono 16 kHz PCM audio, the canonical inference format.
#[derive(Debug, Clone)]
pub struct AudioClip {
    pub samples: Vec<f32>,
}

impl AudioClip {
    pub const SAMPLE_RATE: u32 = 16_000;

    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / Self::SAMPLE_RATE as f64
    }
}

/// One recognized segment with millisecond timestamps.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// A full transcription result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
}

/// Options for one transcription call.
#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// ISO 639-1 language hint; `None` lets the engine detect.
    pub language: Option<String>,
}

/// Errors from the speech subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SpeechError {
    #[error("failed to load speech model: {0}")]
    ModelLoad(String),
    #[error("inference failed: {0}")]
    Inference(String),
    #[error("invalid audio: {0}")]
    InvalidAudio(String),
    #[error("model download failed: {0}")]
    Download(String),
    #[error("checksum mismatch for {name}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A speech-to-text backend. Implementations are expensive to construct
/// (model load) and cheap to call repeatedly; calls are blocking and belong
/// on a blocking thread when driven from async code.
pub trait SpeechEngine: Send + Sync {
    fn transcribe(
        &self,
        audio: &AudioClip,
        options: &TranscribeOptions,
    ) -> Result<Transcript, SpeechError>;
}
