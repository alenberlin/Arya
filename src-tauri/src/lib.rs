mod account;
pub mod agent;
mod attachments;
pub mod audio;
mod calendar;
pub mod cleanup;
mod db;
mod dictation;
mod http;
mod links;
mod meeting_detect;
mod notes;
mod paste;
mod rag;
mod recording;
pub mod speech;
mod translate;
mod tray;
mod vecmath;

/// Re-export for diagnostic integration tests.
pub use recording::diarize as recording_diarize;

use std::sync::Arc;

use tauri::Manager;

use dictation::service::DictationService;
use recording::recorder::Recorder;

/// Builds and runs the Tauri application.
pub fn run() {
    // whisper.cpp's Metal backend (macOS >= 15) registers every model buffer in
    // a device "residency set" and asserts that set is empty when it tears the
    // device down at process exit. Loaded whisper engines live in a
    // process-wide cache that is never dropped, so their buffers are still
    // registered at teardown and trip `GGML_ASSERT([rsets->data count] == 0)`,
    // aborting on quit. Short transcription clips don't benefit from residency
    // sets, so opt out via ggml's own escape hatch before any Metal device is
    // created. Set here, before any worker thread is spawned, so the write is
    // race-free.
    std::env::set_var("GGML_METAL_NO_RESIDENCY", "1");

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // "Launch at login": when enabled, macOS starts Arya with `--minimized`
        // so it waits quietly in the menu bar, ready for dictation.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let pool = tauri::async_runtime::block_on(db::init_pool(&data_dir.join("arya.db")))?;
            app.manage(pool);

            let config_dir = app.path().app_config_dir()?;
            let settings = dictation::settings::load(&config_dir);
            let app_profiles = dictation::profiles::load(&config_dir);
            let service = Arc::new(DictationService::new(settings.clone(), app_profiles));
            app.manage(service);
            if let Err(e) = dictation::hotkey::register(app.handle(), &settings) {
                // A bad persisted shortcut must not brick startup; surface it
                // and continue so the user can rebind in settings.
                eprintln!("dictation hotkey not registered: {e}");
            }
            // Right-Shift dictation trigger (hold = push-to-talk, double-tap =
            // hands-free). Its gesture state is shared so the pill's Stop can
            // reset it.
            let tap_state: Arc<std::sync::Mutex<dictation::keytap::TapState>> =
                Arc::new(std::sync::Mutex::new(dictation::keytap::TapState::default()));
            app.manage(tap_state.clone());
            // Ask for the permissions dictation needs at startup so the user can
            // grant them, rather than hitting silent failures. Only prompts for
            // what isn't already granted.
            #[cfg(target_os = "macos")]
            {
                if settings.uses_right_shift() && !dictation::keytap::input_monitoring_granted() {
                    dictation::keytap::request_input_monitoring();
                }
                if !paste::accessibility_trusted() {
                    paste::prompt_accessibility();
                }
            }
            dictation::keytap::spawn(app.handle().clone(), tap_state);
            app.manage(Recorder::spawn());
            app.manage(recording::commands::SystemCaptureSlot::default());
            app.manage(agent::AgentRuntime::default());
            app.manage(account::commands::AccountState::default());
            spawn_agent_scheduler(app.handle().clone());
            #[cfg(target_os = "macos")]
            meeting_detect::macos::spawn_poller(app.handle().clone());
            spawn_calendar_poller(app.handle().clone());
            if let Err(e) = tray::setup(app.handle()) {
                eprintln!("tray setup failed: {e}");
            }
            position_hud_bottom_center(app.handle());
            // Make the transparent pill a non-activating panel so it paints
            // without ever stealing focus from the app being dictated into.
            if let Some(hud) = app.get_webview_window("hud") {
                dictation::panel::make_hud_panel(&hud);
            }
            // Launched at login (autostart passes --minimized): stay in the menu
            // bar rather than popping the main window open on every boot.
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                }
            }
            #[cfg(debug_assertions)]
            dev_hooks::install(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notes::create_note,
            notes::list_notes,
            notes::search_notes,
            notes::get_note,
            notes::get_note_turns,
            notes::update_note,
            notes::delete_note,
            notes::delete_all_notes,
            attachments::attach_file,
            attachments::list_attachments,
            attachments::remove_attachment,
            attachments::open_attachment,
            notes::create_folder,
            notes::list_folders,
            notes::rename_folder,
            notes::delete_folder,
            notes::assign_note_to_folder,
            links::create_link,
            links::list_links_from,
            links::list_links_to,
            links::delete_link,
            links::reconcile_links,
            recording::commands::start_recording,
            recording::commands::pause_recording,
            recording::commands::resume_recording,
            recording::commands::finish_recording,
            recording::commands::recording_status,
            recording::commands::retry_processing,
            recording::recovery::scan_recoverable_recordings,
            recording::recovery::recover_recording,
            recording::recovery::discard_recording,
            recording::commands::get_generation_settings,
            recording::commands::set_generation_settings,
            recording::enroll::enroll_speaker_profile,
            recording::enroll::list_speaker_profiles,
            recording::enroll::delete_speaker_profile,
            recording::commands::calendar_access_status,
            recording::commands::request_calendar_access,
            agent::commands::agent_list_models,
            agent::commands::agent_create_session,
            agent::commands::agent_list_sessions,
            agent::commands::agent_get_messages,
            agent::commands::agent_send,
            agent::commands::agent_steer,
            agent::commands::agent_cancel,
            agent::commands::agent_resolve_approval,
            agent::commands::agent_delete_session,
            agent::agent_workspace_read_b64,
            agent::agent_generate_image,
            agent::ecosystem::mcp_list_servers,
            agent::ecosystem::mcp_add_server,
            agent::ecosystem::mcp_remove_server,
            agent::ecosystem::routine_list,
            agent::ecosystem::routine_create,
            agent::ecosystem::routine_set_enabled,
            agent::ecosystem::routine_delete,
            agent::ecosystem::routine_runs,
            agent::ecosystem::agent_branch_session,
            rag::commands::rag_status,
            rag::commands::rag_reindex,
            rag::commands::rag_search,
            account::commands::account_signin_state,
            account::commands::account_begin_signin,
            // Dev-only token backdoor; compiled out of release builds (must be
            // gated here too, or `generate_handler!` references a symbol that
            // does not exist in release).
            #[cfg(debug_assertions)]
            account::commands::account_set_token,
            account::commands::account_sign_out,
            account::commands::account_snapshot,
            account::commands::account_open_billing,
            dictation::commands::get_dictation_settings,
            dictation::commands::list_ollama_models,
            dictation::commands::set_dictation_settings,
            dictation::commands::dictation_status,
            dictation::commands::open_accessibility_settings,
            dictation::commands::list_dictation_history,
            dictation::commands::delete_dictation_history_item,
            dictation::commands::clear_dictation_history,
            dictation::commands::convert_dictation_to_note,
            dictation::commands::dictation_stop,
            dictation::commands::dictation_cancel,
            dictation::commands::dictation_set_session_polish,
            dictation::commands::dictation_pin_app,
            dictation::commands::dictation_unpin_app,
            dictation::commands::dictation_prepare_streaming,
            hud_resize,
            copy_to_clipboard,
            dictation::commands::list_dictionary_entries,
            dictation::commands::create_dictionary_entry,
            dictation::commands::delete_dictionary_entry,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Ticks the routine scheduler every 30s and reconnects MCP servers once at
/// startup so agent sessions have their tools immediately.
fn spawn_agent_scheduler(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("arya-agent-scheduler".into())
        .spawn(move || {
            let pool = app.state::<sqlx::SqlitePool>().inner().clone();
            let runtime = app.state::<agent::AgentRuntime>();
            // Reconnect persisted MCP servers (best effort; sidecar spawns lazily).
            tauri::async_runtime::block_on(agent::ecosystem::reconnect_all(&app, &pool, &runtime));
            loop {
                std::thread::sleep(std::time::Duration::from_secs(30));
                // Isolate each tick: a panic (e.g. an FFI throw deep inside a
                // routine) must not silently kill the scheduler for the whole
                // session — log it and keep ticking.
                let tick = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runtime = app.state::<agent::AgentRuntime>();
                    tauri::async_runtime::block_on(agent::ecosystem::run_due_routines(
                        &app, &pool, &runtime,
                    ));
                }));
                if tick.is_err() {
                    eprintln!("arya-agent-scheduler: routine tick panicked; continuing");
                }
            }
        })
        .expect("spawn agent scheduler");
}

