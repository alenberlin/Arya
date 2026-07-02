//! Process-wide cache of loaded speech engines, keyed by model path.
//!
//! Model load costs seconds and hundreds of MB; dictation and note
//! processing share one engine per model file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::whisper::WhisperEngine;
use super::SpeechError;

static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<WhisperEngine>>>> = OnceLock::new();

pub fn get_or_load(model_path: &Path) -> Result<Arc<WhisperEngine>, SpeechError> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache.lock().expect("engine cache lock");
        if let Some(engine) = guard.get(model_path) {
            return Ok(Arc::clone(engine));
        }
    }
    // Load outside the lock: model load takes seconds and must not block
    // other engines' lookups.
    let engine = Arc::new(WhisperEngine::load(model_path)?);
    let mut guard = cache.lock().expect("engine cache lock");
    let entry = guard
        .entry(model_path.to_path_buf())
        .or_insert_with(|| Arc::clone(&engine));
    Ok(Arc::clone(entry))
}
