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

/// The visible text of one persisted message (`content_json` is
/// `{text, reasoning?, tools}`; only `text` is the conversation itself).
fn message_text(content_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(content_json)
        .ok()
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_default()
}

/// Render a conversation as a readable markdown transcript (You / Arya turns),
/// skipping empty turns. Shared so a converted chat reads the same everywhere.
fn transcript_markdown(messages: &[AgentMessage]) -> String {
    let mut md = String::new();
    for m in messages {
        let text = message_text(&m.content_json);
        if text.trim().is_empty() {
            continue;
        }
        let speaker = if m.role == "user" { "You" } else { "Arya" };
        md.push_str(&format!("**{speaker}:** {}\n\n", text.trim()));
    }
    md.trim_end().to_string()
}

/// Converts an agent chat into a note: the conversation becomes a markdown
/// transcript, titled from the session (or its first line). The note then joins
/// the connected brain like any other. Returns the new note id. Translating a
/// chat is this plus a side-by-side note translation, composed on the frontend.
#[tauri::command]
pub async fn convert_session_to_note(
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<String, String> {
    let title: String = sqlx::query_scalar("SELECT title FROM agent_sessions WHERE id = ?1")
        .bind(&session_id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "chat not found".to_string())?;

    let messages: Vec<AgentMessage> = sqlx::query_as::<_, AgentMessage>(
        "SELECT id, role, content_json, created_at
         FROM agent_messages WHERE session_id = ?1 ORDER BY created_at",
    )
    .bind(&session_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let body = transcript_markdown(&messages);
    if body.trim().is_empty() {
        return Err("this chat has no messages to convert".into());
    }
    let note_title = if title.trim().is_empty() || title == "New session" {
        crate::notes::title_from_text(&body)
    } else {
        title
    };

    let note = crate::notes::insert_note(&pool, &note_title)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "UPDATE notes SET body_md = ?2, processing_status = 'ready',
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(&note.id)
    .bind(&body)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(note.id)
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

    let workspace = super::agent_workspace(app)?;

    // Title new sessions from the first message.
    if session.title == "New session" {
        let title: String = text.chars().take(48).collect();
        let _ = sqlx::query("UPDATE agent_sessions SET title = ?2 WHERE id = ?1")
            .bind(&session_id)
            .bind(title.trim())
            .execute(pool)
            .await;
    }

    // Persist the user message before delivery so a restore below reads it back.
    let user_msg_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO agent_messages (id, session_id, role, content_json, created_at)
         VALUES (?1, ?2, 'user', ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(&user_msg_id)
    .bind(&session_id)
    .bind(json!({ "text": text, "tools": [] }).to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // Deliver only the new message: the sidecar keeps the running conversation
    // in memory, so re-seeding the whole history is done ONLY when the sidecar
    // has no session for this id (the process restarted). Previously session.start
    // re-sent the entire transcript on every message — O(n^2) in tokens/transport.
    let deliver = json!({ "sessionId": &session_id, "text": &text });
    match runtime.request(app, mode, "session.message", deliver.clone()) {
        Ok(_) => Ok(()),
        Err(e) if e.contains("unknown session") => {
            let history = restore_history(pool, &session_id, &user_msg_id).await?;
            runtime.request(
                app,
                mode,
                "session.start",
                json!({
                    "sessionId": &session_id,
                    "model": session.model,
                    "mode": session.mode,
                    "workspace": workspace.to_string_lossy(),
                    "history": history,
                }),
            )?;
            runtime.request(app, mode, "session.message", deliver)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Prior turns for restoring a sidecar session after a restart, excluding the
/// just-inserted current message (which `session.message` re-delivers, so
/// including it would duplicate the user turn in the model's context).
async fn restore_history(
    pool: &SqlitePool,
    session_id: &str,
    exclude_message_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(sqlx::query_as::<_, (String, String)>(
        "SELECT role, content_json FROM agent_messages
         WHERE session_id = ?1 AND id != ?2 ORDER BY created_at",
    )
    .bind(session_id)
    .bind(exclude_message_id)
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
    .collect())
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
    runtime: State<'_, AgentRuntime>,
    session_id: String,
) -> Result<(), String> {
    agent_delete_session_inner(&pool, &runtime, session_id).await
}

pub async fn agent_delete_session_inner(
    pool: &SqlitePool,
    runtime: &AgentRuntime,
    session_id: String,
) -> Result<(), String> {
    let mode = sqlx::query_scalar::<_, String>("SELECT mode FROM agent_sessions WHERE id = ?1")
        .bind(&session_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(mode) = mode {
        runtime.end_session_if_running(WriteMode::parse(&mode), &session_id);
    } else {
        runtime.clear_session_state(&session_id);
    }
    sqlx::query("DELETE FROM agent_sessions WHERE id = ?1")
        .bind(session_id)
        .execute(pool)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    fn msg(role: &str, text: &str) -> AgentMessage {
        AgentMessage {
            id: "x".into(),
            role: role.into(),
            content_json: json!({ "text": text, "tools": [] }).to_string(),
            created_at: "2026-01-01".into(),
        }
    }

    #[test]
    fn transcript_renders_turns_and_skips_empties() {
        let messages = vec![
            msg("user", "Plan the migration"),
            msg("assistant", "Here's a plan."),
            msg("assistant", "   "), // empty → skipped
        ];
        let md = transcript_markdown(&messages);
        assert_eq!(
            md,
            "**You:** Plan the migration\n\n**Arya:** Here's a plan."
        );
    }

    #[test]
    fn message_text_pulls_text_field_only() {
        let c = json!({ "text": "hi", "reasoning": "secret", "tools": [] }).to_string();
        assert_eq!(message_text(&c), "hi");
        assert_eq!(message_text("not json"), "");
    }

    #[tokio::test]
    async fn delete_session_removes_row_and_runtime_state() {
        let pool = test_pool().await;
        let runtime = AgentRuntime::default();
        let session_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO agent_sessions (id, title, model, mode, created_at, updated_at)
             VALUES (?1, 'Doomed', 'ollama:test', 'sandboxed',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(&session_id)
        .execute(&pool)
        .await
        .unwrap();
        runtime.accumulators.lock().unwrap().insert(
            session_id.clone(),
            super::super::TurnAccumulator {
                text: "partial".into(),
                reasoning: String::new(),
                tools: vec![],
            },
        );

        agent_delete_session_inner(&pool, &runtime, session_id.clone())
            .await
            .unwrap();

        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_sessions WHERE id = ?1")
                .bind(&session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(exists, 0);
        assert!(!runtime
            .accumulators
            .lock()
            .unwrap()
            .contains_key(&session_id));
    }
}