/// Emits `calendar:upcoming` when an event starts within five minutes (and
/// nothing is being recorded), so the UI can offer one-click capture.
fn spawn_calendar_poller(app: tauri::AppHandle) {
    use tauri::Emitter;
    std::thread::Builder::new()
        .name("arya-calendar".into())
        .spawn(move || {
            let mut last_title: Option<String> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                // Isolate each poll: an EventKit FFI panic must not kill the
                // poller for the rest of the session. The closure returns the
                // next `last_title`; a panic keeps the previous one.
                let tick = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let recording = app
                        .try_state::<recording::recorder::Recorder>()
                        .map(|r| r.status().state != recording::recorder::RecorderState::Idle)
                        .unwrap_or(false);
                    if recording {
                        return last_title.clone();
                    }
                    match calendar::current_or_upcoming_event(5) {
                        Some(event) if last_title.as_deref() != Some(event.title.as_str()) => {
                            let title = event.title.clone();
                            let _ = app.emit("calendar:upcoming", event);
                            Some(title)
                        }
                        Some(_) => last_title.clone(),
                        None => None,
                    }
                }));
                match tick {
                    Ok(next) => last_title = next,
                    Err(_) => eprintln!("arya-calendar: poll panicked; continuing"),
                }
            }
        })
        .expect("spawn calendar poller");
}

