//! Shared "low-power mode" runtime: holds the config dir in a `OnceLock` so the
//! engine-evictor thread can read the user's `low_power` toggle each tick
//! without depending on Tauri-managed resources.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with the resolved app config dir. Idempotent: the
/// first call wins so background threads never see a swap.
pub fn set_config_dir(path: PathBuf) {
    let _ = CONFIG_DIR.set(path);
}

/// The config dir captured at startup, or a temporary fallback if it was never
/// registered (only reachable in tests / synthetic callers).
pub fn config_dir() -> PathBuf {
    CONFIG_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("arya-no-config-dir"))
}

/// Loads the latest dictation settings (cheap JSON read) so callers can branch
/// on `low_power` without touching the Tauri-managed state.
pub fn is_enabled() -> bool {
    crate::dictation::settings::load(&config_dir()).low_power
}

/// Convenience for places that already have a config dir on the stack.
#[allow(dead_code)]
pub fn is_enabled_in(config_dir: &Path) -> bool {
    crate::dictation::settings::load(config_dir).low_power
}
