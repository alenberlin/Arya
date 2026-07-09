//! Bot-free meeting detection (macOS).
//!
//! Polls CoreAudio for processes actively using the microphone
//! (`kAudioProcessPropertyIsRunningInput`) and matches their bundle ids
//! against known meeting apps. Debounced; suppressed while Arya's recorder is
//! active. Emits `meeting:detected` / `meeting:cleared`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingInfo {
    pub bundle_id: String,
    pub app_name: String,
}

/// Bundle-id prefixes that count as meeting apps. Browsers are included
/// because Meet/Teams-on-web hold the mic through the browser process.
const MEETING_APPS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.microsoft.MSTeams", "Microsoft Teams"),
    ("com.cisco.webexmeetingsapp", "Webex"),
    ("Cisco-Systems.Spark", "Webex"),
    ("com.google.Chrome", "Chrome"),
    ("company.thebrowser.Browser", "Arc"),
    ("com.apple.Safari", "Safari"),
    ("org.mozilla.firefox", "Firefox"),
    ("com.brave.Browser", "Brave"),
    ("com.microsoft.edgemac", "Edge"),
];

pub fn classify_bundle(bundle_id: &str) -> Option<&'static str> {
    MEETING_APPS
        .iter()
        .find(|(prefix, _)| bundle_id.starts_with(prefix))
        .map(|(_, name)| *name)
}

/// Decides the current detection state from the set of mic-active bundles.
pub fn detect_from_bundles<'a>(bundles: impl Iterator<Item = &'a str>) -> Option<MeetingInfo> {
    let mut best: Option<MeetingInfo> = None;
    for bundle_id in bundles {
        if let Some(name) = classify_bundle(bundle_id) {
            let candidate = MeetingInfo {
                bundle_id: bundle_id.to_string(),
                app_name: name.to_string(),
            };
            // Prefer dedicated meeting apps over browsers.
            let is_browser = matches!(
                name,
                "Chrome" | "Arc" | "Safari" | "Firefox" | "Brave" | "Edge"
            );
            match &best {
                None => best = Some(candidate),
                Some(_) if !is_browser => best = Some(candidate),
                _ => {}
            }
        }
    }
    best
}

#[cfg(target_os = "macos")]
pub mod macos {
    use tauri::{Emitter, Manager};

    use super::{detect_from_bundles, MeetingInfo};

