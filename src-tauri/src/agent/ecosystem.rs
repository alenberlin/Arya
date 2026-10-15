//! Agent ecosystem: MCP server registry, scheduled routines, and session
//! branching.

use serde_json::json;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};

use super::sidecar::WriteMode;
use super::AgentRuntime;

// ---- MCP servers --------------------------------------------------------

#[derive(Debug, serde::Serialize, sqlx::FromRow, specta::Type)]
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

/// Keychain service for MCP env blobs (they often carry API tokens, so they
/// must not sit in SQLite as plaintext — same posture as provider keys).
const MCP_ENV_SERVICE: &str = "dev.arya.app.mcp-env";

fn mcp_env_entry(name: &str) -> crate::error::AryaResult<keyring::Entry> {
    keyring::Entry::new(MCP_ENV_SERVICE, name).map_err(crate::error::AryaError::from_display)
}

/// Store an MCP server's env JSON in the Keychain. Returns false when the
/// Keychain is unavailable, so the caller can keep a plaintext fallback instead
/// of silently dropping the user's config.
fn mcp_env_store(name: &str, env_json: &str) -> bool {
    mcp_env_entry(name)
        .and_then(|e| {
            e.set_password(env_json)
                .map_err(crate::error::AryaError::from_display)
        })
        .is_ok()
}

fn mcp_env_load(name: &str) -> Option<String> {
    mcp_env_entry(name)
        .ok()?
        .get_password()
        .ok()
        .filter(|s| !s.is_empty())
}

fn mcp_env_delete(name: &str) {
    if let Ok(entry) = mcp_env_entry(name) {
        let _ = entry.delete_credential();
    }
}

#[specta::specta]
#[tauri::command]
pub async fn mcp_list_servers(
    pool: State<'_, SqlitePool>,
) -> crate::error::AryaResult<Vec<McpServerRow>> {
    let mut servers = sqlx::query_as::<_, McpServerRow>(
        "SELECT id, name, command, args_json, env_json, enabled, created_at
         FROM mcp_servers ORDER BY name",
    )
    .fetch_all(&*pool)
    .await
    .map_err(crate::error::AryaError::from_display)?;
    // Env stored in the Keychain is returned decrypted so the UI can edit it;
    // a legacy/fallback plaintext row is used as-is.
    for server in &mut servers {
        if server.env_json.is_empty() {
            if let Some(env) = mcp_env_load(&server.name) {
                server.env_json = env;
            }
        }
    }
    Ok(servers)
}

