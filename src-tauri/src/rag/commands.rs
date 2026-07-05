//! RAG commands: reindex the workspace, semantic search, and the status the
//! settings UI shows.

use std::sync::{Mutex, OnceLock};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};

use super::embed::{Embedder, OllamaEmbedder};
use super::{blob_to_f32, chunk_text, cosine, f32_to_blob, SearchHit};

const OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// In-memory cache of parsed chunk embeddings so search doesn't re-load and
/// re-deserialize every blob from SQLite on each query. Invalidated on
/// reindex. Bounded by the workspace's chunk count.
struct CachedChunk {
    source_kind: String,
    source_id: String,
    title: String,
    content: String,
    embedding: Vec<f32>,
}

fn cache() -> &'static Mutex<Option<Vec<CachedChunk>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<CachedChunk>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn invalidate_cache() {
    *cache().lock().expect("rag cache lock") = None;
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RagStatus {
    pub embedder_available: bool,
    pub indexed_chunks: i64,
}

#[tauri::command]
pub async fn rag_status(pool: State<'_, SqlitePool>) -> Result<RagStatus, String> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rag_chunks")
        .fetch_one(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    let available =
        tauri::async_runtime::spawn_blocking(|| OllamaEmbedder::new(OLLAMA_URL).is_available())
            .await
            .unwrap_or(false);
    Ok(RagStatus {
        embedder_available: available,
        indexed_chunks: count,
    })
}

/// Rebuilds the index from all current notes, transcripts, dictation, and
/// agent sessions. Emits `rag:progress` as it goes.
#[tauri::command]
pub async fn rag_reindex(app: AppHandle, pool: State<'_, SqlitePool>) -> Result<i64, String> {
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || reindex_blocking(&app, &pool))
        .await
        .map_err(|e| e.to_string())?
}

/// Public wrapper for the debug-only dev hooks; compiled out of release, like
/// its only caller.
#[cfg(debug_assertions)]
pub fn reindex_blocking_public(app: &AppHandle, pool: &SqlitePool) -> Result<i64, String> {
    reindex_blocking(app, pool)
}

fn reindex_blocking(app: &AppHandle, pool: &SqlitePool) -> Result<i64, String> {
    let embedder = OllamaEmbedder::new(OLLAMA_URL);
    if !embedder.is_available() {
        return Err("local embedding model (Ollama) is not running".into());
    }

    invalidate_cache();
    let documents = tauri::async_runtime::block_on(collect_documents(pool))?;
    let _ = app.emit(
        "rag:progress",
        serde_json::json!({ "stage": "embedding", "total": documents.len() }),
    );

    // Embed everything into memory FIRST. Only once all embedding succeeds do we
    // touch the live index, and then in a single transaction — so a mid-run
    // Ollama failure leaves the existing index intact instead of emptying search.
    struct Staged {
        kind: String,
        id: String,
        title: String,
        content: String,
        blob: Vec<u8>,
    }
    let mut staged: Vec<Staged> = Vec::new();
    for doc in &documents {
        let chunks = chunk_text(&doc.content, 180, 40);
        if chunks.is_empty() {
            continue;
        }
        let embeddings = embedder.embed(&chunks)?;
        for (chunk, embedding) in chunks.iter().zip(&embeddings) {
            staged.push(Staged {
                kind: doc.kind.clone(),
                id: doc.id.clone(),
                title: doc.title.clone(),
                content: chunk.clone(),
                blob: f32_to_blob(embedding),
            });
        }
    }

    let total = staged.len() as i64;
    let model = embedder.model().to_string();
    tauri::async_runtime::block_on(async {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM rag_chunks")
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        for row in &staged {
            sqlx::query(
                "INSERT INTO rag_chunks
                     (id, source_kind, source_id, title, content, embedding, model, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&row.kind)
            .bind(&row.id)
            .bind(&row.title)
            .bind(&row.content)
            .bind(&row.blob)
            .bind(&model)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())
    })?;

    // Drop the stale cache; the next search repopulates from the new rows.
    invalidate_cache();
    let _ = app.emit(
        "rag:progress",
        serde_json::json!({ "stage": "done", "total": total }),
    );
    Ok(total)
}

/// Semantic search over the index. Returns the top `limit` chunks.
#[tauri::command]
pub async fn rag_search(
    pool: State<'_, SqlitePool>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<SearchHit>, String> {
    let limit = limit.unwrap_or(8).clamp(1, 50) as usize;
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || search_blocking(&pool, &query, limit))
        .await
        .map_err(|e| e.to_string())?
}

pub fn search_blocking(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, String> {
    let embedder = OllamaEmbedder::new(OLLAMA_URL);
    let query_embedding = embedder
        .embed(&[query.to_string()])?
        .into_iter()
        .next()
        .ok_or("no embedding for query")?;

    // Populate the embedding cache once (per reindex), then score in memory.
    ensure_cache_loaded(pool)?;
    let guard = cache().lock().expect("rag cache lock");
    let chunks = guard.as_ref().expect("cache loaded");

    let mut scored: Vec<SearchHit> = chunks
        .iter()
        .map(|c| SearchHit {
            source_kind: c.source_kind.clone(),
            source_id: c.source_id.clone(),
            title: c.title.clone(),
            content: c.content.clone(),
            score: cosine(&query_embedding, &c.embedding),
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    Ok(scored)
}

/// Loads chunk embeddings into the in-memory cache if not already present.
fn ensure_cache_loaded(pool: &SqlitePool) -> Result<(), String> {
    if cache().lock().expect("rag cache lock").is_some() {
        return Ok(());
    }
    let rows = tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, String, String, Vec<u8>)>(
            "SELECT source_kind, source_id, title, content, embedding FROM rag_chunks",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    })?;
    let chunks: Vec<CachedChunk> = rows
        .into_iter()
        .map(
            |(source_kind, source_id, title, content, blob)| CachedChunk {
                source_kind,
                source_id,
                title,
                content,
                embedding: blob_to_f32(&blob),
            },
        )
        .collect();
    *cache().lock().expect("rag cache lock") = Some(chunks);
    Ok(())
}

struct Document {
    kind: String,
    id: String,
    title: String,
    content: String,
}

async fn collect_documents(pool: &SqlitePool) -> Result<Vec<Document>, String> {
    let mut docs = Vec::new();

    // Notes: title + body + manual notes.
    let notes = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, title, body_md, manual_notes FROM notes WHERE processing_status != 'recording'",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for (id, title, body, manual) in notes {
        let content = format!("{title}\n{manual}\n{body}");
        if content.trim().len() > 3 {
            docs.push(Document {
                kind: "note".into(),
                id,
                title,
                content,
            });
        }
    }

    // Dictation history.
    let dictations = sqlx::query_as::<_, (String, String)>(
        "SELECT id, clean_text FROM dictation_history WHERE length(clean_text) > 3",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for (id, text) in dictations {
        docs.push(Document {
            kind: "dictation".into(),
            id,
            title: "Dictation".into(),
            content: text,
        });
    }

    // Agent sessions: concatenate message text.
    let sessions = sqlx::query_as::<_, (String, String)>("SELECT id, title FROM agent_sessions")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    for (id, title) in sessions {
        let texts = sqlx::query_scalar::<_, String>(
            "SELECT content_json FROM agent_messages WHERE session_id = ?1 ORDER BY created_at",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        let content: String = texts
            .iter()
            .filter_map(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .filter_map(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect::<Vec<_>>()
            .join("\n");
        if content.trim().len() > 3 {
            docs.push(Document {
                kind: "session".into(),
                id,
                title,
                content,
            });
        }
    }

    Ok(docs)
}
