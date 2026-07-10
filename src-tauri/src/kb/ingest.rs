//! Document ingestion: extract → chunk → embed → store, run on a background
//! blocking thread so a large PDF never stalls the UI.
//!
//! Ingestion is atomic per document: chunks are embedded fully in memory and
//! written in a single transaction, so a mid-run embedder failure leaves the
//! document's prior index intact and flips it to `failed` rather than emptying
//! it. Progress is reported over the `kb:progress` event and the document's
//! `status` column, so the UI reflects state whether or not it's listening.

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

use super::{extract, ocr};
use crate::rag::embed::{Embedder, OllamaEmbedder};
use crate::vecmath::f32_to_blob;

/// Chunking mirrors the workspace RAG index (word windows with overlap) so the
/// two behave alike; page structure from the extractor is preserved per chunk.
const WORDS_PER_CHUNK: usize = 180;
const CHUNK_OVERLAP: usize = 40;
/// Embed in bounded batches so one huge document doesn't build a single massive
/// request; each batch gets the embedder's own 120s window.
const EMBED_BATCH: usize = 48;

fn emit_progress(app: &AppHandle, doc_id: &str, coll_id: &str, status: &str, error: Option<&str>) {
    let _ = app.emit(
        "kb:progress",
        serde_json::json!({
            "documentId": doc_id,
            "collectionId": coll_id,
            "status": status,
            "error": error,
        }),
    );
}

fn set_status(pool: &SqlitePool, document_id: &str, status: &str, error: Option<&str>) {
    let _ = tauri::async_runtime::block_on(async {
        sqlx::query(
            "UPDATE kb_documents SET status = ?2, error = ?3, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        )
        .bind(document_id)
        .bind(status)
        .bind(error)
        .execute(pool)
        .await
    });
}

/// Ingest one document end-to-end (blocking). Marks it `processing`, then
/// `ready` or `failed`, emitting `kb:progress` at each transition. Returns the
/// inner error too so callers/tests can assert on it.
pub fn ingest_blocking(
    app: &AppHandle,
    pool: &SqlitePool,
    document_id: &str,
) -> Result<(), String> {
    let row = tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT collection_id, ext, stored_path FROM kb_documents WHERE id = ?1",
        )
        .bind(document_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
    })?;
    let Some((collection_id, ext, stored_path)) = row else {
        return Err("document not found".into());
    };

    set_status(pool, document_id, "processing", None);
    emit_progress(app, document_id, &collection_id, "processing", None);

    let app_data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            let msg = e.to_string();
            set_status(pool, document_id, "failed", Some(&msg));
            emit_progress(app, document_id, &collection_id, "failed", Some(&msg));
            return Err(msg);
        }
    };

    let result = ingest_inner(
        pool,
        document_id,
        &collection_id,
        &ext,
        &stored_path,
        &app_data_dir,
    );
    match &result {
        Ok(()) => emit_progress(app, document_id, &collection_id, "ready", None),
        Err(e) => {
            set_status(pool, document_id, "failed", Some(e));
            emit_progress(app, document_id, &collection_id, "failed", Some(e));
        }
    }
    result
}

fn ingest_inner(
    pool: &SqlitePool,
    document_id: &str,
    collection_id: &str,
    ext: &str,
    stored_path: &str,
    app_data_dir: &std::path::Path,
) -> Result<(), String> {
    let embedder = OllamaEmbedder::new(crate::transform::ollama_url());
    if !embedder.is_available() {
        return Err("the local embedding model (Ollama) isn't running".into());
    }

    let bytes = std::fs::read(stored_path).map_err(|e| format!("could not read file: {e}"))?;
    let extracted = extract_with_ocr(ext, &bytes, app_data_dir)?;

    let chunks = chunk_pages(&extracted);
    if chunks.is_empty() {
        return Err("no readable text found in this document".into());
    }

    let texts: Vec<String> = chunks.iter().map(|(_, t)| t.clone()).collect();
    let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
    for batch in texts.chunks(EMBED_BATCH) {
        embeddings.extend(embedder.embed(batch)?);
    }
    if embeddings.len() != chunks.len() {
        return Err(format!(
            "embedding model returned {} rows for {} chunks",
            embeddings.len(),
            chunks.len()
        ));
    }

    let model = embedder.model().to_string();
    write_chunks(
        pool,
        document_id,
        collection_id,
        &chunks,
        &embeddings,
        &model,
        extracted.extractor,
        extracted.page_count,
    )
}

