//! Inserting text into the foreground app (macOS).
//!
//! Strategy: swap the general pasteboard to the dictated text, synthesize
//! Cmd+V into the frontmost app, then restore the previous pasteboard
//! contents. Requires the Accessibility (TCC) grant for event synthesis.

#[derive(Debug, thiserror::Error)]
pub enum PasteError {
    #[error("accessibility permission is not granted")]
    AccessibilityDenied,
    #[error("pasteboard error: {0}")]
    Pasteboard(String),
    #[error("event synthesis failed")]
    EventSynthesis,
    #[cfg(not(target_os = "macos"))]
    #[error("unsupported platform")]
    Unsupported,
}

/// Info about the app that will receive the paste.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetApp {
    pub bundle_id: Option<String>,
    pub name: Option<String>,
}

/// Bundle ids treated as email compose surfaces.
pub const EMAIL_BUNDLE_IDS: &[&str] = &[
    "com.apple.mail",
    "com.microsoft.Outlook",
    "com.readdle.smartemail-Mac",
    "com.superhuman.electron",
    "com.airmailapp.airmail",
    "com.mimestream.Mimestream",
];

pub fn is_email_app(bundle_id: Option<&str>) -> bool {
    bundle_id.is_some_and(|id| EMAIL_BUNDLE_IDS.contains(&id))
}

#[cfg(target_os = "macos")]
pub use macos::{
    accessibility_trusted, frontmost_app, paste_text, prompt_accessibility, set_clipboard,
};

#[cfg(not(target_os = "macos"))]
pub fn paste_text(_text: &str) -> Result<(), PasteError> {
    Err(PasteError::Unsupported)
}

#[cfg(not(target_os = "macos"))]
pub fn set_clipboard(_text: &str) -> Result<(), PasteError> {
    Err(PasteError::Unsupported)
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> TargetApp {
    TargetApp {
        bundle_id: None,
        name: None,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_trusted() -> bool {
    false
}

#[cfg(target_os = "macos")]
mod macos {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSWorkspace};
    use objc2_foundation::NSString;

    use super::{PasteError, TargetApp};

    const KEY_V: u16 = 9; // kVK_ANSI_V

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    pub fn accessibility_trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Opens the system Accessibility prompt for this process (no-op if
    /// already trusted).
    pub fn prompt_accessibility() {
        // AXIsProcessTrustedWithOptions with kAXTrustedCheckOptionPrompt
        // requires CoreFoundation dictionary plumbing; opening the settings
        // pane directly is equally effective and simpler.
        if !accessibility_trusted() {
            let _ = std::process::Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
                )
                .spawn();
        }
    }

    pub fn frontmost_app() -> TargetApp {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication();
        TargetApp {
            bundle_id: app
                .as_ref()
                .and_then(|a| a.bundleIdentifier())
                .map(|s| s.to_string()),
            name: app
                .as_ref()
                .and_then(|a| a.localizedName())
                .map(|s| s.to_string()),
        }
    }

    pub fn paste_text(text: &str) -> Result<(), PasteError> {
        if !accessibility_trusted() {
            return Err(PasteError::AccessibilityDenied);
        }
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            let previous = pasteboard.stringForType(NSPasteboardTypeString);

            pasteboard.clearContents();
            let ok =
                pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
            if !ok {
                return Err(PasteError::Pasteboard("setString failed".into()));
            }

            synthesize_cmd_v()?;

            // Give the target app time to read the pasteboard before
            // restoring what the user had on it.
            std::thread::sleep(std::time::Duration::from_millis(300));
            if let Some(previous) = previous {
                pasteboard.clearContents();
                pasteboard.setString_forType(&previous, NSPasteboardTypeString);
            }
        }
        Ok(())
    }

    /// Puts `text` on the general pasteboard (a plain Copy — no paste, and the
    /// previous contents are intentionally replaced). Needs no permissions.
    pub fn set_clipboard(text: &str) -> Result<(), PasteError> {
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            let ok =
                pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
            if !ok {
                return Err(PasteError::Pasteboard("setString failed".into()));
            }
        }
        Ok(())
    }

    fn synthesize_cmd_v() -> Result<(), PasteError> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| PasteError::EventSynthesis)?;
        let key_down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
            .map_err(|_| PasteError::EventSynthesis)?;
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        key_down.post(CGEventTapLocation::HID);
        let key_up = CGEvent::new_keyboard_event(source, KEY_V, false)
            .map_err(|_| PasteError::EventSynthesis)?;
        key_up.set_flags(CGEventFlags::CGEventFlagCommand);
        key_up.post(CGEventTapLocation::HID);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_detection_matches_known_clients_only() {
        assert!(is_email_app(Some("com.apple.mail")));
        assert!(is_email_app(Some("com.microsoft.Outlook")));
        assert!(!is_email_app(Some("com.apple.Safari")));
        assert!(!is_email_app(None));
    }
}
