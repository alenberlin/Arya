//! Right-Shift dictation trigger via a low-level macOS event tap.
//!
//! A bare modifier key can't be a `tauri-plugin-global-shortcut` accelerator,
//! so we watch flagsChanged events for the right Shift key (keycode 60) with a
//! listen-only `CGEventTap` — the key still behaves normally elsewhere.
//!
//! Gesture model (all on the right Shift key):
//! - **Hold** → push-to-talk: recording runs while held, transcribes on release.
//! - **Double-tap** → hands-free: recording latches until another press (or the
//!   pill's Stop) ends it — so a press is always a way out.
//!
//! The gesture state machine ([`TapState`]) is pure and unit-tested; the tap
//! thread just translates key transitions into [`TapAction`]s.

use std::time::{Duration, Instant};

/// kVK_RightShift.
#[cfg(target_os = "macos")]
const RIGHT_SHIFT_KEYCODE: i64 = 60;

/// A press shorter than this is a tap (candidate double-tap), not a hold.
const HOLD_MIN: Duration = Duration::from_millis(250);
/// A second tap within this of the previous tap latches hands-free mode.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(450);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapMode {
    Idle,
    Holding,
    Latched,
}

/// What the tap thread should do in response to a key transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapAction {
    None,
    /// Begin capturing for push-to-talk (key went down).
    BeginPushToTalk,
    /// Finish a genuine hold: transcribe + insert.
    FinishPushToTalk,
    /// Discard a too-short tap (candidate first half of a double-tap).
    AbortTap,
    /// Latch hands-free capture until stopped.
    BeginHandsFree,
    /// Stop a latched hands-free capture (a press is the keyboard way out).
    StopHandsFree,
}

/// Pure gesture state machine for the right-Shift trigger.
#[derive(Debug)]
pub struct TapState {
    pub mode: TapMode,
    down_at: Option<Instant>,
    last_tap_at: Option<Instant>,
}

impl Default for TapState {
    fn default() -> Self {
        Self {
            mode: TapMode::Idle,
            down_at: None,
            last_tap_at: None,
        }
    }
}

impl TapState {
    /// Feed a right-Shift transition (`down` = pressed, else released) at `now`.
    pub fn on_event(&mut self, down: bool, now: Instant) -> TapAction {
        if down {
            if self.mode == TapMode::Latched {
                // A press is the keyboard way out of hands-free, so the user is
                // never trapped if the pill's Stop button is out of reach.
                self.reset();
                return TapAction::StopHandsFree;
            }
            // A recent prior tap makes this a double-tap → latch hands-free.
            if let Some(t) = self.last_tap_at.take() {
                if now.duration_since(t) < DOUBLE_TAP_WINDOW {
                    self.mode = TapMode::Latched;
                    self.down_at = None;
                    return TapAction::BeginHandsFree;
                }
            }
            self.mode = TapMode::Holding;
            self.down_at = Some(now);
            TapAction::BeginPushToTalk
        } else {
            match self.mode {
                TapMode::Holding => {
                    let held = self
                        .down_at
                        .take()
                        .map(|d| now.duration_since(d))
                        .unwrap_or_default();
                    self.mode = TapMode::Idle;
                    if held >= HOLD_MIN {
                        self.last_tap_at = None;
                        TapAction::FinishPushToTalk
                    } else {
                        // Too short for a hold — remember it in case a second
                        // tap follows to latch hands-free.
                        self.last_tap_at = Some(now);
                        TapAction::AbortTap
                    }
                }
                // Latched ignores release; Idle has nothing to do.
                _ => TapAction::None,
            }
        }
    }

    /// Return to idle after an external stop (e.g. the pill's Stop button).
    pub fn reset(&mut self) {
        self.mode = TapMode::Idle;
        self.down_at = None;
        self.last_tap_at = None;
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use core_foundation::runloop::CFRunLoop;
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, CallbackResult, EventField,
    };
    use sqlx::SqlitePool;
    use tauri::{AppHandle, Emitter, Manager};

