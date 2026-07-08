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
    /// Parent page, or `None` for a top-level note (F3 nesting).
    pub parent_note_id: Option<String>,
    pub created_at: String,
}

/// Full note payload for the editor.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    pub id: String,
    pub title: String,
    pub body_md: String,
    /// BlockNote block-JSON — the editor's source of truth. Empty for legacy
    /// notes authored before the block editor (they fall back to `body_md`).
    pub document_json: String,
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
    insert_note_under(pool, title, None).await
}

/// Insert a note, optionally as a child of `parent_id` (F3 nesting).
pub async fn insert_note_under(
    pool: &SqlitePool,
    title: &str,
    parent_id: Option<&str>,
) -> Result<Note, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query_as::<_, Note>(
        "INSERT INTO notes (id, title, parent_note_id, created_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, created_at",
    )
    .bind(&id)
    .bind(title)
    .bind(parent_id)
    .fetch_one(pool)
    .await
}

/// All descendant note ids of `id` (its subtree, excluding `id` itself).
async fn descendant_ids(pool: &SqlitePool, id: &str) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE subtree(id) AS (
             SELECT id FROM notes WHERE parent_note_id = ?1
             UNION ALL
             SELECT n.id FROM notes n JOIN subtree s ON n.parent_note_id = s.id
         )
         SELECT id FROM subtree",
    )
    .bind(id)
    .fetch_all(pool)
    .await
}

pub async fn fetch_notes(pool: &SqlitePool) -> Result<Vec<NoteSummary>, sqlx::Error> {
    // rowid is monotonic with insertion, so it's a stable newest-first tiebreak
    // when two notes share a created_at (unlike the random UUID id).
    sqlx::query_as::<_, NoteSummary>(
        "SELECT id, title, processing_status, processing_error, folder_id,
                parent_note_id, created_at
         FROM notes ORDER BY created_at DESC, rowid DESC",
    )
    .fetch_all(pool)
    .await
}

/// Case-insensitive keyword filter over note titles, bodies, manual notes, and
/// transcript text. Returns summaries newest-first. This is a fast substring
/// filter of the notes list — distinct from the semantic Search pillar.
pub async fn search_notes_query(
    pool: &SqlitePool,
    query: &str,
) -> Result<Vec<NoteSummary>, sqlx::Error> {
    let like = format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    sqlx::query_as::<_, NoteSummary>(
        "SELECT DISTINCT n.id, n.title, n.processing_status, n.processing_error,
                n.folder_id, n.parent_note_id, n.created_at
         FROM notes n
         LEFT JOIN transcript_turns t ON t.note_id = n.id
         WHERE n.title LIKE ?1 ESCAPE '\\'
            OR n.body_md LIKE ?1 ESCAPE '\\'
            OR n.manual_notes LIKE ?1 ESCAPE '\\'
            OR t.text LIKE ?1 ESCAPE '\\'
         ORDER BY n.created_at DESC",
    )
    .bind(like)
    .fetch_all(pool)
    .await
}

