//! RAG commands: reindex the workspace, semantic search, and the status the
//! settings UI shows.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, State};

use super::embed::{Embedder, OllamaEmbedder};
use super::{blob_to_f32, chunk_text, cosine, f32_to_blob, SearchHit};

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
pub async fn rag_status(pool: State<'_, SqlitePool>) -> crate::error::AryaResult<RagStatus> {
    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rag_chunks")
        .fetch_one(&*pool)
        .await
        .map_err(crate::error::AryaError::from_display)?;
    let available = tauri::async_runtime::spawn_blocking(|| {
        OllamaEmbedder::new(crate::provider::ollama_url()).is_available()
    })
    .await
    .unwrap_or(false);
    Ok(RagStatus {
        embedder_available: available,
        indexed_chunks: count,
    })
}

/// Removes the semantic-index rows for one deleted node (kind ∈ `note` |
/// `dictation` | `session`) and invalidates the in-memory search cache so the
/// next query reflects the deletion. Every delete path must call this so a
/// deleted note/dictation/chat is truly gone, not still searchable.
pub async fn delete_for_node(
    pool: &SqlitePool,
    kind: &str,
    id: &str,
) -> crate::error::AryaResult<()> {
    sqlx::query("DELETE FROM rag_chunks WHERE source_kind = ?1 AND source_id = ?2")
        .bind(kind)
        .bind(id)
        .execute(pool)
        .await
        .map_err(crate::error::AryaError::from_display)?;
    invalidate_cache();
    Ok(())
}

/// Removes every semantic-index row of a kind (delete-all flows).
pub async fn delete_for_kind(pool: &SqlitePool, kind: &str) -> crate::error::AryaResult<()> {
    sqlx::query("DELETE FROM rag_chunks WHERE source_kind = ?1")
        .bind(kind)
        .execute(pool)
        .await
        .map_err(crate::error::AryaError::from_display)?;
    invalidate_cache();
    Ok(())
}

/// Rebuilds the index from all current notes, transcripts, dictation, and
/// agent sessions. Emits `rag:progress` as it goes.
#[tauri::command]
pub async fn rag_reindex(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
) -> crate::error::AryaResult<i64> {
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || reindex_blocking(&app, &pool))
        .await
        .map_err(crate::error::AryaError::from_display)?
}

/// Public wrapper for the debug-only dev hooks; compiled out of release, like
/// its only caller.
#[cfg(debug_assertions)]
pub fn reindex_blocking_public(
    app: &AppHandle,
    pool: &SqlitePool,
) -> crate::error::AryaResult<i64> {
    reindex_blocking(app, pool)
}

fn reindex_blocking(app: &AppHandle, pool: &SqlitePool) -> crate::error::AryaResult<i64> {
    let embedder = OllamaEmbedder::new(crate::provider::ollama_url());
    if !embedder.is_available() {
        return Err("local embedding model (Ollama) is not running".into());
    }

    invalidate_cache();
    let documents = tauri::async_runtime::block_on(collect_documents(pool))?;
    let _ = app.emit(
        "rag:progress",
        serde_json::json!({ "stage": "embedding", "total": documents.len() }),
    );

    let total = replace_index(pool, &documents, &embedder)?;

    // Drop the stale cache; the next search repopulates from the new rows.
    invalidate_cache();
    let _ = app.emit(
        "rag:progress",
        serde_json::json!({ "stage": "done", "total": total }),
    );
    Ok(total)
}

struct Staged {
    kind: String,
    id: String,
    title: String,
    content: String,
    blob: Vec<u8>,
}

fn replace_index(
    pool: &SqlitePool,
    documents: &[Document],
    embedder: &dyn Embedder,
) -> crate::error::AryaResult<i64> {
    // Embed everything into memory FIRST. Only once all embedding succeeds do we
    // touch the live index, and then in a single transaction — so a mid-run
    // Ollama failure leaves the existing index intact instead of emptying search.
    let mut staged: Vec<Staged> = Vec::new();
    for doc in documents {
        let chunks = chunk_text(&doc.content, super::CHUNK_WORDS, super::CHUNK_OVERLAP);
        if chunks.is_empty() {
            continue;
        }
        let embeddings = embedder.embed(&chunks)?;
        if embeddings.len() != chunks.len() {
            return Err(crate::error::AryaError::message(format!(
                "embedding model returned {} rows for {} chunks",
                embeddings.len(),
                chunks.len()
            )));
        }
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
        let mut tx = pool
            .begin()
            .await
            .map_err(crate::error::AryaError::from_display)?;
        sqlx::query("DELETE FROM rag_chunks")
            .execute(&mut *tx)
            .await
            .map_err(crate::error::AryaError::from_display)?;
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
            .map_err(crate::error::AryaError::from_display)?;
        }
        tx.commit()
            .await
            .map_err(crate::error::AryaError::from_display)
    })?;
    Ok(total)
}

/// Semantic search over the index. Returns the top `limit` chunks.
#[tauri::command]
pub async fn rag_search(
    pool: State<'_, SqlitePool>,
    query: String,
    limit: Option<i64>,
) -> crate::error::AryaResult<Vec<SearchHit>> {
    let limit = limit.unwrap_or(8).clamp(1, 50) as usize;
    let pool = pool.inner().clone();
    tauri::async_runtime::spawn_blocking(move || search_blocking(&pool, &query, limit))
        .await
        .map_err(crate::error::AryaError::from_display)?
}

