//! The Arya agent: our own model-agnostic runtime (TypeScript sidecar on the
//! Vercel AI SDK), spawned per write-mode, Seatbelt-jailed by default.

pub mod commands;
pub mod ecosystem;
pub mod sidecar;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use sidecar::{Sidecar, WriteMode};

/// Holds the running sidecars (one per write-mode) plus per-session
/// accumulation buffers used to persist assistant turns.
#[derive(Default)]
pub struct AgentRuntime {
    sidecars: Mutex<HashMap<WriteMode, Arc<Sidecar>>>,
    accumulators: Mutex<HashMap<String, TurnAccumulator>>,
}

#[derive(Default, Clone)]
struct TurnAccumulator {
    text: String,
    reasoning: String,
    tools: Vec<Value>,
}

impl AgentRuntime {
    /// Returns (spawning on demand) an `Arc` to the sidecar for `mode`. The
    /// map lock is held only long enough to look up or insert; the caller
    /// makes the blocking round-trip against the returned handle with no lock
    /// held, so cancel/steer on the same mode never wait behind a long call.
    fn get_or_spawn(&self, app: &AppHandle, mode: WriteMode) -> Result<Arc<Sidecar>, String> {
        {
            let mut guard = self.sidecars.lock().expect("sidecars lock");
            if let Some(existing) = guard.get(&mode) {
                if existing.is_alive() {
                    return Ok(Arc::clone(existing));
                }
                guard.remove(&mode);
            }
        }
        let script = sidecar::sidecar_script()?;
        let workspace = agent_workspace(app)?;
        let app_for_events = app.clone();
        let app_for_context = app.clone();
        let sidecar = Arc::new(Sidecar::spawn(
            &script,
            mode,
            &workspace,
            move |params| {
                handle_event(&app_for_events, &params);
            },
            move |query, limit| {
                // Answer the agent's search_workspace tool via local RAG.
                let pool = app_for_context.state::<SqlitePool>().inner().clone();
                let hits = crate::rag::commands::search_blocking(&pool, &query, limit as usize)?;
                Ok(serde_json::json!({ "hits": hits }))
            },
        )?);
        let mut guard = self.sidecars.lock().expect("sidecars lock");
        // Another thread may have spawned concurrently; keep whichever is in
        // the map so both callers talk to the same process.
        let entry = guard.entry(mode).or_insert_with(|| Arc::clone(&sidecar));
        Ok(Arc::clone(entry))
    }

    /// Spawns the sidecar for `mode` if it isn't already running.
    pub fn ensure(&self, app: &AppHandle, mode: WriteMode) -> Result<(), String> {
        self.get_or_spawn(app, mode).map(|_| ())
    }

    pub fn request(
        &self,
        app: &AppHandle,
        mode: WriteMode,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let sidecar = self.get_or_spawn(app, mode)?;
        // No map lock held here — the blocking round-trip can't serialize
        // other commands (cancel/steer) for this mode.
        sidecar.request(method, params)
    }

    /// Best-effort session teardown without spawning a sidecar just to end it.
    pub fn end_session_if_running(&self, mode: WriteMode, session_id: &str) {
        let sidecar = {
            let mut guard = self.sidecars.lock().expect("sidecars lock");
            match guard.get(&mode) {
                Some(existing) if existing.is_alive() => Some(Arc::clone(existing)),
                Some(_) => {
                    guard.remove(&mode);
                    None
                }
                None => None,
            }
        };
        if let Some(sidecar) = sidecar {
            let _ = sidecar.request(
                "session.end",
                serde_json::json!({ "sessionId": session_id }),
            );
        }
        self.clear_session_state(session_id);
    }

    pub fn clear_session_state(&self, session_id: &str) {
        self.accumulators
            .lock()
            .expect("acc lock")
            .remove(session_id);
    }
}

pub fn agent_workspace(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("agent/workspace"))
}