/// Places the (hidden) dictation HUD at the bottom-center of the primary
/// monitor; it is shown/hidden by the dictation service and resized to fit its
/// content by the `hud_resize` command.
fn position_hud_bottom_center(app: &tauri::AppHandle) {
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
    let margin = (84.0 * monitor.scale_factor()) as i32; // logical px off the bottom
    let x = (screen.width as i32 - hud_size.width as i32) / 2;
    let y = (screen.height as i32 - hud_size.height as i32 - margin).max(0);
    let _ = hud.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Resizes the HUD to fit the content the webview reports (logical px), keeping
/// its bottom edge and horizontal center fixed — so the pill grows upward like
/// a card and stays wherever the user dragged it.
#[tauri::command]
fn hud_resize(app: tauri::AppHandle, width: f64, height: f64) {
    let Some(hud) = app.get_webview_window("hud") else {
        return;
    };
    let scale = hud.scale_factor().unwrap_or(1.0);
    let (Ok(old_pos), Ok(old_size)) = (hud.outer_position(), hud.outer_size()) else {
        return;
    };
    let new_w = (width * scale).round() as i32;
    let new_h = (height * scale).round() as i32;
    if new_w <= 0 || new_h <= 0 {
        return;
    }
    let new_x = old_pos.x + (old_size.width as i32 - new_w) / 2;
    let new_y = (old_pos.y + (old_size.height as i32 - new_h)).max(0);
    let _ = hud.set_size(tauri::PhysicalSize::new(new_w as u32, new_h as u32));
    let _ = hud.set_position(tauri::PhysicalPosition::new(new_x, new_y));
}

/// Copies text to the system clipboard (the dictation-history copy button).
#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    paste::set_clipboard(&text).map_err(|e| e.to_string())
}

/// Debug-only runtime hooks for automated E2E checks. Each is env-gated and
/// compiled out of release builds entirely.
#[cfg(debug_assertions)]
mod dev_hooks;
