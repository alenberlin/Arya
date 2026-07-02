mod db;
mod notes;

use tauri::Manager;

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let pool = tauri::async_runtime::block_on(db::init_pool(&data_dir.join("arya.db")))?;
            app.manage(pool);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notes::create_note,
            notes::list_notes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