/// Recover text for a document: text formats via [`extract`], image files via
/// [`ocr`], and text-poor PDFs (scans) fall back to OCR of their page images.
fn extract_with_ocr(
    ext: &str,
    bytes: &[u8],
    app_data_dir: &std::path::Path,
) -> Result<extract::Extracted, String> {
    if ocr::is_image_ext(ext) {
        return ocr::ocr_image_file(app_data_dir, bytes);
    }
    if !extract::is_text_ext(ext) {
        return Err(format!("unsupported file type: .{ext}"));
    }
    let extracted = extract::extract(ext, bytes)?;
    // A PDF with almost no extractable text is almost certainly a scan; try OCR
    // of its embedded page images and keep whichever recovered more text.
    if ext == "pdf" {
        let threshold = (extracted.page_count.max(1) as usize) * 8;
        if extracted.text_len() < threshold {
            if let Ok(ocr_extracted) = ocr::ocr_pdf_images(app_data_dir, bytes) {
                if ocr_extracted.text_len() > extracted.text_len() {
                    return Ok(ocr_extracted);
                }
            }
        }
    }
    Ok(extracted)
}

/// Split every extracted page into overlapping word-window chunks, carrying the
/// page number so retrieval can cite it.
fn chunk_pages(extracted: &extract::Extracted) -> Vec<(Option<i64>, String)> {
    let mut out = Vec::new();
    for page in &extracted.pages {
        for content in crate::rag::chunk_text(&page.text, WORDS_PER_CHUNK, CHUNK_OVERLAP) {
            out.push((page.page, content));
        }
    }
    out
}

