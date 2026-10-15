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
use tauri::{
    ipc::{InvokeBody, Request},
    AppHandle, Manager, State,
};

use crate::filestage::{header, safe_file_name, stage, FileSource};

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

/// Stage `source` into `<attachments_root>/<note_id>/<uuid>-<name>`, record it,
/// and return the row. The testable core of [`attach_file`] and
/// [`attach_dropped_file`].
pub async fn insert_attachment(
    pool: &SqlitePool,
    attachments_root: &Path,
    note_id: &str,
    name: &str,
    source: FileSource,
) -> crate::error::AryaResult<Attachment> {
    let name = safe_file_name(name);
    let dir = attachments_root.join(note_id);
    let id = uuid::Uuid::new_v4().to_string();
    let (path, size_bytes) = stage(dir, &id, &name, source).await?;

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
    .map_err(crate::error::AryaError::from_display)
}

pub async fn list_for_note(
    pool: &SqlitePool,
    note_id: &str,
) -> crate::error::AryaResult<Vec<Attachment>> {
    sqlx::query_as::<_, Attachment>(
        "SELECT id, note_id, name, path, size_bytes, created_at
         FROM note_attachments WHERE note_id = ?1 ORDER BY created_at",
    )
    .bind(note_id)
    .fetch_all(pool)
    .await
    .map_err(crate::error::AryaError::from_display)
}

/// Delete one attachment's row and its file.
pub async fn delete_attachment(pool: &SqlitePool, id: &str) -> crate::error::AryaResult<()> {
    let path: Option<String> =
        sqlx::query_scalar("SELECT path FROM note_attachments WHERE id = ?1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(crate::error::AryaError::from_display)?;
    if let Some(path) = path {
        let _ = std::fs::remove_file(&path);
    }
    sqlx::query("DELETE FROM note_attachments WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(crate::error::AryaError::from_display)
}

fn attachments_root(app: &AppHandle) -> crate::error::AryaResult<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(crate::error::AryaError::from_display)?
        .join("attachments"))
}

/// Open a path in the OS default app (macOS `open`), mirroring the account
/// module's URL opener.
fn open_path(path: &str) -> crate::error::AryaResult<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(crate::error::AryaError::from_display)
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
) -> crate::error::AryaResult<Attachment> {
    let root = attachments_root(&app)?;
    insert_attachment(
        &pool,
        &root,
        &note_id,
        &source_path,
        FileSource::Path(PathBuf::from(&source_path)),
    )
    .await
}

/// Attach a file dropped onto the window.
///
/// The window keeps the OS drag handler off so in-page dragging still works,
/// which leaves the frontend holding the file's *contents* rather than a path:
/// the bytes arrive as the raw IPC body, with the note and filename in headers.
#[tauri::command]
pub async fn attach_dropped_file(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    request: Request<'_>,
) -> crate::error::AryaResult<Attachment> {
    let note_id = header(&request, "x-note-id")?;
    let name = header(&request, "x-filename")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("dropped file arrived without its contents".into());
    };
    if bytes.is_empty() {
        return Err("dropped file is empty".into());
    }
    let root = attachments_root(&app)?;
    insert_attachment(
        &pool,
        &root,
        &note_id,
        &name,
        FileSource::Bytes(bytes.clone()),
    )
    .await
}

#[tauri::command]
pub async fn list_attachments(
    pool: State<'_, SqlitePool>,
    note_id: String,
) -> crate::error::AryaResult<Vec<Attachment>> {
    list_for_note(&pool, &note_id).await
}

#[tauri::command]
pub async fn remove_attachment(
    pool: State<'_, SqlitePool>,
    id: String,
) -> crate::error::AryaResult<()> {
    delete_attachment(&pool, &id).await
}

#[tauri::command]
pub async fn open_attachment(
    pool: State<'_, SqlitePool>,
    id: String,
) -> crate::error::AryaResult<()> {
    let path: String = sqlx::query_scalar("SELECT path FROM note_attachments WHERE id = ?1")
        .bind(&id)
        .fetch_optional(&*pool)
        .await
        .map_err(crate::error::AryaError::from_display)?
        .ok_or_else(|| crate::error::AryaError::message("attachment not found"))?;
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

        let att = insert_attachment(
            &pool,
            &root,
            &note.id,
            &src.to_string_lossy(),
            FileSource::Path(src.clone()),
        )
        .await
        .unwrap();
        assert_eq!(att.name, "data.csv", "the full path is reduced to its name");
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
        let att = insert_attachment(&pool, &root, &note.id, "a.txt", FileSource::Path(src))
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
