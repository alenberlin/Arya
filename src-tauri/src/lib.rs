pub mod audio;
pub mod cleanup;
mod db;
mod dictation;
mod notes;
mod paste;
mod recording;
pub mod speech;

use std::sync::Arc;

use tauri::Manager;

use dictation::service::DictationService;
use recording::recorder::Recorder;

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
            app.manage(Recorder::spawn());
            position_hud_top_center(app.handle());
            #[cfg(debug_assertions)]
            dev_hooks::install(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notes::create_note,
            notes::list_notes,
            notes::get_note,
            notes::get_note_turns,
            notes::update_note,
            notes::delete_note,
            notes::create_folder,
            notes::list_folders,
            notes::rename_folder,
            notes::delete_folder,
            notes::assign_note_to_folder,
            recording::commands::start_recording,
            recording::commands::pause_recording,
            recording::commands::resume_recording,
            recording::commands::finish_recording,
            recording::commands::recording_status,
            recording::commands::retry_processing,
            recording::commands::scan_recoverable_recordings,
            recording::commands::recover_recording,
            recording::commands::discard_recording,
            recording::commands::get_generation_settings,
            recording::commands::set_generation_settings,
            dictation::commands::get_dictation_settings,
            dictation::commands::set_dictation_settings,
            dictation::commands::dictation_status,
            dictation::commands::open_accessibility_settings,
            dictation::commands::list_dictation_history,
            dictation::commands::delete_dictation_history_item,
            dictation::commands::list_dictionary_entries,
            dictation::commands::create_dictionary_entry,
            dictation::commands::delete_dictionary_entry,
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

/// Debug-only runtime hooks for automated E2E checks. Each is env-gated and
/// compiled out of release builds entirely.
#[cfg(debug_assertions)]
mod dev_hooks {
    use std::sync::Arc;

    use tauri::Manager;

    use crate::dictation::service::DictationService;
    use crate::recording::recorder::Recorder;

    pub fn install(handle: tauri::AppHandle) {
        // ARYA_DEV_DICTATE_MS=<hold ms>: one dictation cycle after launch.
        if let Some(hold_ms) = env_ms("ARYA_DEV_DICTATE_MS") {
            let handle = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let service = handle.state::<Arc<DictationService>>().inner().clone();
                let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
                service.begin(&handle);
                std::thread::sleep(std::time::Duration::from_millis(hold_ms));
                service.finish(&handle, pool);
            });
        }

        // ARYA_DEV_RECORD_MS=<ms>: start a recording, stop after <ms>, process.
        if let Some(record_ms) = env_ms("ARYA_DEV_RECORD_MS") {
            let handle = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
                let recorder = handle.state::<Recorder>().inner().clone();
                let started = tauri::async_runtime::block_on(
                    crate::recording::commands::start_recording_inner(
                        &handle, &pool, &recorder, None,
                    ),
                );
                eprintln!("dev record: started {started:?}");
                std::thread::sleep(std::time::Duration::from_millis(record_ms));
                let finished = tauri::async_runtime::block_on(
                    crate::recording::commands::finish_recording_inner(&handle, &pool, &recorder),
                );
                eprintln!("dev record: finished {finished:?}");
            });
        }

        // ARYA_DEV_RECORD_FOREVER=1: start recording and never stop (crash test).
        if std::env::var("ARYA_DEV_RECORD_FOREVER").as_deref() == Ok("1") {
            let handle = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
                let recorder = handle.state::<Recorder>().inner().clone();
                let started = tauri::async_runtime::block_on(
                    crate::recording::commands::start_recording_inner(
                        &handle, &pool, &recorder, None,
                    ),
                );
                eprintln!("dev record forever: started {started:?}");
            });
        }

        // ARYA_DEV_RECOVER=1: scan for recoverable sessions and recover them.
        if std::env::var("ARYA_DEV_RECOVER").as_deref() == Ok("1") {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
                let found = tauri::async_runtime::block_on(
                    crate::recording::commands::scan_recoverable_inner(&pool),
                );
                eprintln!("dev recover: scan {found:?}");
                if let Ok(list) = found {
                    for item in list {
                        let result = tauri::async_runtime::block_on(
                            crate::recording::commands::recover_recording_inner(
                                &handle,
                                &pool,
                                &item.session_id,
                            ),
                        );
                        eprintln!("dev recover: recovered {result:?}");
                    }
                }
            });
        }
    }

    fn env_ms(name: &str) -> Option<u64> {
        std::env::var(name)
            .ok()?
            .parse::<u64>()
            .ok()
            .filter(|v| *v > 0)
    }
}
