//! File attachments for notes.
//!
//! Picked files are **copied** into the app's workspace (so a note is
//! self-contained), listed, opened in the OS default app, and removed with
//! their files. When a whole note is deleted, [`crate::notes::delete_note`]
//! removes the attachment files after deleting the row (the rows cascade on the
//! note's foreign key).

use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub note_id: String,
    pub name: String,
    pub path: String,
    pub size_bytes: i64,
    pub created_at: String,
}

/// Copy `source` into `<attachments_root>/<note_id>/<uuid>-<name>`, record it,
/// and return the row. The testable core of [`attach_file`].
pub async fn insert_attachment(
    pool: &SqlitePool,
    attachments_root: &Path,
    note_id: &str,
    source: &Path,
) -> Result<Attachment, String> {
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "attachment has no file name".to_string())?
        .to_string();
    let dir = attachments_root.join(note_id);
    let id = uuid::Uuid::new_v4().to_string();
    // The uuid prefix keeps the workspace name unique even if two attachments
    // share an original filename.
    let dest = dir.join(format!("{id}-{name}"));
    let path = dest.to_string_lossy().to_string();
    // Copying a (possibly large) file must not block the async runtime thread.
    let size_bytes = {
        let source = source.to_path_buf();
        tauri::async_runtime::spawn_blocking(move || -> Result<i64, String> {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::copy(&source, &dest).map_err(|e| format!("copy failed: {e}"))?;
            Ok(std::fs::metadata(&dest)
                .map(|m| m.len() as i64)
                .unwrap_or(0))
        })
        .await
        .map_err(|e| e.to_string())??
    };

    sqlx::query_as::<_, Attachment>(
        "INSERT INTO note_attachments (id, note_id, name, path, size_bytes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, note_id, name, path, size_bytes, created_at",
    )
    .bind(&id)
    .bind(note_id)
    .bind(&name)
    .bind(&path)
    .bind(size_bytes)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())
}

pub async fn list_for_note(pool: &SqlitePool, note_id: &str) -> Result<Vec<Attachment>, String> {
    sqlx::query_as::<_, Attachment>(
        "SELECT id, note_id, name, path, size_bytes, created_at
         FROM note_attachments WHERE note_id = ?1 ORDER BY created_at",
    )
    .bind(note_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

/// Delete one attachment's row and its file.
pub async fn delete_attachment(pool: &SqlitePool, id: &str) -> Result<(), String> {
    let path: Option<String> =
        sqlx::query_scalar("SELECT path FROM note_attachments WHERE id = ?1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some(path) = path {
        let _ = std::fs::remove_file(&path);
    }
    sqlx::query("DELETE FROM note_attachments WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn attachments_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments"))
}

/// Open a path in the OS default app (macOS `open`), mirroring the account
/// module's URL opener.
fn open_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("unsupported platform".into())
    }
}

#[tauri::command]
pub async fn attach_file(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    note_id: String,
    source_path: String,
) -> Result<Attachment, String> {
    let root = attachments_root(&app)?;
    insert_attachment(&pool, &root, &note_id, Path::new(&source_path)).await
}

#[tauri::command]
pub async fn list_attachments(
    pool: State<'_, SqlitePool>,
    note_id: String,
) -> Result<Vec<Attachment>, String> {
    list_for_note(&pool, &note_id).await
}

#[tauri::command]
pub async fn remove_attachment(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    delete_attachment(&pool, &id).await
}

#[tauri::command]
pub async fn open_attachment(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    let path: String = sqlx::query_scalar("SELECT path FROM note_attachments WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "attachment not found".to_string())?;
    open_path(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::notes::insert_note;

    #[tokio::test]
    async fn attach_list_remove_round_trip() {
        let pool = test_pool().await;
        let note = insert_note(&pool, "n").await.unwrap();
        let tmp = std::env::temp_dir().join(format!("arya-att-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("data.csv");
        std::fs::write(&src, b"a,b,c\n1,2,3\n").unwrap();
        let root = tmp.join("attachments");

        let att = insert_attachment(&pool, &root, &note.id, &src)
            .await
            .unwrap();
        assert_eq!(att.name, "data.csv");
        assert!(Path::new(&att.path).exists(), "file copied into workspace");
        assert!(att.size_bytes > 0);

        let list = list_for_note(&pool, &note.id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, att.id);

        delete_attachment(&pool, &att.id).await.unwrap();
        assert!(!Path::new(&att.path).exists(), "file removed on delete");
        assert!(list_for_note(&pool, &note.id).await.unwrap().is_empty());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn delete_note_removes_row_then_files() {
        let pool = test_pool().await;
        let note = insert_note(&pool, "n").await.unwrap();
        let tmp = std::env::temp_dir().join(format!("arya-del-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("a.txt");
        std::fs::write(&src, b"hi").unwrap();
        let root = tmp.join("attachments");
        let att = insert_attachment(&pool, &root, &note.id, &src)
            .await
            .unwrap();
        assert!(Path::new(&att.path).exists());

        crate::notes::delete_note_inner(&pool, &note.id)
            .await
            .unwrap();

        assert!(
            !Path::new(&att.path).exists(),
            "attachment file removed after the row delete"
        );
        assert!(
            list_for_note(&pool, &note.id).await.unwrap().is_empty(),
            "rows gone with the note"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