/// Replace a document's chunks (index + FTS mirror) and mark it ready, all in
/// one transaction.
#[allow(clippy::too_many_arguments)]
fn write_chunks(
    pool: &SqlitePool,
    document_id: &str,
    collection_id: &str,
    chunks: &[(Option<i64>, String)],
    embeddings: &[Vec<f32>],
    model: &str,
    extractor: &str,
    page_count: i64,
) -> Result<(), String> {
    let chunk_count = chunks.len() as i64;
    tauri::async_runtime::block_on(async {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM kb_chunks WHERE document_id = ?1")
            .bind(document_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM kb_chunks_fts WHERE document_id = ?1")
            .bind(document_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        for (ordinal, ((page, content), embedding)) in chunks.iter().zip(embeddings).enumerate() {
            let chunk_id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO kb_chunks \
                     (id, collection_id, document_id, ordinal, page, content, embedding, model) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(&chunk_id)
            .bind(collection_id)
            .bind(document_id)
            .bind(ordinal as i64)
            .bind(*page)
            .bind(content.as_str())
            .bind(f32_to_blob(embedding))
            .bind(model)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            sqlx::query(
                "INSERT INTO kb_chunks_fts (chunk_id, document_id, collection_id, content) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(&chunk_id)
            .bind(document_id)
            .bind(collection_id)
            .bind(content.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        sqlx::query(
            "UPDATE kb_documents SET status = 'ready', extractor = ?2, page_count = ?3, \
                 chunk_count = ?4, error = NULL, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        )
        .bind(document_id)
        .bind(extractor)
        .bind(page_count)
        .bind(chunk_count)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    })
}

/// At startup, re-queue documents left `pending`/`processing` by a crash or
/// quit mid-ingest so they finish (or fail cleanly) rather than hanging forever.
pub async fn recover_unfinished(app: AppHandle, pool: SqlitePool) {
    let ids: Vec<String> = match sqlx::query_scalar(
        "SELECT id FROM kb_documents WHERE status IN ('pending', 'processing')",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(v) => v,
        Err(_) => return,
    };
    for id in ids {
        let app = app.clone();
        let pool = pool.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _ = ingest_blocking(&app, &pool, &id);
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn chunk_pages_preserves_page_numbers() {
        let extracted = extract::Extracted {
            pages: vec![
                extract::Page {
                    page: Some(1),
                    text: "alpha beta gamma".into(),
                },
                extract::Page {
                    page: Some(2),
                    text: "delta epsilon".into(),
                },
            ],
            extractor: "text",
            page_count: 2,
        };
        let chunks = chunk_pages(&extracted);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].0, Some(1));
        assert_eq!(chunks[1].0, Some(2));
        assert!(chunks[1].1.contains("delta"));
    }

    // `write_chunks` blocks internally (mirrors the rag reindex pattern), so its
    // tests are plain `#[test]`s that drive async setup/asserts via block_on —
    // calling a blocking fn from inside a `#[tokio::test]` runtime would panic.
    #[test]
    fn write_chunks_populates_index_and_fts_and_marks_ready() {
        let pool = tauri::async_runtime::block_on(test_pool());
        tauri::async_runtime::block_on(async {
            sqlx::query("INSERT INTO kb_collections (id, name) VALUES ('c1', 'C')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO kb_documents (id, collection_id, filename, ext, status) \
                 VALUES ('d1', 'c1', 'a.txt', 'txt', 'processing')",
            )
            .execute(&pool)
            .await
            .unwrap();
        });

        let chunks = vec![
            (Some(1_i64), "quarterly revenue grew".to_string()),
            (None, "hiring plan for engineering".to_string()),
        ];
        let embeddings = vec![vec![0.1_f32, 0.2, 0.3], vec![0.4, 0.5, 0.6]];
        write_chunks(
            &pool,
            "d1",
            "c1",
            &chunks,
            &embeddings,
            "nomic-embed-text",
            "text",
            1,
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            let (status, chunk_count): (String, i64) =
                sqlx::query_as("SELECT status, chunk_count FROM kb_documents WHERE id = 'd1'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(status, "ready");
            assert_eq!(chunk_count, 2);

            let chunks_n: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM kb_chunks WHERE document_id='d1'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(chunks_n, 2);

            // FTS mirror is queryable.
            let fts_hit: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM kb_chunks_fts WHERE kb_chunks_fts MATCH 'revenue'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(fts_hit, 1);
        });
    }

    #[test]
    fn re_ingest_replaces_prior_chunks() {
        let pool = tauri::async_runtime::block_on(test_pool());
        tauri::async_runtime::block_on(async {
            sqlx::query("INSERT INTO kb_collections (id, name) VALUES ('c1', 'C')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO kb_documents (id, collection_id, filename, ext, status) \
                 VALUES ('d1', 'c1', 'a.txt', 'txt', 'processing')",
            )
            .execute(&pool)
            .await
            .unwrap();
        });
        let emb = vec![vec![1.0_f32, 0.0]];
        write_chunks(
            &pool,
            "d1",
            "c1",
            &[(None, "first".into())],
            &emb,
            "m",
            "text",
            1,
        )
        .unwrap();
        write_chunks(
            &pool,
            "d1",
            "c1",
            &[(None, "second".into())],
            &emb,
            "m",
            "text",
            1,
        )
        .unwrap();
        tauri::async_runtime::block_on(async {
            let contents: Vec<String> =
                sqlx::query_scalar("SELECT content FROM kb_chunks WHERE document_id='d1'")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(contents, vec!["second".to_string()], "old chunks replaced");
            let fts: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM kb_chunks_fts WHERE kb_chunks_fts MATCH 'first'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(fts, 0, "old FTS rows pruned on re-ingest");
        });
    }
}
