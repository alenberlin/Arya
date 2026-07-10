//! Knowledge Base commands: collection CRUD and embedder status. Document
//! ingestion, retrieval, and grounded chat live in sibling modules and are added
//! to the invoke handler as they land.

use std::path::PathBuf;

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

use super::Collection;
use crate::rag::embed::OllamaEmbedder;

/// Selects a [`Collection`] with its derived document counts. The counts are
/// subqueries rather than a GROUP BY join so a collection with zero documents
/// still returns a row.
const COLLECTION_SELECT: &str = "SELECT c.id, c.name, c.description, c.created_at, c.updated_at, \
     (SELECT COUNT(*) FROM kb_documents d WHERE d.collection_id = c.id) AS document_count, \
     (SELECT COUNT(*) FROM kb_documents d WHERE d.collection_id = c.id AND d.status = 'ready') AS ready_count \
     FROM kb_collections c";

/// Root directory holding every collection's copied raw files:
/// `<app_data>/knowledge/<collection_id>/<uuid>-<name>`.
pub(crate) fn kb_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("knowledge"))
}

/// Re-reads one collection (with counts) after a mutation so the UI always
/// receives the authoritative row.
async fn fetch_collection(pool: &SqlitePool, id: &str) -> Result<Collection, String> {
    sqlx::query_as::<_, Collection>(&format!("{COLLECTION_SELECT} WHERE c.id = ?1"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_list_collections(pool: State<'_, SqlitePool>) -> Result<Vec<Collection>, String> {
    sqlx::query_as::<_, Collection>(&format!("{COLLECTION_SELECT} ORDER BY c.updated_at DESC"))
        .fetch_all(&*pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_create_collection(
    pool: State<'_, SqlitePool>,
    name: String,
    description: Option<String>,
) -> Result<Collection, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a collection name is required".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO kb_collections (id, name, description) VALUES (?1, ?2, ?3)")
        .bind(&id)
        .bind(name)
        .bind(description.unwrap_or_default().trim())
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    fetch_collection(&pool, &id).await
}

#[tauri::command]
pub async fn kb_rename_collection(
    pool: State<'_, SqlitePool>,
    id: String,
    name: String,
    description: Option<String>,
) -> Result<Collection, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a collection name is required".into());
    }
    // COALESCE leaves the description untouched when the caller omits it.
    sqlx::query(
        "UPDATE kb_collections \
         SET name = ?2, description = COALESCE(?3, description), \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE id = ?1",
    )
    .bind(&id)
    .bind(name)
    .bind(description.map(|d| d.trim().to_string()))
    .execute(&*pool)
    .await
    .map_err(|e| e.to_string())?;
    fetch_collection(&pool, &id).await
}

/// Deletes a collection's index rows and returns the stored file paths to clean
/// up. Split out from the command so it is testable without an `AppHandle`. The
/// FTS mirror has no foreign key, so its rows are pruned explicitly before the
/// collection row's cascade removes `kb_documents` + `kb_chunks`.
async fn delete_collection_inner(pool: &SqlitePool, id: &str) -> Result<Vec<String>, String> {
    let paths: Vec<String> = sqlx::query_scalar(
        "SELECT stored_path FROM kb_documents WHERE collection_id = ?1 AND stored_path <> ''",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM kb_chunks_fts WHERE collection_id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM kb_collections WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(paths)
}

#[tauri::command]
pub async fn kb_delete_collection(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    let paths = delete_collection_inner(&pool, &id).await?;
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
    if let Ok(root) = kb_root(&app) {
        let _ = std::fs::remove_dir_all(root.join(&id));
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbStatus {
    /// Whether the local Ollama embedding model is reachable. Ingestion and
    /// search need it; the UI greys out actions and explains when it's false.
    pub embedder_available: bool,
}

#[tauri::command]
pub async fn kb_status() -> Result<KbStatus, String> {
    let url = crate::transform::ollama_url();
    let embedder_available =
        tauri::async_runtime::spawn_blocking(move || OllamaEmbedder::new(url).is_available())
            .await
            .unwrap_or(false);
    Ok(KbStatus { embedder_available })
}

/// A document in a collection as shown in the detail pane.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct KbDocument {
    pub id: String,
    pub collection_id: String,
    pub filename: String,
    pub ext: String,
    pub byte_size: i64,
    pub status: String,
    pub extractor: String,
    pub page_count: i64,
    pub chunk_count: i64,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const DOCUMENT_SELECT: &str = "SELECT id, collection_id, filename, ext, byte_size, status, \
     extractor, page_count, chunk_count, error, created_at, updated_at FROM kb_documents";

#[tauri::command]
pub async fn kb_list_documents(
    pool: State<'_, SqlitePool>,
    collection_id: String,
) -> Result<Vec<KbDocument>, String> {
    sqlx::query_as::<_, KbDocument>(&format!(
        "{DOCUMENT_SELECT} WHERE collection_id = ?1 ORDER BY created_at DESC"
    ))
    .bind(&collection_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

/// Copy each picked file into the collection's workspace directory, register it
/// as `pending`, and kick off background ingestion. Returns the new rows
/// immediately so the UI shows them while they process.
#[tauri::command]
pub async fn kb_add_documents(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    collection_id: String,
    paths: Vec<String>,
) -> Result<Vec<KbDocument>, String> {
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM kb_collections WHERE id = ?1")
        .bind(&collection_id)
        .fetch_optional(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    if exists.is_none() {
        return Err("collection not found".into());
    }
    let dir = kb_root(&app)?.join(&collection_id);

    let mut created = Vec::new();
    for path in paths {
        let src = std::path::PathBuf::from(&path);
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let id = uuid::Uuid::new_v4().to_string();
        let dest = dir.join(format!("{id}-{name}"));

        let (byte_size, stored_path) = {
            let dir = dir.clone();
            let dest = dest.clone();
            tauri::async_runtime::spawn_blocking(move || -> Result<(i64, String), String> {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                std::fs::copy(&src, &dest).map_err(|e| format!("copy failed: {e}"))?;
                let size = std::fs::metadata(&dest)
                    .map(|m| m.len() as i64)
                    .unwrap_or(0);
                Ok((size, dest.to_string_lossy().to_string()))
            })
            .await
            .map_err(|e| e.to_string())??
        };

        sqlx::query(
            "INSERT INTO kb_documents (id, collection_id, filename, ext, byte_size, status, stored_path) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
        )
        .bind(&id)
        .bind(&collection_id)
        .bind(&name)
        .bind(&ext)
        .bind(byte_size)
        .bind(&stored_path)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;

        let doc = sqlx::query_as::<_, KbDocument>(&format!("{DOCUMENT_SELECT} WHERE id = ?1"))
            .bind(&id)
            .fetch_one(&*pool)
            .await
            .map_err(|e| e.to_string())?;
        created.push(doc);
    }

    // Ingest sequentially on a background task so multiple large files don't
    // hammer the embedder concurrently. The command returns without waiting.
    let ids: Vec<String> = created.iter().map(|d| d.id.clone()).collect();
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn(async move {
        for id in ids {
            let app = app.clone();
            let pool = pool.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let _ = super::ingest::ingest_blocking(&app, &pool, &id);
            })
            .await;
        }
    });

    Ok(created)
}

#[tauri::command]
pub async fn kb_delete_document(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    let path: Option<String> =
        sqlx::query_scalar("SELECT stored_path FROM kb_documents WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&*pool)
            .await
            .map_err(|e| e.to_string())?;
    // kb_chunks cascade on the document row; the FTS mirror has no FK.
    sqlx::query("DELETE FROM kb_chunks_fts WHERE document_id = ?1")
        .bind(&id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM kb_documents WHERE id = ?1")
        .bind(&id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(p) = path {
        if !p.is_empty() {
            let _ = std::fs::remove_file(&p);
        }
    }
    Ok(())
}

/// Hybrid search over one collection (vector + keyword, RRF-fused). Returns the
/// top cited chunks; powers the grounded chat.
#[tauri::command]
pub async fn kb_search(
    pool: State<'_, SqlitePool>,
    collection_id: String,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<super::retrieve::KbHit>, String> {
    let limit = limit.unwrap_or(8).clamp(1, 50) as usize;
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::retrieve::search_blocking(&pool, &collection_id, &query, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Re-run extraction + embedding for one document (e.g. after starting Ollama).
#[tauri::command]
pub async fn kb_reindex_document(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    id: String,
) -> Result<(), String> {
    sqlx::query("UPDATE kb_documents SET status = 'pending', error = NULL WHERE id = ?1")
        .bind(&id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _ = super::ingest::ingest_blocking(&app, &pool, &id);
        })
        .await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::vecmath::f32_to_blob;

    async fn insert_collection(pool: &SqlitePool, name: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO kb_collections (id, name, description) VALUES (?1, ?2, '')")
            .bind(&id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn fetch_collection_reports_counts() {
        let pool = test_pool().await;
        let cid = insert_collection(&pool, "Research").await;
        // One ready doc, one still processing → document_count 2, ready_count 1.
        for status in ["ready", "processing"] {
            sqlx::query(
                "INSERT INTO kb_documents (id, collection_id, filename, status) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&cid)
            .bind(format!("{status}.pdf"))
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        }
        let c = fetch_collection(&pool, &cid).await.unwrap();
        assert_eq!(c.name, "Research");
        assert_eq!(c.document_count, 2);
        assert_eq!(c.ready_count, 1);
    }

    #[tokio::test]
    async fn delete_collection_cascades_documents_chunks_and_fts() {
        let pool = test_pool().await;
        let cid = insert_collection(&pool, "Temp").await;
        let did = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO kb_documents (id, collection_id, filename, status) \
             VALUES (?1, ?2, 'a.txt', 'ready')",
        )
        .bind(&did)
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();
        let chunk_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO kb_chunks (id, collection_id, document_id, ordinal, content, embedding, model) \
             VALUES (?1, ?2, ?3, 0, 'hello world', ?4, 'nomic-embed-text')",
        )
        .bind(&chunk_id)
        .bind(&cid)
        .bind(&did)
        .bind(f32_to_blob(&[0.1, 0.2, 0.3]))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO kb_chunks_fts (chunk_id, document_id, collection_id, content) \
             VALUES (?1, ?2, ?3, 'hello world')",
        )
        .bind(&chunk_id)
        .bind(&did)
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();

        delete_collection_inner(&pool, &cid).await.unwrap();

        let docs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_documents")
            .fetch_one(&pool)
            .await
            .unwrap();
        let chunks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_chunks")
            .fetch_one(&pool)
            .await
            .unwrap();
        let fts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_chunks_fts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            (docs, chunks, fts),
            (0, 0, 0),
            "cascade + FTS prune clears all"
        );
    }
}