/// Generates an image via the sidecar and returns its workspace path.
#[tauri::command]
pub fn agent_generate_image(
    app: AppHandle,
    runtime: tauri::State<'_, AgentRuntime>,
    prompt: String,
    size: Option<String>,
) -> Result<serde_json::Value, String> {
    let workspace = agent_workspace(&app)?;
    runtime.request(
        &app,
        sidecar::WriteMode::Sandboxed,
        "image.generate",
        serde_json::json!({
            "workspace": workspace.to_string_lossy(),
            "prompt": prompt,
            "size": size,
        }),
    )
}

/// Reads a workspace file as base64 (for inline image rendering).
#[tauri::command]
pub fn agent_workspace_read_b64(app: AppHandle, path: String) -> Result<String, String> {
    let base = agent_workspace(&app)?;
    let target = base.join(&path).canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&base) {
        return Err("path escapes workspace".into());
    }
    let bytes = std::fs::read(&target).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("file too large".into());
    }
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Caps so a session that never emits `turn-finished` (sidecar crash, an
/// infinite model loop) can't grow its accumulator without bound.
const MAX_ACC_BYTES: usize = 4 * 1024 * 1024;
const MAX_ACC_TOOLS: usize = 2000;

/// Routes a sidecar event: forward to the UI, accumulate, and persist the
/// assistant message when the turn finishes.
fn handle_event(app: &AppHandle, params: &Value) {
    let _ = app.emit("agent:event", params.clone());
    let Some(session_id) = params.get("sessionId").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(event) = params.get("event") else {
        return;
    };
    let kind = event.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let runtime = app.state::<AgentRuntime>();
    let mut accumulators = runtime.accumulators.lock().expect("acc lock");
    let acc = accumulators.entry(session_id.to_string()).or_default();
    match kind {
        "text-delta" => {
            if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                if acc.text.len() < MAX_ACC_BYTES {
                    acc.text.push_str(delta);
                }
            }
        }
        "reasoning-delta" => {
            if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                if acc.reasoning.len() < MAX_ACC_BYTES {
                    acc.reasoning.push_str(delta);
                }
            }
        }
        "tool-call" => {
            if acc.tools.len() < MAX_ACC_TOOLS {
                acc.tools.push(event.clone());
            }
        }
        "tool-result" => {
            // Attach the result to its call entry when present.
            let call_id = event.get("callId").cloned();
            if let (Some(call_id), Some(result)) = (call_id, event.get("result")) {
                for tool in acc.tools.iter_mut() {
                    if tool.get("callId") == Some(&call_id) {
                        tool["result"] = result.clone();
                    }
                }
            }
        }
        "turn-finished" => {
            let finished = std::mem::take(acc);
            accumulators.remove(session_id);
            drop(accumulators);
            let content = serde_json::json!({
                "text": finished.text,
                "reasoning": if finished.reasoning.is_empty() { Value::Null } else { Value::String(finished.reasoning.clone()) },
                "tools": finished.tools,
            });
            let pool = app.state::<SqlitePool>().inner().clone();
            let session_id = session_id.to_string();
            std::thread::spawn(move || {
                let _ = tauri::async_runtime::block_on(async {
                    sqlx::query(
                        "INSERT INTO agent_messages (id, session_id, role, content_json, created_at)
                         VALUES (?1, ?2, 'assistant', ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                    )
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&session_id)
                    .bind(content.to_string())
                    .execute(&pool)
                    .await?;
                    sqlx::query(
                        "UPDATE agent_sessions SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                    )
                    .bind(&session_id)
                    .execute(&pool)
                    .await
                });
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_session_state_removes_accumulator() {
        let runtime = AgentRuntime::default();
        runtime.accumulators.lock().unwrap().insert(
            "session-1".to_string(),
            TurnAccumulator {
                text: "partial".to_string(),
                reasoning: String::new(),
                tools: vec![],
            },
        );

        runtime.clear_session_state("session-1");

        assert!(
            !runtime
                .accumulators
                .lock()
                .unwrap()
                .contains_key("session-1"),
            "deleting a session must drop any unfinished assistant-turn buffer"
        );
    }
}