#[tauri::command]
pub async fn create_note(
    pool: State<'_, SqlitePool>,
    title: String,
    parent_id: Option<String>,
) -> Result<Note, String> {
    insert_note_under(&pool, &title, parent_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Re-parent a note (or move it to top level with `parentId = null`). Guards
/// against cycles: a page cannot become its own parent or a child of one of its
/// own descendants.
#[tauri::command]
pub async fn set_note_parent(
    pool: State<'_, SqlitePool>,
    note_id: String,
    parent_id: Option<String>,
) -> Result<(), String> {
    if let Some(pid) = &parent_id {
        if pid == &note_id {
            return Err("a page cannot be its own parent".into());
        }
        let descendants = descendant_ids(&pool, &note_id)
            .await
            .map_err(|e| e.to_string())?;
        if descendants.iter().any(|d| d == pid) {
            return Err("cannot move a page into its own subtree".into());
        }
    }
    sqlx::query("UPDATE notes SET parent_note_id = ?2 WHERE id = ?1")
        .bind(&note_id)
        .bind(&parent_id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_notes(pool: State<'_, SqlitePool>) -> Result<Vec<NoteSummary>, String> {
    fetch_notes(&pool).await.map_err(|e| e.to_string())
}

/// Filters the notes list by a keyword found in the title or content. An empty
/// query returns the full list.
#[tauri::command]
pub async fn search_notes(
    pool: State<'_, SqlitePool>,
    query: String,
) -> Result<Vec<NoteSummary>, String> {
    let q = query.trim();
    if q.is_empty() {
        return fetch_notes(&pool).await.map_err(|e| e.to_string());
    }
    search_notes_query(&pool, q)
        .await
        .map_err(|e| e.to_string())
}

/// Fetch the full note payload for the editor.
pub async fn fetch_note_detail(pool: &SqlitePool, id: &str) -> Result<NoteDetail, sqlx::Error> {
    sqlx::query_as::<_, NoteDetail>(
        "SELECT id, title, body_md, document_json, manual_notes, processing_status,
                processing_error, folder_id, calendar_context, created_at
         FROM notes WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

#[tauri::command]
pub async fn get_note(pool: State<'_, SqlitePool>, id: String) -> Result<NoteDetail, String> {
    fetch_note_detail(&pool, &id)
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

/// Persist edited note fields. Any `None` field is left unchanged (COALESCE), so
/// a caller can patch one field without clobbering the rest. `document_json` is
/// the block-editor source of truth; `body_md` is its plaintext/markdown
/// projection (kept in sync by the client so search + RAG stay accurate).
pub async fn update_note_fields(
    pool: &SqlitePool,
    id: &str,
    title: Option<&str>,
    body_md: Option<&str>,
    manual_notes: Option<&str>,
    document_json: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notes SET
             title = COALESCE(?2, title),
             body_md = COALESCE(?3, body_md),
             manual_notes = COALESCE(?4, manual_notes),
             document_json = COALESCE(?5, document_json),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(id)
    .bind(title)
    .bind(body_md)
    .bind(manual_notes)
    .bind(document_json)
    .execute(pool)
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn update_note(
    pool: State<'_, SqlitePool>,
    id: String,
    title: Option<String>,
    body_md: Option<String>,
    manual_notes: Option<String>,
    document_json: Option<String>,
) -> Result<(), String> {
    update_note_fields(
        &pool,
        &id,
        title.as_deref(),
        body_md.as_deref(),
        manual_notes.as_deref(),
        document_json.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Deletes a note and its recordings/attachments from disk (rows cascade).
#[tauri::command]
pub async fn delete_note(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    delete_note_inner(&pool, &id).await
}

/// Testable core of [`delete_note`]. Deletes the row FIRST (the authoritative
/// operation, children cascading), then best-effort removes the files — so a
/// failed delete leaves the note and its files intact rather than orphaning
/// files under a still-live row.
pub async fn delete_note_inner(pool: &SqlitePool, id: &str) -> Result<(), String> {
    // Collect every file path BEFORE deleting: the rows that hold the paths
    // cascade away with the note, so afterwards they'd be unqueryable.
    // The whole subtree — the note plus every descendant page — collected before
    // the delete so their files and graph edges can be cleaned even though the
    // rows cascade away.
    let subtree_ids = sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE subtree(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM notes n JOIN subtree s ON n.parent_note_id = s.id
         )
         SELECT id FROM subtree",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let audio_paths = sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE subtree(id) AS (
             SELECT ?1 UNION ALL
             SELECT n.id FROM notes n JOIN subtree s ON n.parent_note_id = s.id
         )
         SELECT a.path FROM audio_artifacts a
         JOIN recording_sessions s ON s.id = a.session_id
         WHERE s.note_id IN (SELECT id FROM subtree)",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let attachment_paths = sqlx::query_scalar::<_, String>(
        "WITH RECURSIVE subtree(id) AS (
             SELECT ?1 UNION ALL
             SELECT n.id FROM notes n JOIN subtree s ON n.parent_note_id = s.id
         )
         SELECT path FROM note_attachments WHERE note_id IN (SELECT id FROM subtree)",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    // Deleting the root cascades the descendant pages (parent_note_id ON DELETE
    // CASCADE), and each page's sessions/attachments/turns cascade in turn.
    sqlx::query("DELETE FROM notes WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    // Drop graph edges for every page that was in the subtree (best-effort: the
    // authoritative rows are already gone; a leftover edge resolves as deleted).
    for node_id in &subtree_ids {
        let _ = crate::links::delete_for_node(pool, "note", node_id).await;
    }

    remove_files(audio_paths.into_iter().chain(attachment_paths));
    Ok(())
}

/// Deletes every note, and its recordings/attachments from disk. Backs the
/// "Delete all" action in the notes list.
#[tauri::command]
pub async fn delete_all_notes(pool: State<'_, SqlitePool>) -> Result<(), String> {
    delete_all_notes_inner(&pool).await
}

/// Testable core of [`delete_all_notes`]. Every audio artifact and attachment
/// belongs to a note (their rows reference `notes` with `ON DELETE CASCADE`),
/// so deleting all notes removes them all. Mirrors [`delete_note_inner`]:
/// collect the file paths first, delete the rows, then unlink the files — a
/// failed delete leaves everything intact rather than orphaning files.
pub async fn delete_all_notes_inner(pool: &SqlitePool) -> Result<(), String> {
    let audio_paths = sqlx::query_scalar::<_, String>("SELECT path FROM audio_artifacts")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let attachment_paths = sqlx::query_scalar::<_, String>("SELECT path FROM note_attachments")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM notes")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    // Every note is gone, so drop every edge touching a note (best-effort).
    let _ = crate::links::delete_for_kind(pool, "note").await;

    remove_files(audio_paths.into_iter().chain(attachment_paths));
    Ok(())
}

/// Best-effort removal of files and their now-empty parent directories. Called
/// after the owning rows are deleted, so failures are ignored: the
/// authoritative delete already succeeded and a leftover file is harmless.
fn remove_files(paths: impl IntoIterator<Item = String>) {
    for path in paths {
        let path = std::path::PathBuf::from(path);
        let _ = std::fs::remove_file(&path);
        if let Some(dir) = path.parent() {
            // Remove the per-session / attachment directory when empty.
            let _ = std::fs::remove_dir(dir);
        }
    }
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
    async fn document_json_defaults_empty_and_round_trips() {
        let pool = test_pool().await;
        let n = insert_note(&pool, "doc").await.unwrap();
        // Legacy default: no rich document yet, empty body.
        let d = fetch_note_detail(&pool, &n.id).await.unwrap();
        assert_eq!(d.document_json, "");
        assert_eq!(d.body_md, "");

        // Save block-JSON + its markdown projection; a None field (manual_notes)
        // is left untouched.
        update_note_fields(
            &pool,
            &n.id,
            None,
            Some("# Title\n\nbody"),
            None,
            Some(r#"[{"type":"heading"}]"#),
        )
        .await
        .unwrap();
        let d2 = fetch_note_detail(&pool, &n.id).await.unwrap();
        assert_eq!(d2.document_json, r#"[{"type":"heading"}]"#);
        assert_eq!(d2.body_md, "# Title\n\nbody");
        assert_eq!(d2.manual_notes, "");
    }

    #[tokio::test]
    async fn child_note_nests_under_its_parent() {
        let pool = test_pool().await;
        let parent = insert_note(&pool, "Parent").await.unwrap();
        let child = insert_note_under(&pool, "Child", Some(&parent.id))
            .await
            .unwrap();
        let notes = fetch_notes(&pool).await.unwrap();
        let child_row = notes.iter().find(|n| n.id == child.id).unwrap();
        assert_eq!(
            child_row.parent_note_id.as_deref(),
            Some(parent.id.as_str())
        );
        let parent_row = notes.iter().find(|n| n.id == parent.id).unwrap();
        assert_eq!(parent_row.parent_note_id, None);
    }

    #[tokio::test]
    async fn descendant_ids_walks_the_whole_subtree() {
        let pool = test_pool().await;
        let a = insert_note(&pool, "A").await.unwrap();
        let b = insert_note_under(&pool, "B", Some(&a.id)).await.unwrap();
        let c = insert_note_under(&pool, "C", Some(&b.id)).await.unwrap();
        let d = insert_note(&pool, "D").await.unwrap(); // unrelated top-level
        let mut desc = descendant_ids(&pool, &a.id).await.unwrap();
        desc.sort();
        let mut expected = vec![b.id.clone(), c.id.clone()];
        expected.sort();
        assert_eq!(desc, expected);
        assert!(!desc.contains(&d.id));
    }

    #[tokio::test]
    async fn deleting_a_parent_removes_its_subtree_and_files() {
        // Verifies the parent_note_id ON DELETE CASCADE *and* that the subtree's
        // files are collected + removed even for a deep descendant.
        let pool = test_pool().await;
        let dir = std::env::temp_dir().join(format!("arya-nest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let parent = insert_note(&pool, "Parent").await.unwrap();
        let child = insert_note_under(&pool, "Child", Some(&parent.id))
            .await
            .unwrap();
        let grandchild = insert_note_under(&pool, "Grandchild", Some(&child.id))
            .await
            .unwrap();

        let attach = dir.join("deep.pdf");
        std::fs::write(&attach, b"pdf").unwrap();
        sqlx::query(
            "INSERT INTO note_attachments (id, note_id, name, path, size_bytes, created_at)
             VALUES ('a1', ?1, 'deep.pdf', ?2, 3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .bind(&grandchild.id)
        .bind(attach.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();

        delete_note_inner(&pool, &parent.id)
            .await
            .expect("delete subtree");

        assert!(
            fetch_notes(&pool).await.unwrap().is_empty(),
            "the whole subtree cascaded away"
        );
        assert!(!attach.exists(), "the grandchild's file was removed");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[tokio::test]
    async fn search_matches_title_body_and_transcript_newest_first() {
        let pool = test_pool().await;
        let a = insert_note(&pool, "Budget planning").await.unwrap(); // title match
        let b = insert_note(&pool, "Random note").await.unwrap(); // body match
        let c = insert_note(&pool, "Team meeting").await.unwrap(); // transcript match
        let d = insert_note(&pool, "Unrelated").await.unwrap(); // no match
        for (id, ts) in [
            (&a.id, "2026-01-01T00:00:01.000Z"),
            (&b.id, "2026-01-01T00:00:02.000Z"),
            (&c.id, "2026-01-01T00:00:03.000Z"),
            (&d.id, "2026-01-01T00:00:04.000Z"),
        ] {
            sqlx::query("UPDATE notes SET created_at = ?2 WHERE id = ?1")
                .bind(id)
                .bind(ts)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE notes SET body_md = 'quarterly budget review' WHERE id = ?1")
            .bind(&b.id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcript_turns (id, note_id, source, turn_index, start_ms, end_ms, text)
             VALUES ('t1', ?1, 'microphone', 0, 0, 1000, 'we discussed the budget')",
        )
        .bind(&c.id)
        .execute(&pool)
        .await
        .unwrap();

        let hits = search_notes_query(&pool, "budget").await.unwrap();
        let ids: Vec<String> = hits.iter().map(|n| n.id.clone()).collect();
        assert_eq!(hits.len(), 3, "title, body, and transcript matches");
        // Newest-first: c (00:03) → b (00:02) → a (00:01); d excluded.
        assert_eq!(ids, vec![c.id.clone(), b.id.clone(), a.id.clone()]);

        assert!(search_notes_query(&pool, "zzz-nope")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn delete_all_notes_clears_rows_cascades_and_removes_files() {
        let pool = test_pool().await;
        let dir = std::env::temp_dir().join(format!("arya-del-all-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // Note A: an attachment on disk. Note B: a recording session with an
        // audio artifact on disk. Both files must be gone after delete-all.
        let a = insert_note(&pool, "A").await.unwrap();
        let b = insert_note(&pool, "B").await.unwrap();
        let attach = dir.join("report.pdf");
        let audio = dir.join("mic.wav");
        std::fs::write(&attach, b"pdf").unwrap();
        std::fs::write(&audio, b"wav").unwrap();

        sqlx::query(
            "INSERT INTO note_attachments (id, note_id, name, path, size_bytes, created_at)
             VALUES ('att1', ?1, 'report.pdf', ?2, 3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .bind(&a.id)
        .bind(attach.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO recording_sessions
                (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
             VALUES ('sess1', ?1, 'finished', 'microphone-only', 16000, 1,
                     strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .bind(&b.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO audio_artifacts (id, session_id, source, path, status, size_bytes, created_at)
             VALUES ('art1', 'sess1', 'microphone', ?1, 'final', 3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .bind(audio.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();

        delete_all_notes_inner(&pool).await.expect("delete all");

        assert!(fetch_notes(&pool).await.unwrap().is_empty(), "notes gone");
        // Children cascade away with their notes.
        let attach_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM note_attachments")
            .fetch_one(&pool)
            .await
            .unwrap();
        let artifact_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audio_artifacts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((attach_rows, artifact_rows), (0, 0), "children cascaded");
        // And their files are unlinked from disk.
        assert!(!attach.exists(), "attachment file removed");
        assert!(!audio.exists(), "audio file removed");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