pub fn search_blocking(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
) -> crate::error::AryaResult<Vec<SearchHit>> {
    let embedder = OllamaEmbedder::new(crate::provider::ollama_url());
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
fn ensure_cache_loaded(pool: &SqlitePool) -> crate::error::AryaResult<()> {
    if cache().lock().expect("rag cache lock").is_some() {
        return Ok(());
    }
    let rows = tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, String, String, Vec<u8>)>(
            "SELECT source_kind, source_id, title, content, embedding FROM rag_chunks",
        )
        .fetch_all(pool)
        .await
        .map_err(crate::error::AryaError::from_display)
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

async fn collect_documents(pool: &SqlitePool) -> crate::error::AryaResult<Vec<Document>> {
    let mut docs = Vec::new();

    // Notes: title + body + manual notes.
    let notes = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, title, body_md, manual_notes FROM notes WHERE processing_status != 'recording'",
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::AryaError::from_display)?;
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
    .map_err(crate::error::AryaError::from_display)?;
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
        .map_err(crate::error::AryaError::from_display)?;
    for (id, title) in sessions {
        let texts = sqlx::query_scalar::<_, String>(
            "SELECT content_json FROM agent_messages WHERE session_id = ?1 ORDER BY created_at",
        )
        .bind(&id)
        .fetch_all(pool)
        .await
        .map_err(crate::error::AryaError::from_display)?;
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

    // Meeting transcripts: the raw turn text (distinct from the generated note
    // summary) so the full meeting content is semantically searchable, not just
    // the summary. Grouped per note; the note id is the source_id so deleting a
    // note also drops its transcript chunks.
    let mut by_note: HashMap<String, (String, Vec<String>)> = HashMap::new();
    let turns = sqlx::query_as::<_, (String, String, String)>(
        "SELECT t.note_id, n.title, t.text
         FROM transcript_turns t JOIN notes n ON n.id = t.note_id
         WHERE length(t.text) > 3
         ORDER BY t.note_id, t.turn_index",
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::AryaError::from_display)?;
    for (note_id, title, text) in turns {
        let entry = by_note
            .entry(note_id)
            .or_insert_with(|| (title, Vec::new()));
        entry.1.push(text);
    }
    for (note_id, (title, texts)) in by_note {
        docs.push(Document {
            kind: "transcript".into(),
            id: note_id,
            title,
            content: texts.join("\n"),
        });
    }

    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    struct FailingEmbedder;

    impl Embedder for FailingEmbedder {
        fn embed(&self, _texts: &[String]) -> crate::error::AryaResult<Vec<Vec<f32>>> {
            Err("embedding backend failed".into())
        }

        fn model(&self) -> &str {
            "test-failing"
        }
    }

    #[test]
    fn failed_reindex_preserves_existing_chunks() {
        let pool = tauri::async_runtime::block_on(test_pool());
        tauri::async_runtime::block_on(async {
            sqlx::query(
                "INSERT INTO rag_chunks
                     (id, source_kind, source_id, title, content, embedding, model, updated_at)
                 VALUES ('existing', 'note', 'old-note', 'Old', 'keep me', ?1, 'old-model',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(f32_to_blob(&[1.0, 0.0]))
            .execute(&pool)
            .await
            .unwrap();
            let note = crate::notes::insert_note(&pool, "New note").await.unwrap();
            sqlx::query("UPDATE notes SET body_md = 'new searchable text' WHERE id = ?1")
                .bind(&note.id)
                .execute(&pool)
                .await
                .unwrap();
        });
        let documents = tauri::async_runtime::block_on(collect_documents(&pool)).unwrap();

        let err = replace_index(&pool, &documents, &FailingEmbedder).unwrap_err();
        assert!(err.to_string().contains("embedding backend failed"));

        let rows: Vec<(String, String)> = tauri::async_runtime::block_on(async {
            sqlx::query_as("SELECT id, content FROM rag_chunks ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap()
        });
        assert_eq!(rows, vec![("existing".into(), "keep me".into())]);
    }

    #[test]
    fn delete_for_node_removes_its_chunks() {
        let pool = tauri::async_runtime::block_on(test_pool());
        tauri::async_runtime::block_on(async {
            let note = crate::notes::insert_note(&pool, "Delete me").await.unwrap();
            sqlx::query(
                "INSERT INTO rag_chunks
                     (id, source_kind, source_id, title, content, embedding, model, updated_at)
                 VALUES ('c1', 'note', ?1, 'Delete me', 'secret text', ?2, 'm',
                         strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            )
            .bind(&note.id)
            .bind(f32_to_blob(&[1.0, 0.0]))
            .execute(&pool)
            .await
            .unwrap();

            delete_for_node(&pool, "note", &note.id).await.unwrap();

            let remaining: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM rag_chunks WHERE source_kind = 'note' AND source_id = ?1",
            )
            .bind(&note.id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(remaining, 0, "deleting a note must delete its rag chunks");
        });
    }
}
