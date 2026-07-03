use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

/// A note row as created (walking-skeleton shape, still used by inserts).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

/// Summary row for lists.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub processing_status: String,
    pub processing_error: Option<String>,
    pub folder_id: Option<String>,
    pub created_at: String,
}

/// Full note payload for the editor.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    pub id: String,
    pub title: String,
    pub body_md: String,
    pub manual_notes: String,
    pub processing_status: String,
    pub processing_error: Option<String>,
    pub folder_id: Option<String>,
    pub calendar_context: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptTurn {
    pub turn_index: i64,
    pub source: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

pub async fn insert_note(pool: &SqlitePool, title: &str) -> Result<Note, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query_as::<_, Note>(
        "INSERT INTO notes (id, title, created_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, created_at",
    )
    .bind(&id)
    .bind(title)
    .fetch_one(pool)
    .await
}

pub async fn fetch_notes(pool: &SqlitePool) -> Result<Vec<NoteSummary>, sqlx::Error> {
    sqlx::query_as::<_, NoteSummary>(
        "SELECT id, title, processing_status, processing_error, folder_id, created_at
         FROM notes ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
}

#[tauri::command]
pub async fn create_note(pool: State<'_, SqlitePool>, title: String) -> Result<Note, String> {
    insert_note(&pool, &title).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_notes(pool: State<'_, SqlitePool>) -> Result<Vec<NoteSummary>, String> {
    fetch_notes(&pool).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_note(pool: State<'_, SqlitePool>, id: String) -> Result<NoteDetail, String> {
    sqlx::query_as::<_, NoteDetail>(
        "SELECT id, title, body_md, manual_notes, processing_status, processing_error,
                folder_id, calendar_context, created_at
         FROM notes WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_note_turns(
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<Vec<TranscriptTurn>, String> {
    sqlx::query_as::<_, TranscriptTurn>(
        "SELECT turn_index, source, start_ms, end_ms, speaker, text
         FROM transcript_turns WHERE note_id = ?1 ORDER BY turn_index",
    )
    .bind(id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_note(
    pool: State<'_, SqlitePool>,
    id: String,
    title: Option<String>,
    body_md: Option<String>,
    manual_notes: Option<String>,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes SET
             title = COALESCE(?2, title),
             body_md = COALESCE(?3, body_md),
             manual_notes = COALESCE(?4, manual_notes),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(id)
    .bind(title)
    .bind(body_md)
    .bind(manual_notes)
    .execute(&*pool)
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Deletes a note and its recordings from disk (rows cascade).
#[tauri::command]
pub async fn delete_note(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    let paths = sqlx::query_scalar::<_, String>(
        "SELECT a.path FROM audio_artifacts a
         JOIN recording_sessions s ON s.id = a.session_id WHERE s.note_id = ?1",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())?;
    for path in paths {
        let path = std::path::PathBuf::from(path);
        let _ = std::fs::remove_file(&path);
        if let Some(dir) = path.parent() {
            // Remove the per-session directory when empty.
            let _ = std::fs::remove_dir(dir);
        }
    }
    sqlx::query("DELETE FROM notes WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_folder(pool: State<'_, SqlitePool>, name: String) -> Result<Folder, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("folder name is required".into());
    }
    sqlx::query_as::<_, Folder>(
        "INSERT INTO folders (id, name, created_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, name, created_at",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&name)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_folders(pool: State<'_, SqlitePool>) -> Result<Vec<Folder>, String> {
    sqlx::query_as::<_, Folder>("SELECT id, name, created_at FROM folders ORDER BY name")
        .fetch_all(&*pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_folder(
    pool: State<'_, SqlitePool>,
    id: String,
    name: String,
) -> Result<(), String> {
    sqlx::query("UPDATE folders SET name = ?2 WHERE id = ?1")
        .bind(id)
        .bind(name.trim())
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Deletes a folder; notes inside revert to no folder.
#[tauri::command]
pub async fn delete_folder(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    sqlx::query("UPDATE notes SET folder_id = NULL WHERE folder_id = ?1")
        .bind(&id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM folders WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn assign_note_to_folder(
    pool: State<'_, SqlitePool>,
    note_id: String,
    folder_id: Option<String>,
) -> Result<(), String> {
    sqlx::query("UPDATE notes SET folder_id = ?2 WHERE id = ?1")
        .bind(note_id)
        .bind(folder_id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn insert_then_fetch_round_trips() {
        let pool = test_pool().await;
        let created = insert_note(&pool, "First note").await.expect("insert");
        assert_eq!(created.title, "First note");
        let notes = fetch_notes(&pool).await.expect("fetch");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, created.id);
        assert_eq!(notes[0].processing_status, "idle");
    }

    #[tokio::test]
    async fn folders_assign_and_delete_detaches_notes() {
        let pool = test_pool().await;
        let note = insert_note(&pool, "n").await.unwrap();
        let folder = sqlx::query_as::<_, Folder>(
            "INSERT INTO folders (id, name, created_at)
             VALUES ('f1', 'Work', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             RETURNING id, name, created_at",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE notes SET folder_id = ?2 WHERE id = ?1")
            .bind(&note.id)
            .bind(&folder.id)
            .execute(&pool)
            .await
            .unwrap();
        // Detach + delete.
        sqlx::query("UPDATE notes SET folder_id = NULL WHERE folder_id = 'f1'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM folders WHERE id = 'f1'")
            .execute(&pool)
            .await
            .unwrap();
        let notes = fetch_notes(&pool).await.unwrap();
        assert_eq!(notes[0].folder_id, None);
    }

    #[tokio::test]
    async fn turns_round_trip_for_note() {
        let pool = test_pool().await;
        let note = insert_note(&pool, "with turns").await.unwrap();
        sqlx::query(
            "INSERT INTO transcript_turns (id, note_id, source, turn_index, start_ms, end_ms, text)
             VALUES ('t1', ?1, 'microphone', 0, 0, 1500, 'hello world')",
        )
        .bind(&note.id)
        .execute(&pool)
        .await
        .unwrap();
        let turns = sqlx::query_as::<_, TranscriptTurn>(
            "SELECT turn_index, source, start_ms, end_ms, speaker, text
             FROM transcript_turns WHERE note_id = ?1 ORDER BY turn_index",
        )
        .bind(&note.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].text, "hello world");
        assert_eq!(turns[0].end_ms, 1500);
    }
}
