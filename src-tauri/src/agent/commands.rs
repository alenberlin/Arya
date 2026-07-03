//! Tauri commands for agent sessions.

use serde_json::json;
use sqlx::SqlitePool;
use tauri::{AppHandle, State};

use super::sidecar::WriteMode;
use super::AgentRuntime;

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub title: String,
    pub model: String,
    pub mode: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: String,
    pub role: String,
    pub content_json: String,
    pub created_at: String,
}

#[tauri::command]
pub fn agent_list_models(
    app: AppHandle,
    runtime: State<'_, AgentRuntime>,
) -> Result<Vec<String>, String> {
    let result = runtime.request(&app, WriteMode::Sandboxed, "models.list", json!({}))?;
    Ok(result
        .get("models")
        .and_then(|m| m.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

#[tauri::command]
pub async fn agent_create_session(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    model: String,
    mode: Option<String>,
) -> Result<AgentSession, String> {
    agent_create_session_inner(&app, &pool, &runtime, model, mode).await
}

pub async fn agent_create_session_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    runtime: &AgentRuntime,
    model: String,
    mode: Option<String>,
) -> Result<AgentSession, String> {
    let mode = mode.unwrap_or_else(|| "sandboxed".into());
    let id = uuid::Uuid::new_v4().to_string();
    let session = sqlx::query_as::<_, AgentSession>(
        "INSERT INTO agent_sessions (id, title, model, mode, created_at, updated_at)
         VALUES (?1, 'New session', ?2, ?3,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, model, mode, created_at, updated_at",
    )
    .bind(&id)
    .bind(&model)
    .bind(&mode)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let workspace = super::agent_workspace(app)?;
    runtime.request(
        app,
        WriteMode::parse(&mode),
        "session.start",
        json!({
            "sessionId": id,
            "model": model,
            "mode": mode,
            "workspace": workspace.to_string_lossy(),
        }),
    )?;
    Ok(session)
}

#[tauri::command]
pub async fn agent_list_sessions(pool: State<'_, SqlitePool>) -> Result<Vec<AgentSession>, String> {
    sqlx::query_as::<_, AgentSession>(
        "SELECT id, title, model, mode, created_at, updated_at
         FROM agent_sessions ORDER BY updated_at DESC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_get_messages(
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<Vec<AgentMessage>, String> {
    sqlx::query_as::<_, AgentMessage>(
        "SELECT id, role, content_json, created_at
         FROM agent_messages WHERE session_id = ?1 ORDER BY created_at",
    )
    .bind(session_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

/// Sends a user message; ensures the sidecar session exists (restores
/// history after an app restart), persists the user turn, and starts the run.
#[tauri::command]
pub async fn agent_send(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    agent_send_inner(&app, &pool, &runtime, session_id, text).await
}

pub async fn agent_send_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    runtime: &AgentRuntime,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let session = sqlx::query_as::<_, AgentSession>(
        "SELECT id, title, model, mode, created_at, updated_at FROM agent_sessions WHERE id = ?1",
    )
    .bind(&session_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mode = WriteMode::parse(&session.mode);

    // Restore the sidecar session if the process restarted since creation.
    let history = sqlx::query_as::<_, (String, String)>(
        "SELECT role, content_json FROM agent_messages WHERE session_id = ?1 ORDER BY created_at",
    )
    .bind(&session_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .filter_map(|(role, content)| {
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let text = parsed.get("text").and_then(|t| t.as_str())?.to_string();
        if text.is_empty() {
            None
        } else {
            Some(json!({ "role": role, "text": text }))
        }
    })
    .collect::<Vec<_>>();

    let workspace = super::agent_workspace(app)?;
    let _ = runtime.request(
        app,
        mode,
        "session.start",
        json!({
            "sessionId": session_id,
            "model": session.model,
            "mode": session.mode,
            "workspace": workspace.to_string_lossy(),
            "history": history,
        }),
    );

    // Title new sessions from the first message.
    if session.title == "New session" {
        let title: String = text.chars().take(48).collect();
        let _ = sqlx::query("UPDATE agent_sessions SET title = ?2 WHERE id = ?1")
            .bind(&session_id)
            .bind(title.trim())
            .execute(pool)
            .await;
    }

    sqlx::query(
        "INSERT INTO agent_messages (id, session_id, role, content_json, created_at)
         VALUES (?1, ?2, 'user', ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&session_id)
    .bind(json!({ "text": text, "tools": [] }).to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    runtime.request(
        app,
        mode,
        "session.message",
        json!({ "sessionId": session_id, "text": text }),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn agent_steer(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let mode = session_mode(&pool, &session_id).await?;
    runtime.request(
        &app,
        mode,
        "session.steer",
        json!({ "sessionId": session_id, "text": text }),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn agent_cancel(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    session_id: String,
) -> Result<(), String> {
    let mode = session_mode(&pool, &session_id).await?;
    runtime.request(
        &app,
        mode,
        "session.cancel",
        json!({ "sessionId": session_id }),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn agent_resolve_approval(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    session_id: String,
    call_id: String,
    decision: String,
) -> Result<(), String> {
    let mode = session_mode(&pool, &session_id).await?;
    runtime.request(
        &app,
        mode,
        "approval.resolve",
        json!({ "sessionId": session_id, "callId": call_id, "decision": decision }),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn agent_delete_session(
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM agent_sessions WHERE id = ?1")
        .bind(session_id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn session_mode(pool: &SqlitePool, session_id: &str) -> Result<WriteMode, String> {
    let mode = sqlx::query_scalar::<_, String>("SELECT mode FROM agent_sessions WHERE id = ?1")
        .bind(session_id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(WriteMode::parse(&mode))
}
