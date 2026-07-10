//! Debug-only runtime hooks for automated E2E checks. Each is env-gated and
//! compiled out of release builds (the `mod` is `#[cfg(debug_assertions)]`).

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

    // ARYA_DEV_HUD_HOLD=1: show the dictation HUD and leave it up (visual debug
    // of the overlay — never finishes, so it stays on screen for inspection).
    if std::env::var("ARYA_DEV_HUD_HOLD").as_deref() == Ok("1") {
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let service = handle.state::<Arc<DictationService>>().inner().clone();
            service.begin(&handle);
        });
    }

    // ARYA_DEV_RECORD_MS=<ms>: start a recording, stop after <ms>, process.
    if let Some(record_ms) = env_ms("ARYA_DEV_RECORD_MS") {
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let recorder = handle.state::<Recorder>().inner().clone();
            let mode = std::env::var("ARYA_DEV_RECORD_MODE").ok();
            let started =
                tauri::async_runtime::block_on(crate::recording::commands::start_recording_inner(
                    &handle, &pool, &recorder, None, mode,
                ));
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
            let mode = std::env::var("ARYA_DEV_RECORD_MODE").ok();
            let started =
                tauri::async_runtime::block_on(crate::recording::commands::start_recording_inner(
                    &handle, &pool, &recorder, None, mode,
                ));
            eprintln!("dev record forever: started {started:?}");
        });
    }

    // ARYA_DEV_AGENT=<prompt>: run one agent turn on a local model,
    // auto-approving tool requests, for automated E2E checks.
    if let Ok(prompt) = std::env::var("ARYA_DEV_AGENT") {
        let handle = handle.clone();
        let model =
            std::env::var("ARYA_DEV_AGENT_MODEL").unwrap_or_else(|_| "ollama:qwen3.6:35b".into());
        std::thread::spawn(move || {
            use tauri::Listener;
            std::thread::sleep(std::time::Duration::from_secs(2));
            // Auto-approve any tool approval so the run is unattended.
            let approver = handle.clone();
            handle.listen("agent:event", move |event| {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(event.payload()) else {
                    return;
                };
                let ev = &value["event"];
                if ev["kind"] == "tool-approval-required" {
                    eprintln!("dev agent: auto-approving {}", ev["description"]);
                    let session_id = value["sessionId"].as_str().unwrap_or("").to_string();
                    let call_id = ev["callId"].as_str().unwrap_or("").to_string();
                    let runtime = approver.state::<crate::agent::AgentRuntime>();
                    let _ = runtime.request(
                        &approver,
                        crate::agent::sidecar::WriteMode::Sandboxed,
                        "approval.resolve",
                        serde_json::json!({
                            "sessionId": session_id,
                            "callId": call_id,
                            "decision": "once",
                        }),
                    );
                }
                if ev["kind"] == "turn-finished" {
                    eprintln!("dev agent: turn finished");
                }
            });
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let runtime_handle = handle.clone();
            let result = tauri::async_runtime::block_on(async move {
                let runtime = runtime_handle.state::<crate::agent::AgentRuntime>();
                let session = crate::agent::commands::agent_create_session_inner(
                    &runtime_handle,
                    &pool,
                    &runtime,
                    model,
                    None,
                )
                .await?;
                eprintln!("dev agent: session {}", session.id);
                crate::agent::commands::agent_send_inner(
                    &runtime_handle,
                    &pool,
                    &runtime,
                    session.id.clone(),
                    prompt,
                )
                .await
                .map(|_| session.id)
            });
            eprintln!("dev agent: send result {result:?}");
        });
    }

    // ARYA_DEV_RAG=<query>: reindex the workspace, then run a timed search.
    if let Ok(query) = std::env::var("ARYA_DEV_RAG") {
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let start = std::time::Instant::now();
            let indexed = crate::rag::commands::reindex_blocking_public(&handle, &pool);
            eprintln!("dev rag: indexed {indexed:?} in {:?}", start.elapsed());
            let start = std::time::Instant::now();
            let hits = crate::rag::commands::search_blocking(&pool, &query, 5);
            let elapsed = start.elapsed();
            match hits {
                Ok(hits) => {
                    eprintln!("dev rag: search took {elapsed:?}, {} hits", hits.len());
                    for hit in hits.iter().take(3) {
                        eprintln!(
                            "dev rag: [{}:{}] {:.2} {}",
                            hit.source_kind,
                            hit.title,
                            hit.score,
                            hit.content.chars().take(80).collect::<String>()
                        );
                    }
                }
                Err(e) => eprintln!("dev rag: search failed {e}"),
            }
        });
    }

    // ARYA_DEV_ENROLL=<name>:<seconds>: enroll a voice profile from the mic.
    if let Ok(spec) = std::env::var("ARYA_DEV_ENROLL") {
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let (name, secs) = spec.split_once(':').unwrap_or((spec.as_str(), "6"));
            let seconds = secs.parse::<u32>().unwrap_or(6);
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let result = crate::recording::enroll::enroll_blocking(&handle, &pool, name, seconds);
            eprintln!("dev enroll: {result:?}");
        });
    }

    // ARYA_DEV_CALENDAR=1: print calendar access + current event.
    if std::env::var("ARYA_DEV_CALENDAR").as_deref() == Ok("1") {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(2));
            eprintln!(
                "dev calendar: access={:?}",
                crate::calendar::access_status()
            );
            eprintln!(
                "dev calendar: event={:?}",
                crate::calendar::current_or_upcoming_event(10)
            );
        });
    }

    // ARYA_DEV_CLOSE_TEST=1: exercise the close→hide→reopen cycle
    // programmatically. `Window::close()` emits the same `CloseRequested`
    // event a real close-button click does, so this proves the tray/dock
    // "show it again" path works without depending on synthetic OS clicks
    // landing on the (specially-protected) traffic-light control.
    if std::env::var("ARYA_DEV_CLOSE_TEST").as_deref() == Ok("1") {
        let handle = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if let Some(w) = handle.get_webview_window("main") {
                eprintln!("dev close-test: calling window.close()");
                let _ = w.close();
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            let still_gettable = handle.get_webview_window("main").is_some();
            let visible_after_close = handle
                .get_webview_window("main")
                .and_then(|w| w.is_visible().ok());
            eprintln!(
                "dev close-test: gettable={still_gettable:?} visible_after_close={visible_after_close:?} (expect true / Some(false) — hidden, not destroyed)"
            );
            std::thread::sleep(std::time::Duration::from_secs(1));
            eprintln!("dev close-test: calling crate::show_main (what the tray/dock reopen do)");
            crate::show_main(&handle);
            std::thread::sleep(std::time::Duration::from_millis(300));
            let visible_after_show = handle
                .get_webview_window("main")
                .and_then(|w| w.is_visible().ok());
            eprintln!(
                "dev close-test: visible_after_show={visible_after_show:?} (expect Some(true))"
            );
        });
    }

    // ARYA_DEV_RECOVER=1: scan for recoverable sessions and recover them.
    if std::env::var("ARYA_DEV_RECOVER").as_deref() == Ok("1") {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let pool = handle.state::<sqlx::SqlitePool>().inner().clone();
            let found = tauri::async_runtime::block_on(
                crate::recording::recovery::scan_recoverable_inner(&pool),
            );
            eprintln!("dev recover: scan {found:?}");
            if let Ok(list) = found {
                for item in list {
                    let result = tauri::async_runtime::block_on(
                        crate::recording::recovery::recover_recording_inner(
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
