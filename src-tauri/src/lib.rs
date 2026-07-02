pub mod audio;
pub mod cleanup;
mod db;
mod dictation;
mod notes;
mod paste;
pub mod speech;

use std::sync::Arc;

use tauri::Manager;

use dictation::service::DictationService;

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let pool = tauri::async_runtime::block_on(db::init_pool(&data_dir.join("arya.db")))?;
            app.manage(pool);

            let config_dir = app.path().app_config_dir()?;
            let settings = dictation::settings::load(&config_dir);
            let service = Arc::new(DictationService::new(settings.clone()));
            app.manage(service);
            if let Err(e) = dictation::hotkey::register(app.handle(), &settings) {
                // A bad persisted shortcut must not brick startup; surface it
                // and continue so the user can rebind in settings.
                eprintln!("dictation hotkey not registered: {e}");
            }
            position_hud_top_center(app.handle());

            // Dev-only runtime hook: ARYA_DEV_DICTATE_MS=<hold ms> drives one
            // dictation cycle shortly after launch, for automated E2E checks.
            #[cfg(debug_assertions)]
            {
                let hold_ms = std::env::var("ARYA_DEV_DICTATE_MS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                if hold_ms > 0 {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        let service = handle.state::<Arc<DictationService>>().inner().clone();
                        let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
                        service.begin(&handle);
                        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
                        service.finish(&handle, pool);
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notes::create_note,
            notes::list_notes,
            dictation::commands::get_dictation_settings,
            dictation::commands::set_dictation_settings,
            dictation::commands::dictation_status,
            dictation::commands::open_accessibility_settings,
            dictation::commands::list_dictation_history,
            dictation::commands::delete_dictation_history_item,
            dictation::commands::list_dictionary_entries,
            dictation::commands::create_dictionary_entry,
            dictation::commands::delete_dictionary_entry,
            #[cfg(debug_assertions)]
            dictation::commands::dev_run_dictation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Places the (hidden) dictation HUD at the top-center of the primary
/// monitor; it is shown/hidden by the dictation service.
fn position_hud_top_center(app: &tauri::AppHandle) {
    let Some(hud) = app.get_webview_window("hud") else {
        return;
    };
    let Ok(Some(monitor)) = hud.primary_monitor() else {
        return;
    };
    let screen = monitor.size();
    let Ok(hud_size) = hud.outer_size() else {
        return;
    };
    let x = (screen.width as i32 - hud_size.width as i32) / 2;
    let _ = hud.set_position(tauri::PhysicalPosition::new(x, 16));
}