    // CoreAudio FFI (constants verified against the macOS SDK headers).
    const K_AUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
    const K_AUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST: u32 = u32::from_be_bytes(*b"prs#");
    const K_AUDIO_PROCESS_PROPERTY_BUNDLE_ID: u32 = u32::from_be_bytes(*b"pbid");
    const K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_INPUT: u32 = u32::from_be_bytes(*b"piri");
    const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyData(
            object_id: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier_data: *const std::ffi::c_void,
            data_size: *mut u32,
            data: *mut std::ffi::c_void,
        ) -> i32;
        fn AudioObjectGetPropertyDataSize(
            object_id: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier_data: *const std::ffi::c_void,
            data_size: *mut u32,
        ) -> i32;
    }

    fn property(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        }
    }

    fn process_objects() -> Vec<u32> {
        let address = property(K_AUDIO_HARDWARE_PROPERTY_PROCESS_OBJECT_LIST);
        // Ask CoreAudio the actual byte size first, then size the buffer to fit,
        // rather than silently truncating the process list at a fixed cap on a
        // busy machine (which could drop the Zoom/Teams process and miss it).
        let mut size: u32 = 0;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
            )
        };
        if status != 0 || size == 0 {
            return Vec::new();
        }
        let count = size as usize / std::mem::size_of::<u32>();
        let mut ids = vec![0u32; count];
        let mut size_out = size;
        let status = unsafe {
            AudioObjectGetPropertyData(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size_out,
                ids.as_mut_ptr() as *mut _,
            )
        };
        if status != 0 {
            return Vec::new();
        }
        ids.truncate(size_out as usize / std::mem::size_of::<u32>());
        ids
    }

    fn process_bundle_id(object: u32) -> Option<String> {
        use core_foundation::base::TCFType;
        use core_foundation::string::{CFString, CFStringRef};
        let address = property(K_AUDIO_PROCESS_PROPERTY_BUNDLE_ID);
        let mut string_ref: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut string_ref as *mut _ as *mut _,
            )
        };
        if status != 0 || string_ref.is_null() {
            return None;
        }
        let value = unsafe { CFString::wrap_under_create_rule(string_ref) }.to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    fn process_running_input(object: u32) -> bool {
        let address = property(K_AUDIO_PROCESS_PROPERTY_IS_RUNNING_INPUT);
        let mut value: u32 = 0;
        let mut size = 4u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut _ as *mut _,
            )
        };
        status == 0 && value != 0
    }

    /// Current meeting candidate from live CoreAudio state.
    pub fn scan() -> Option<MeetingInfo> {
        let bundles: Vec<String> = process_objects()
            .into_iter()
            .filter(|object| process_running_input(*object))
            .filter_map(process_bundle_id)
            .collect();
        detect_from_bundles(bundles.iter().map(|s| s.as_str()))
    }

    /// Spawns the detection poller. Detection is suppressed while Arya's own
    /// recorder is active, so capturing a note never prompts to record itself.
    pub fn spawn_poller(app: tauri::AppHandle) {
        std::thread::Builder::new()
            .name("arya-meeting-detect".into())
            .spawn(move || {
                let mut active: Option<MeetingInfo> = None;
                let mut clear_countdown = 0u8;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    let recording = app
                        .try_state::<crate::recording::recorder::Recorder>()
                        .map(|r| {
                            r.status().state != crate::recording::recorder::RecorderState::Idle
                        })
                        .unwrap_or(false);
                    if recording {
                        continue;
                    }
                    let found = scan();
                    match (&active, &found) {
                        (None, Some(info)) => {
                            active = Some(info.clone());
                            clear_countdown = 0;
                            let _ = app.emit("meeting:detected", info.clone());
                            #[cfg(debug_assertions)]
                            eprintln!("meeting detected: {} ({})", info.app_name, info.bundle_id);
                        }
                        (Some(_), None) => {
                            // Debounce: require two consecutive empty scans.
                            clear_countdown += 1;
                            if clear_countdown >= 2 {
                                active = None;
                                clear_countdown = 0;
                                let _ = app.emit("meeting:cleared", ());
                                #[cfg(debug_assertions)]
                                eprintln!("meeting cleared");
                            }
                        }
                        _ => {
                            clear_countdown = 0;
                        }
                    }
                }
            })
            .expect("spawn meeting detector");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_meeting_apps_and_prefixes() {
        assert_eq!(classify_bundle("us.zoom.xos"), Some("Zoom"));
        assert_eq!(classify_bundle("us.zoom.xos.helper"), Some("Zoom"));
        assert_eq!(classify_bundle("com.google.Chrome.helper"), Some("Chrome"));
        assert_eq!(classify_bundle("com.apple.finder"), None);
    }

    #[test]
    fn prefers_dedicated_apps_over_browsers() {
        let bundles = ["com.google.Chrome", "us.zoom.xos"];
        let info = detect_from_bundles(bundles.iter().copied()).unwrap();
        assert_eq!(info.app_name, "Zoom");
    }

    #[test]
    fn browser_alone_still_detects() {
        let bundles = ["com.google.Chrome.helper.plugin"];
        let info = detect_from_bundles(bundles.iter().copied()).unwrap();
        assert_eq!(info.app_name, "Chrome");
    }

    #[test]
    fn nothing_active_is_none() {
        assert!(detect_from_bundles(["com.apple.dock"].iter().copied()).is_none());
    }
}