    use super::{TapAction, TapState, RIGHT_SHIFT_KEYCODE};
    use crate::dictation::service::DictationService;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
        fn IOHIDRequestAccess(request: u32) -> bool;
    }

    /// Whether Input Monitoring (keystroke listening) is authorized.
    /// kIOHIDRequestTypeListenEvent = 1, kIOHIDAccessTypeGranted = 0.
    pub fn input_monitoring_granted() -> bool {
        unsafe { IOHIDCheckAccess(1) == 0 }
    }

    /// Shows the system Input Monitoring prompt when access is undetermined, so
    /// the user can grant it. Returns the current grant status.
    pub fn request_input_monitoring() -> bool {
        unsafe { IOHIDRequestAccess(1) }
    }

    /// Spawns the right-Shift event tap on a dedicated run-loop thread. If the
    /// tap can't be created yet (Input Monitoring not granted), it retries so it
    /// starts working the moment the user enables the permission — no restart.
    pub fn spawn(app: AppHandle, state: Arc<Mutex<TapState>>) {
        std::thread::Builder::new()
            .name("arya-dictation-keytap".into())
            .spawn(move || loop {
                let cb_app = app.clone();
                let cb_state = state.clone();
                let result = CGEventTap::with_enabled(
                    // Session-level, listen-only: the standard place a regular
                    // (non-root) app taps keyboard events, and we never consume
                    // the key so Shift keeps working normally.
                    CGEventTapLocation::Session,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    vec![CGEventType::FlagsChanged],
                    move |_proxy, _etype, event| {
                        let keycode =
                            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                        if keycode == RIGHT_SHIFT_KEYCODE && uses_right_shift(&cb_app) {
                            let down = event.get_flags().contains(CGEventFlags::CGEventFlagShift);
                            let action = cb_state
                                .lock()
                                .map(|mut s| s.on_event(down, Instant::now()))
                                .unwrap_or(TapAction::None);
                            dispatch(&cb_app, action);
                        }
                        CallbackResult::Keep
                    },
                    CFRunLoop::run_current,
                );
                // On success `with_enabled` blocks in the run loop and only
                // returns if it stops. On failure it returns immediately — wait
                // for the permission grant and retry.
                if result.is_ok() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            })
            .expect("spawn dictation keytap");
    }

    /// The tap runs process-wide but only acts when the right-Shift trigger is
    /// the configured shortcut (a custom accelerator uses the global-shortcut
    /// path instead).
    fn uses_right_shift(app: &AppHandle) -> bool {
        app.try_state::<Arc<DictationService>>()
            .map(|s| s.settings().uses_right_shift())
            .unwrap_or(false)
    }

    fn dispatch(app: &AppHandle, action: TapAction) {
        let Some(service) = app.try_state::<Arc<DictationService>>() else {
            return;
        };
        let service = service.inner().clone();
        match action {
            TapAction::BeginPushToTalk => service.begin(app),
            TapAction::AbortTap => service.abort_recording(app),
            TapAction::FinishPushToTalk => {
                if let Some(pool) = app.try_state::<SqlitePool>() {
                    service.finish(app, pool.inner().clone());
                }
            }
            TapAction::BeginHandsFree => {
                service.begin(app);
                let _ = app.emit("dictation:hands-free", true);
            }
            TapAction::StopHandsFree => {
                if let Some(pool) = app.try_state::<SqlitePool>() {
                    service.finish(app, pool.inner().clone());
                }
                let _ = app.emit("dictation:hands-free", false);
            }
            TapAction::None => {}
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::{input_monitoring_granted, request_input_monitoring, spawn};

#[cfg(not(target_os = "macos"))]
pub fn spawn(_app: tauri::AppHandle, _state: std::sync::Arc<std::sync::Mutex<TapState>>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn hold_is_push_to_talk() {
        let mut s = TapState::default();
        let t0 = Instant::now();
        assert_eq!(s.on_event(true, t0), TapAction::BeginPushToTalk);
        // Released after a genuine hold.
        assert_eq!(s.on_event(false, t0 + ms(600)), TapAction::FinishPushToTalk);
        assert_eq!(s.mode, TapMode::Idle);
    }

    #[test]
    fn double_tap_latches_hands_free_and_a_press_stops_it() {
        let mut s = TapState::default();
        let t0 = Instant::now();
        // First quick tap: begins then aborts (too short).
        assert_eq!(s.on_event(true, t0), TapAction::BeginPushToTalk);
        assert_eq!(s.on_event(false, t0 + ms(80)), TapAction::AbortTap);
        // Second tap within the window latches hands-free.
        assert_eq!(s.on_event(true, t0 + ms(250)), TapAction::BeginHandsFree);
        assert_eq!(s.mode, TapMode::Latched);
        // Releasing the latching press is ignored — it stays recording.
        assert_eq!(s.on_event(false, t0 + ms(300)), TapAction::None);
        assert_eq!(s.mode, TapMode::Latched);
        // A later press is the keyboard way out: stop and return to idle.
        assert_eq!(s.on_event(true, t0 + ms(2000)), TapAction::StopHandsFree);
        assert_eq!(s.mode, TapMode::Idle);
        // The release after that stop is a no-op (not a fresh trigger).
        assert_eq!(s.on_event(false, t0 + ms(2100)), TapAction::None);
    }

    #[test]
    fn slow_second_tap_is_a_fresh_push_to_talk_not_a_latch() {
        let mut s = TapState::default();
        let t0 = Instant::now();
        s.on_event(true, t0);
        assert_eq!(s.on_event(false, t0 + ms(80)), TapAction::AbortTap);
        // Too late to count as a double-tap.
        assert_eq!(
            s.on_event(true, t0 + ms(900)),
            TapAction::BeginPushToTalk,
            "a late second press starts a new push-to-talk"
        );
    }

    #[test]
    fn consecutive_holds_do_not_false_trigger_hands_free() {
        let mut s = TapState::default();
        let t0 = Instant::now();
        // A real hold clears any tap memory.
        s.on_event(true, t0);
        assert_eq!(s.on_event(false, t0 + ms(600)), TapAction::FinishPushToTalk);
        // A press soon after must be push-to-talk, not a latch.
        assert_eq!(s.on_event(true, t0 + ms(700)), TapAction::BeginPushToTalk);
    }

    #[test]
    fn reset_returns_to_idle() {
        let mut s = TapState::default();
        s.on_event(true, Instant::now());
        s.reset();
        assert_eq!(s.mode, TapMode::Idle);
    }
}
