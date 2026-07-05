//! Turns the dictation HUD window into a non-activating panel (macOS).
//!
//! A transparent Tauri window created with `focus: false` renders *nothing* on
//! macOS — its WebView only starts painting once the window has been key. But
//! the pill must never *become* key: dictation pastes by synthesizing ⌘V into
//! the frontmost app, so if Arya stole focus the paste would land in the pill
//! instead of the user's document.
//!
//! Reclassing the underlying `NSWindow` to an `NSPanel` with the
//! `NonactivatingPanel` style resolves the conflict — a non-activating panel
//! paints its content like an ordinary window yet never activates the app or
//! takes key focus, so keystrokes keep flowing to whatever the user is in.

/// Reclass the HUD window to a non-activating `NSPanel` and let it float over
/// other apps, Spaces, and full-screen windows. No-op off macOS.
#[cfg(target_os = "macos")]
pub fn make_hud_panel(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    // ObjC runtime primitive (libobjc) — reparents an instance's class.
    unsafe extern "C" {
        fn object_setClass(obj: *mut AnyObject, cls: *const AnyClass) -> *const AnyClass;
    }

    // NSWindowStyleMaskNonactivatingPanel.
    const NONACTIVATING_PANEL: usize = 1 << 7;
    // NSWindowCollectionBehavior CanJoinAllSpaces | FullScreenAuxiliary.
    const COLLECTION_ALL_SPACES: usize = (1 << 0) | (1 << 8);

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    let ns_window = ptr as *mut AnyObject;
    // AppKit window mutation must run on the main thread; Tauri's `setup` hook
    // (the only caller) already runs there.
    unsafe {
        if let Some(panel) = AnyClass::get(c"NSPanel") {
            let mask: usize = msg_send![ns_window, styleMask];
            object_setClass(ns_window, panel);
            let _: () = msg_send![ns_window, setStyleMask: mask | NONACTIVATING_PANEL];
        }
        let _: () = msg_send![ns_window, setCollectionBehavior: COLLECTION_ALL_SPACES];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn make_hud_panel(_window: &tauri::WebviewWindow) {}
