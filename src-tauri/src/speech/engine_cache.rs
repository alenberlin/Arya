//! Process-wide cache of loaded speech engines, keyed by model path.
//!
//! Model load costs seconds and hundreds of MB; dictation and note
//! processing share one engine per model file. Engines are evicted after a
//! period of non-use so an idle app releases its Metal/GPU residency instead of
//! holding it for the whole process lifetime.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::whisper::WhisperEngine;
use super::SpeechError;

struct CacheEntry {
    engine: Arc<WhisperEngine>,
    last_used: Instant,
}

static CACHE: OnceLock<Mutex<HashMap<PathBuf, CacheEntry>>> = OnceLock::new();

pub fn get_or_load(model_path: &Path) -> Result<Arc<WhisperEngine>, SpeechError> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let mut guard = cache.lock().expect("engine cache lock");
        if let Some(entry) = guard.get_mut(model_path) {
            entry.last_used = Instant::now();
            return Ok(Arc::clone(&entry.engine));
        }
    }
    // Load outside the lock: model load takes seconds and must not block
    // other engines' lookups.
    let engine = Arc::new(WhisperEngine::load(model_path)?);
    let mut guard = cache.lock().expect("engine cache lock");
    let entry = guard
        .entry(model_path.to_path_buf())
        .or_insert_with(|| CacheEntry {
            engine: Arc::clone(&engine),
            last_used: Instant::now(),
        });
    Ok(Arc::clone(&entry.engine))
}

/// Drop engines idle for longer than `idle`, releasing their GPU residency.
/// Best-effort: an in-flight caller may still hold an `Arc`, in which case the
/// engine frees when that borrow ends. Returns how many entries were evicted.
pub fn evict_idle(idle: Duration) -> usize {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("engine cache lock");
    let before = guard.len();
    guard.retain(|_, entry| entry.last_used.elapsed() < idle);
    before - guard.len()
}