#[specta::specta]
#[tauri::command]
pub async fn mcp_add_server(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    name: String,
    command: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
) -> crate::error::AryaResult<Vec<String>> {
    let name = name.trim().to_string();
    if name.is_empty() || command.trim().is_empty() {
        return Err("name and command are required".into());
    }
    let args_json = serde_json::to_string(&args).map_err(crate::error::AryaError::from_display)?;
    let env_json = serde_json::to_string(&env).map_err(crate::error::AryaError::from_display)?;
    // Keep the env (which often carries API tokens) in the Keychain, not SQLite
    // plaintext. If the Keychain is unavailable, fall back to plaintext rather
    // than dropping the user's config.
    let stored_secure = mcp_env_store(&name, &env_json);
    let db_env = if stored_secure {
        String::new()
    } else {
        env_json.clone()
    };
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
    .bind(&db_env)
    .execute(&*pool)
    .await
    .map_err(crate::error::AryaError::from_display)?;

    // Connect immediately in the sandboxed sidecar so tools appear now.
    let params = json!({ "name": name, "command": command, "args": args, "env": env });
    let mut tools = Vec::new();
    if let Ok(result) = runtime.request(&app, WriteMode::Sandboxed, "mcp.connect", params) {
        if let Some(list) = result.get("tools").and_then(|t| t.as_array()) {
            tools = list
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
    }
    Ok(tools)
}

#[specta::specta]
#[tauri::command]
pub async fn mcp_remove_server(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    runtime: State<'_, AgentRuntime>,
    id: String,
) -> crate::error::AryaResult<()> {
    let name = sqlx::query_scalar::<_, String>("SELECT name FROM mcp_servers WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&*pool)
        .await
        .map_err(crate::error::AryaError::from_display)?;
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(crate::error::AryaError::from_display)?;
    if let Some(name) = &name {
        mcp_env_delete(name);
    }
    if let Some(name) = name {
        let params = json!({ "name": name });
        let _ = runtime.request(&app, WriteMode::Sandboxed, "mcp.disconnect", params);
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
        // Env lives in the Keychain when set; a legacy plaintext row is the
        // fallback for servers added before this change (or a failed Keychain).
        let env_json = if server.env_json.is_empty() {
            mcp_env_load(&server.name).unwrap_or_default()
        } else {
            server.env_json
        };
        let env: std::collections::HashMap<String, String> =
            serde_json::from_str(&env_json).unwrap_or_default();
        let params =
            json!({ "name": server.name, "command": server.command, "args": args, "env": env });
        let _ = runtime.request(app, WriteMode::Sandboxed, "mcp.connect", params);
    }
}

// ---- Routines -----------------------------------------------------------

#[derive(Debug, serde::Serialize, sqlx::FromRow, specta::Type)]
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

#[derive(Debug, serde::Serialize, sqlx::FromRow, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRunRow {
    pub id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub detail: Option<String>,
    pub started_at: String,
}

#[specta::specta]
#[tauri::command]
pub async fn routine_list(
    pool: State<'_, SqlitePool>,
) -> crate::error::AryaResult<Vec<RoutineRow>> {
    sqlx::query_as::<_, RoutineRow>(
        "SELECT id, title, prompt, model, mode, interval_minutes, enabled,
                last_run_at, next_run_at, created_at
         FROM routines ORDER BY created_at DESC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(crate::error::AryaError::from_display)
}

#[specta::specta]
#[tauri::command]
pub async fn routine_create(
    pool: State<'_, SqlitePool>,
    title: String,
    prompt: String,
    model: String,
    interval_minutes: i64,
) -> crate::error::AryaResult<RoutineRow> {
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
    .map_err(crate::error::AryaError::from_display)
}

#[specta::specta]
#[tauri::command]
pub async fn routine_set_enabled(
    pool: State<'_, SqlitePool>,
    id: String,
    enabled: bool,
) -> crate::error::AryaResult<()> {
    sqlx::query("UPDATE routines SET enabled = ?2 WHERE id = ?1")
        .bind(id)
        .bind(i64::from(enabled))
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(crate::error::AryaError::from_display)
}

#[specta::specta]
#[tauri::command]
pub async fn routine_delete(
    pool: State<'_, SqlitePool>,
    id: String,
) -> crate::error::AryaResult<()> {
    sqlx::query("DELETE FROM routines WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(crate::error::AryaError::from_display)
}

#[specta::specta]
#[tauri::command]
pub async fn routine_runs(
    pool: State<'_, SqlitePool>,
    routine_id: String,
) -> crate::error::AryaResult<Vec<RoutineRunRow>> {
    sqlx::query_as::<_, RoutineRunRow>(
        "SELECT id, session_id, status, detail, started_at
         FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC LIMIT 50",
    )
    .bind(routine_id)
    .fetch_all(&*pool)
    .await
    .map_err(crate::error::AryaError::from_display)
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
                let _ = record_run(
                    pool,
                    &run_id,
                    &routine.id,
                    None,
                    "failed",
                    Some(&e.to_string()),
                )
                .await;
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
        // Only a delivery failure ends the run here. On success it stays
        // "running": agent_send_inner merely accepts the message, and the
        // turn-finished event is what actually marks the run "done" (the old
        // code reported "done" the moment the message was accepted).
        if let Err(e) = result {
            let _ =
                sqlx::query("UPDATE routine_runs SET status = 'failed', detail = ?2 WHERE id = ?1")
                    .bind(&run_id)
                    .bind(e.to_string())
                    .execute(pool)
                    .await;
        }
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
#[specta::specta]
#[tauri::command]
pub async fn agent_branch_session(
    pool: State<'_, SqlitePool>,
    session_id: String,
    through_message_id: String,
) -> crate::error::AryaResult<super::commands::AgentSession> {
    let source = sqlx::query_as::<_, super::commands::AgentSession>(
        "SELECT id, title, model, mode, created_at, updated_at FROM agent_sessions WHERE id = ?1",
    )
    .bind(&session_id)
    .fetch_one(&*pool)
    .await
    .map_err(crate::error::AryaError::from_display)?;

    let cutoff =
        sqlx::query_scalar::<_, String>("SELECT created_at FROM agent_messages WHERE id = ?1")
            .bind(&through_message_id)
            .fetch_one(&*pool)
            .await
            .map_err(crate::error::AryaError::from_display)?;

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
    .map_err(crate::error::AryaError::from_display)?;

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
    .map_err(crate::error::AryaError::from_display)?;

    Ok(new_session)
}
