//! whisper.cpp backend via whisper-rs (Metal-accelerated on Apple Silicon).

use std::path::Path;
use std::sync::Mutex;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use super::{AudioClip, Segment, SpeechEngine, SpeechError, TranscribeOptions, Transcript};

pub struct WhisperEngine {
    /// One decode state, created at load and reused for every transcription.
    ///
    /// whisper.cpp clears its result buffers at the start of each `whisper_full`
    /// call, so a single state serves all clips independently. Reusing it avoids
    /// reallocating the model's Metal compute buffers on every call — the note
    /// pipeline transcribes many short chunks per recording, and a fresh state
    /// per chunk churned those buffers needlessly. `full` needs `&mut self`; the
    /// mutex serializes calls, which is free because GPU transcription is
    /// already serial.
    ///
    /// The state owns an `Arc` to the loaded model, so holding it here keeps the
    /// model resident for the engine's lifetime — no separate context field is
    /// needed.
    state: Mutex<WhisperState>,
}

impl WhisperEngine {
    /// Loads a ggml/gguf whisper model from disk. Expensive; hold onto the
    /// engine and reuse it.
    pub fn load(model_path: &Path) -> Result<Self, SpeechError> {
        let path = model_path
            .to_str()
            .ok_or_else(|| SpeechError::ModelLoad("model path is not valid UTF-8".into()))?;
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
        let state = context
            .create_state()
            .map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
        Ok(Self {
            state: Mutex::new(state),
        })
    }
}

impl SpeechEngine for WhisperEngine {
    fn transcribe(
        &self,
        audio: &AudioClip,
        options: &TranscribeOptions,
    ) -> Result<Transcript, SpeechError> {
        if audio.samples.is_empty() {
            return Err(SpeechError::InvalidAudio("empty audio clip".into()));
        }

        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_no_context(true);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);
        if let Some(language) = options.language.as_deref() {
            params.set_language(Some(language));
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| SpeechError::Inference("whisper state lock poisoned".into()))?;
        state
            .full(params, &audio.samples)
            .map_err(|e| SpeechError::Inference(e.to_string()))?;

        let mut segments = Vec::new();
        let mut text = String::new();
        for segment in state.as_iter() {
            let segment_text = segment
                .to_str_lossy()
                .map_err(|e| SpeechError::Inference(e.to_string()))?
                .trim()
                .to_string();
            // whisper.cpp timestamps are in 10 ms ticks.
            let start_ms = segment.start_timestamp() * 10;
            let end_ms = segment.end_timestamp() * 10;
            if segment_text.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&segment_text);
            segments.push(Segment {
                start_ms,
                end_ms,
                text: segment_text,
            });
        }
        Ok(Transcript { text, segments })
    }
}
