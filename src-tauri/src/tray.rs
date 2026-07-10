//! Menu-bar tray: agent status at a glance, quick actions.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter};

use crate::show_main;

/// Builds the tray icon and menu. Clicking "Show Arya" focuses the main
/// window; the menu also offers quit.
pub fn setup(app: &AppHandle) -> Result<(), tauri::Error> {
    let show = MenuItem::with_id(app, "show", "Show Arya", true, None::<&str>)?;
    let new_session =
        MenuItem::with_id(app, "new_session", "New agent session", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Arya", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &new_session, &quit])?;

    TrayIconBuilder::with_id("arya-tray")
        .icon(app.default_window_icon().cloned().expect("default icon"))
        .tooltip("Arya")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "new_session" => {
                show_main(app);
                let _ = app.emit_to("main", "tray:new-session", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
