//! Global-shortcut registration for dictation.
//!
//! Push-to-talk maps Pressed/Released to begin/finish; Toggle flips on
//! Pressed. Re-registration replaces any previous dictation shortcut.

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::service::DictationService;
use super::settings::{ActivationMode, DictationSettings};

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("invalid shortcut '{0}'")]
    Parse(String),
    #[error("shortcut registration failed: {0}")]
    Register(String),
}

pub fn register(app: &AppHandle, settings: &DictationSettings) -> Result<(), HotkeyError> {
    // The right-Shift trigger (hold / double-tap) is served by the low-level
    // event tap in `keytap`, not a global-shortcut accelerator.
    if settings.uses_right_shift() {
        let _ = app.global_shortcut().unregister_all();
        return Ok(());
    }

    let shortcut: Shortcut = settings
        .shortcut
        .parse()
        .map_err(|_| HotkeyError::Parse(settings.shortcut.clone()))?;
    let mode = settings.mode;

    // Replace whatever was registered before; dictation owns one shortcut.
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| HotkeyError::Register(e.to_string()))?;

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            let service = app.state::<Arc<DictationService>>().inner().clone();
            let pool = app.state::<sqlx::SqlitePool>().inner().clone();
            match (mode, event.state()) {
                (ActivationMode::PushToTalk, ShortcutState::Pressed) => service.begin(app),
                (ActivationMode::PushToTalk, ShortcutState::Released) => service.finish(app, pool),
                (ActivationMode::Toggle, ShortcutState::Pressed) => {
                    if service.is_recording() {
                        service.finish(app, pool);
                    } else {
                        service.begin(app);
                    }
                }
                (ActivationMode::Toggle, ShortcutState::Released) => {}
            }
        })
        .map_err(|e| HotkeyError::Register(e.to_string()))?;
    Ok(())
}
