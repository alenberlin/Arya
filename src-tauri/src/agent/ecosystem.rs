//! Agent ecosystem: MCP server registry, scheduled routines, and session
//! branching.

use serde_json::json;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};

use super::sidecar::WriteMode;
use super::AgentRuntime;

// ---- MCP servers --------------------------------------------------------

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct McpServerRow {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args_json: String,
    pub env_json: String,
    pub enabled: i64,
    pub created_at: String,
}

#[tauri::command]
pub async fn mcp_list_servers(pool: State<'_, SqlitePool>) -> Result<Vec<McpServerRow>, String> {
    sqlx::query_as::<_, McpServerRow>(
        "SELECT id, name, command, args_json, env_json, enabled, created_at
         FROM mcp_servers ORDER BY name",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_add_server(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    name: String,
    command: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
) -> Result<Vec<String>, String> {
    let name = name.trim().to_string();
    if name.is_empty() || command.trim().is_empty() {
        return Err("name and command are required".into());
    }
    let args_json = serde_json::to_string(&args).map_err(|e| e.to_string())?;
    let env_json = serde_json::to_string(&env).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO mcp_servers (id, name, command, args_json, env_json, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(name) DO UPDATE SET command = excluded.command,
             args_json = excluded.args_json, env_json = excluded.env_json",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&name)
    .bind(&command)
    .bind(&args_json)
    .bind(&env_json)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    // Connect immediately in both write-modes' sidecars so tools appear now.
    let params = json!({ "name": name, "command": command, "args": args, "env": env });
    let mut tools = Vec::new();
    if let Ok(result) = runtime.request(&app, WriteMode::Sandboxed, "mcp.connect", params.clone()) {
        if let Some(list) = result.get("tools").and_then(|t| t.as_array()) {
            tools = list
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
    }
    let _ = runtime.request(&app, WriteMode::Unrestricted, "mcp.connect", params);
    Ok(tools)
}

#[tauri::command]
pub async fn mcp_remove_server(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    id: String,
) -> Result<(), String> {
    let name = sqlx::query_scalar::<_, String>("SELECT name FROM mcp_servers WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(name) = name {
        let params = json!({ "name": name });
        let _ = runtime.request(&app, WriteMode::Sandboxed, "mcp.disconnect", params.clone());
        let _ = runtime.request(&app, WriteMode::Unrestricted, "mcp.disconnect", params);
    }
    Ok(())
}

/// Reconnects all enabled MCP servers into a freshly (re)started sidecar.
pub async fn reconnect_all(app: &AppHandle, pool: &SqlitePool, runtime: &AgentRuntime) {
    let servers = sqlx::query_as::<_, McpServerRow>(
        "SELECT id, name, command, args_json, env_json, enabled, created_at
         FROM mcp_servers WHERE enabled = 1",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for server in servers {
        let args: Vec<String> = serde_json::from_str(&server.args_json).unwrap_or_default();
        let env: std::collections::HashMap<String, String> =
            serde_json::from_str(&server.env_json).unwrap_or_default();
        let params =
            json!({ "name": server.name, "command": server.command, "args": args, "env": env });
        let _ = runtime.request(app, WriteMode::Sandboxed, "mcp.connect", params);
    }
}

// ---- Routines -----------------------------------------------------------

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRow {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub model: String,
    pub mode: String,
    pub interval_minutes: i64,
    pub enabled: i64,
    pub last_run_at: Option<String>,
    pub next_run_at: String,
    pub created_at: String,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRunRow {
    pub id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub detail: Option<String>,
    pub started_at: String,
}

#[tauri::command]
pub async fn routine_list(pool: State<'_, SqlitePool>) -> Result<Vec<RoutineRow>, String> {
    sqlx::query_as::<_, RoutineRow>(
        "SELECT id, title, prompt, model, mode, interval_minutes, enabled,
                last_run_at, next_run_at, created_at
         FROM routines ORDER BY created_at DESC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn routine_create(
    pool: State<'_, SqlitePool>,
    title: String,
    prompt: String,
    model: String,
    interval_minutes: i64,
) -> Result<RoutineRow, String> {
    if title.trim().is_empty() || prompt.trim().is_empty() {
        return Err("title and prompt are required".into());
    }
    if interval_minutes < 1 {
        return Err("interval must be at least one minute".into());
    }
    sqlx::query_as::<_, RoutineRow>(
        "INSERT INTO routines
             (id, title, prompt, model, mode, interval_minutes, enabled, next_run_at, created_at)
         VALUES (?1, ?2, ?3, ?4, 'sandboxed', ?5, 1,
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ?5 || ' minutes'),
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, prompt, model, mode, interval_minutes, enabled,
                   last_run_at, next_run_at, created_at",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(title.trim())
    .bind(prompt.trim())
    .bind(model)
    .bind(interval_minutes)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn routine_set_enabled(
    pool: State<'_, SqlitePool>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE routines SET enabled = ?2 WHERE id = ?1")
        .bind(id)
        .bind(i64::from(enabled))
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn routine_delete(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    sqlx::query("DELETE FROM routines WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn routine_runs(
    pool: State<'_, SqlitePool>,
    routine_id: String,
) -> Result<Vec<RoutineRunRow>, String> {
    sqlx::query_as::<_, RoutineRunRow>(
        "SELECT id, session_id, status, detail, started_at
         FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC LIMIT 50",
    )
    .bind(routine_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

/// Runs any routine whose next_run_at has passed, creating an agent session
/// per run and rescheduling. Called by the scheduler tick.
pub async fn run_due_routines(app: &AppHandle, pool: &SqlitePool, runtime: &AgentRuntime) {
    let due = sqlx::query_as::<_, RoutineRow>(
        "SELECT id, title, prompt, model, mode, interval_minutes, enabled,
                last_run_at, next_run_at, created_at
         FROM routines
         WHERE enabled = 1 AND next_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for routine in due {
        // Reschedule first so a slow run cannot double-fire.
        let _ = sqlx::query(
            "UPDATE routines SET last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 next_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || interval_minutes || ' minutes')
             WHERE id = ?1",
        )
        .bind(&routine.id)
        .execute(pool)
        .await;

        let run_id = uuid::Uuid::new_v4().to_string();
        let session = super::commands::agent_create_session_inner(
            app,
            pool,
            runtime,
            routine.model.clone(),
            Some(routine.mode.clone()),
        )
        .await;
        let session_id = match session {
            Ok(s) => s.id,
            Err(e) => {
                let _ = record_run(pool, &run_id, &routine.id, None, "failed", Some(&e)).await;
                continue;
            }
        };
        let _ = record_run(
            pool,
            &run_id,
            &routine.id,
            Some(&session_id),
            "running",
            None,
        )
        .await;
        let _ = app.emit(
            "routine:started",
            json!({ "routineId": routine.id, "sessionId": session_id }),
        );

        let result = super::commands::agent_send_inner(
            app,
            pool,
            runtime,
            session_id.clone(),
            routine.prompt.clone(),
        )
        .await;
        let (status, detail) = match result {
            Ok(()) => ("done", None),
            Err(e) => ("failed", Some(e)),
        };
        let _ = sqlx::query("UPDATE routine_runs SET status = ?2, detail = ?3 WHERE id = ?1")
            .bind(&run_id)
            .bind(status)
            .bind(detail)
            .execute(pool)
            .await;
    }
}

async fn record_run(
    pool: &SqlitePool,
    run_id: &str,
    routine_id: &str,
    session_id: Option<&str>,
    status: &str,
    detail: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO routine_runs (id, routine_id, session_id, status, detail, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    )
    .bind(run_id)
    .bind(routine_id)
    .bind(session_id)
    .bind(status)
    .bind(detail)
    .execute(pool)
    .await
    .map(|_| ())
}

// ---- Session branching --------------------------------------------------

/// Forks a session into a new one carrying its history up to (and including)
/// `through_message_id`, so the user can explore an alternative from a point.
#[tauri::command]
pub async fn agent_branch_session(
    pool: State<'_, SqlitePool>,
    session_id: String,
    through_message_id: String,
) -> Result<super::commands::AgentSession, String> {
    let source = sqlx::query_as::<_, super::commands::AgentSession>(
        "SELECT id, title, model, mode, created_at, updated_at FROM agent_sessions WHERE id = ?1",
    )
    .bind(&session_id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let cutoff =
        sqlx::query_scalar::<_, String>("SELECT created_at FROM agent_messages WHERE id = ?1")
            .bind(&through_message_id)
            .fetch_one(&*pool)
            .await
            .map_err(|e| e.to_string())?;

    let new_id = uuid::Uuid::new_v4().to_string();
    let new_session = sqlx::query_as::<_, super::commands::AgentSession>(
        "INSERT INTO agent_sessions (id, title, model, mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, model, mode, created_at, updated_at",
    )
    .bind(&new_id)
    .bind(format!("{} (branch)", source.title))
    .bind(&source.model)
    .bind(&source.mode)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        "INSERT INTO agent_messages (id, session_id, role, content_json, created_at)
         SELECT lower(hex(randomblob(16))), ?1, role, content_json, created_at
         FROM agent_messages WHERE session_id = ?2 AND created_at <= ?3 ORDER BY created_at",
    )
    .bind(&new_id)
    .bind(&session_id)
    .bind(&cutoff)
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(new_session)
}
