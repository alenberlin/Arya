//! whisper.cpp backend via whisper-rs (Metal-accelerated on Apple Silicon).

use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::{AudioClip, Segment, SpeechEngine, SpeechError, TranscribeOptions, Transcript};

pub struct WhisperEngine {
    context: WhisperContext,
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
        Ok(Self { context })
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
        let mut state = self
            .context
            .create_state()
            .map_err(|e| SpeechError::Inference(e.to_string()))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(language) = options.language.as_deref() {
            params.set_language(Some(language));
        }

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
