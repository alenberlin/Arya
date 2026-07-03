//! The Arya agent: our own model-agnostic runtime (TypeScript sidecar on the
//! Vercel AI SDK), spawned per write-mode, Seatbelt-jailed by default.

pub mod commands;
pub mod sidecar;

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use sidecar::{Sidecar, WriteMode};

/// Holds the running sidecars (one per write-mode) plus per-session
/// accumulation buffers used to persist assistant turns.
#[derive(Default)]
pub struct AgentRuntime {
    sidecars: Mutex<HashMap<WriteMode, Sidecar>>,
    accumulators: Mutex<HashMap<String, TurnAccumulator>>,
}

#[derive(Default, Clone)]
struct TurnAccumulator {
    text: String,
    reasoning: String,
    tools: Vec<Value>,
}

impl AgentRuntime {
    /// Returns (spawning on demand) the sidecar for `mode`.
    pub fn ensure(&self, app: &AppHandle, mode: WriteMode) -> Result<(), String> {
        let mut guard = self.sidecars.lock().expect("sidecars lock");
        if let Some(existing) = guard.get_mut(&mode) {
            if existing.is_alive() {
                return Ok(());
            }
            guard.remove(&mode);
        }
        let script = sidecar::sidecar_script()?;
        let workspace = agent_workspace(app)?;
        let app_for_events = app.clone();
        let sidecar = Sidecar::spawn(&script, mode, &workspace, move |params| {
            handle_event(&app_for_events, &params);
        })?;
        guard.insert(mode, sidecar);
        Ok(())
    }

    pub fn request(
        &self,
        app: &AppHandle,
        mode: WriteMode,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.ensure(app, mode)?;
        let guard = self.sidecars.lock().expect("sidecars lock");
        let sidecar = guard.get(&mode).ok_or("sidecar missing after ensure")?;
        sidecar.request(method, params)
    }
}

pub fn agent_workspace(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("agent/workspace"))
}

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
                acc.text.push_str(delta);
            }
        }
        "reasoning-delta" => {
            if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                acc.reasoning.push_str(delta);
            }
        }
        "tool-call" => acc.tools.push(event.clone()),
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
