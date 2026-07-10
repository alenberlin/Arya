//! Per-collection hybrid retrieval: dense (vector cosine) + sparse (SQLite FTS5
//! BM25), fused with Reciprocal-Rank Fusion.
//!
//! The two legs catch different things — vectors find paraphrases and related
//! ideas, the keyword leg nails exact terms, names, and codes a dense model can
//! blur. RRF merges their rankings without needing the scores to be on the same
//! scale. Retrieval is scoped to one collection, and every hit carries its
//! document, page, and quote so the grounded chat can cite it.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::rag::embed::{Embedder, OllamaEmbedder};
use crate::vecmath::{blob_to_f32, cosine};

/// Reciprocal-Rank Fusion constant. 60 is the value from the original RRF paper
/// and what most hybrid-search implementations use; it damps the influence of
/// rank position so a strong hit in one leg isn't swamped by the other.
const RRF_K: f32 = 60.0;
/// Weight of the keyword (FTS/BM25) leg relative to the vector leg. Kept below
/// 1.0 so keyword matches *support* semantic relevance rather than dominate it:
/// table-of-contents / index pages repeat a query's terms and otherwise score
/// so high on BM25 that RRF buries the actual content page a query is about.
const FTS_WEIGHT: f32 = 0.3;
/// Always keep the top few pure-vector (semantic) hits, whatever fusion does, so
/// the single most relevant passage can never be crowded out by keyword-dense
/// noise. Belt-and-suspenders alongside the down-weighted keyword leg.
const GUARANTEE_VECTOR: usize = 3;

/// A retrieved chunk with its fused relevance and everything needed to cite it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbHit {
    pub chunk_id: String,
    pub document_id: String,
    pub filename: String,
    pub page: Option<i64>,
    pub content: String,
    pub score: f32,
}

/// A chunk loaded into memory for scoring.
struct ChunkRow {
    chunk_id: String,
    document_id: String,
    filename: String,
    page: Option<i64>,
    content: String,
    embedding: Vec<f32>,
}

/// Search one collection for the `limit` most relevant chunks. Blocking (called
/// from a worker thread).
pub fn search_blocking(
    pool: &SqlitePool,
    collection_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<KbHit>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let embedder = OllamaEmbedder::new(crate::transform::ollama_url());
    let query_vec = embedder
        .embed(&[query.to_string()])?
        .into_iter()
        .next()
        .ok_or("no embedding for query")?;

    let rows = tauri::async_runtime::block_on(load_chunks(pool, collection_id))?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    // Candidate pool per leg — generous enough that a hit strong in only one leg
    // still surfaces, bounded so fusion stays cheap.
    let pool_size = (limit * 5).max(20);
    let fts_ranked =
        tauri::async_runtime::block_on(fts_search(pool, collection_id, query, pool_size as i64))?;

    Ok(rank(&rows, &query_vec, &fts_ranked, pool_size, limit))
}

/// Rank `rows` by fusing a vector-cosine ordering with the FTS ordering
/// (`fts_ranked_ids`) via RRF. Pure so the fusion is unit-testable without a DB
/// or embedder.
fn rank(
    rows: &[ChunkRow],
    query_vec: &[f32],
    fts_ranked_ids: &[String],
    pool_size: usize,
    limit: usize,
) -> Vec<KbHit> {
    // Dense leg: cosine against every chunk, best first.
    let mut vector_scored: Vec<usize> = (0..rows.len()).collect();
    let cosines: Vec<f32> = rows
        .iter()
        .map(|r| cosine(query_vec, &r.embedding))
        .collect();
    vector_scored.sort_by(|&a, &b| {
        cosines[b]
            .partial_cmp(&cosines[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    vector_scored.truncate(pool_size);

    let index_by_id: HashMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.chunk_id.as_str(), i))
        .collect();

    let mut fused: HashMap<usize, f32> = HashMap::new();
    for (position, &idx) in vector_scored.iter().enumerate() {
        *fused.entry(idx).or_insert(0.0) += 1.0 / (RRF_K + position as f32 + 1.0);
    }
    for (position, id) in fts_ranked_ids.iter().enumerate() {
        if let Some(&idx) = index_by_id.get(id.as_str()) {
            *fused.entry(idx).or_insert(0.0) += FTS_WEIGHT / (RRF_K + position as f32 + 1.0);
        }
    }

    // Order candidates by fused score.
    let mut ranked_idx: Vec<usize> = fused.keys().copied().collect();
    ranked_idx.sort_by(|&a, &b| {
        fused[&b]
            .partial_cmp(&fused[&a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

    // Build the final set: the guaranteed top-vector hits first (semantic
    // relevance never dropped), then the rest in fused order, up to `limit`.
    let mut selected: Vec<usize> = Vec::with_capacity(limit);
    for &idx in vector_scored.iter().take(GUARANTEE_VECTOR) {
        if !selected.contains(&idx) {
            selected.push(idx);
        }
    }
    for &idx in &ranked_idx {
        if selected.len() >= limit {
            break;
        }
        if !selected.contains(&idx) {
            selected.push(idx);
        }
    }
    selected.truncate(limit);

    selected
        .into_iter()
        .map(|idx| {
            let r = &rows[idx];
            KbHit {
                chunk_id: r.chunk_id.clone(),
                document_id: r.document_id.clone(),
                filename: r.filename.clone(),
                page: r.page,
                content: r.content.clone(),
                score: *fused.get(&idx).unwrap_or(&0.0),
            }
        })
        .collect()
}

async fn load_chunks(pool: &SqlitePool, collection_id: &str) -> Result<Vec<ChunkRow>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, String, Vec<u8>)>(
        "SELECT c.id, c.document_id, d.filename, c.page, c.content, c.embedding \
         FROM kb_chunks c JOIN kb_documents d ON d.id = c.document_id \
         WHERE c.collection_id = ?1",
    )
    .bind(collection_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(
            |(chunk_id, document_id, filename, page, content, blob)| ChunkRow {
                chunk_id,
                document_id,
                filename,
                page,
                content,
                embedding: blob_to_f32(&blob),
            },
        )
        .collect())
}

/// Sparse leg: BM25-ranked chunk ids from the FTS mirror, scoped to the
/// collection. Returns ids best-match-first.
async fn fts_search(
    pool: &SqlitePool,
    collection_id: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<String>, String> {
    let Some(match_query) = fts_match_query(query) else {
        return Ok(Vec::new());
    };
    sqlx::query_scalar::<_, String>(
        "SELECT chunk_id FROM kb_chunks_fts \
         WHERE kb_chunks_fts MATCH ?1 AND collection_id = ?2 \
         ORDER BY bm25(kb_chunks_fts) LIMIT ?3",
    )
    .bind(match_query)
    .bind(collection_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

/// Build a safe FTS5 MATCH expression from free text: each alphanumeric token is
/// wrapped as a quoted phrase and OR-joined. Quoting neutralises FTS5 operators
/// so a query like `a AND (b` can't become a syntax error or injection.
fn fts_match_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" OR "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, embedding: Vec<f32>) -> ChunkRow {
        ChunkRow {
            chunk_id: id.into(),
            document_id: "d".into(),
            filename: "f.pdf".into(),
            page: None,
            content: format!("content {id}"),
            embedding,
        }
    }

    #[test]
    fn fts_match_query_quotes_tokens_and_skips_punctuation() {
        assert_eq!(
            fts_match_query("Quarterly revenue!"),
            Some("\"quarterly\" OR \"revenue\"".to_string())
        );
        // Single-letter tokens and pure punctuation are dropped.
        assert_eq!(fts_match_query("a b"), None);
        assert_eq!(fts_match_query("...,,,"), None);
    }

    #[test]
    fn rank_prefers_chunks_strong_in_both_legs() {
        // Query aligned with the x-axis. c1 is the closest vector match AND an
        // FTS hit; c2 is a decent vector match; c3 is only an FTS hit.
        let rows = vec![
            row("c1", vec![1.0, 0.0]),
            row("c2", vec![0.8, 0.6]),
            row("c3", vec![0.0, 1.0]),
        ];
        let query_vec = vec![1.0, 0.0];
        let fts = vec!["c1".to_string(), "c3".to_string()];
        let hits = rank(&rows, &query_vec, &fts, 10, 3);

        assert_eq!(hits.len(), 3);
        // c1: best vector rank (pos 0) + FTS rank (pos 0) → highest fused score.
        assert_eq!(hits[0].chunk_id, "c1");
        // c1 strictly beats the others (present in both legs).
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn fts_search_scopes_to_collection_and_matches() {
        let pool = tauri::async_runtime::block_on(crate::db::test_pool());
        tauri::async_runtime::block_on(async {
            for (cid, chunk, text) in [
                ("c1", "k1", "quarterly revenue growth"),
                ("c2", "k2", "quarterly revenue growth"),
            ] {
                sqlx::query(
                    "INSERT INTO kb_chunks_fts (chunk_id, document_id, collection_id, content) \
                     VALUES (?1, 'd', ?2, ?3)",
                )
                .bind(chunk)
                .bind(cid)
                .bind(text)
                .execute(&pool)
                .await
                .unwrap();
            }
            // Only c1's chunk is returned even though c2 has identical text.
            let hits = fts_search(&pool, "c1", "revenue", 10).await.unwrap();
            assert_eq!(hits, vec!["k1".to_string()]);
            // A non-matching term returns nothing.
            let none = fts_search(&pool, "c1", "unrelated", 10).await.unwrap();
            assert!(none.is_empty());
        });
    }

    #[test]
    fn strong_semantic_hit_survives_keyword_dense_noise() {
        // 'target' is the best semantic match but hits no keywords. The four
        // 'noise' chunks are only-mediocre semantic matches that all hit keywords
        // (like table-of-contents / index pages that repeat query terms).
        // Equal-weight RRF would bury 'target' beneath them; the down-weighted
        // keyword leg + top-vector guarantee keep it — this is the Satvic-recipe
        // regression.
        let rows = vec![
            row("target", vec![1.0, 0.0]),
            row("n1", vec![0.6, 0.6]),
            row("n2", vec![0.55, 0.6]),
            row("n3", vec![0.5, 0.6]),
            row("n4", vec![0.45, 0.6]),
        ];
        let fts = vec!["n1".into(), "n2".into(), "n3".into(), "n4".into()];
        let hits = rank(&rows, &[1.0, 0.0], &fts, 20, 3);
        assert!(
            hits.iter().any(|h| h.chunk_id == "target"),
            "the strongest semantic match must survive keyword-dense noise"
        );
        assert_eq!(hits[0].chunk_id, "target", "and it should lead");
    }

    #[test]
    fn rank_truncates_to_limit() {
        let rows = vec![
            row("c1", vec![1.0, 0.0]),
            row("c2", vec![0.9, 0.1]),
            row("c3", vec![0.5, 0.5]),
        ];
        let hits = rank(&rows, &[1.0, 0.0], &[], 10, 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, "c1");
    }

    /// End-to-end retrieval against a real local embedder (Ollama +
    /// nomic-embed-text). A paraphrased query with no shared keywords must still
    /// surface the semantically-matching chunk first — proving the dense leg,
    /// FTS leg, and RRF fusion work together on real vectors.
    /// Run with: `cargo test -p arya --lib -- --ignored real_hybrid`.
    #[ignore = "requires a local Ollama with nomic-embed-text"]
    #[test]
    fn real_hybrid_retrieval_finds_the_relevant_chunk() {
        use crate::rag::embed::{Embedder, OllamaEmbedder};
        use crate::vecmath::f32_to_blob;

        let pool = tauri::async_runtime::block_on(crate::db::test_pool());
        tauri::async_runtime::block_on(async {
            sqlx::query("INSERT INTO kb_collections (id, name) VALUES ('c1', 'C')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO kb_documents (id, collection_id, filename, status) \
                 VALUES ('d1', 'c1', 'facts.txt', 'ready')",
            )
            .execute(&pool)
            .await
            .unwrap();
        });

        let sentences = [
            "The quarterly revenue grew by twenty percent last year.",
            "Our headquarters is located in Berlin, Germany.",
            "The new engineer starts work next Monday morning.",
        ];
        let embedder = OllamaEmbedder::new(crate::transform::ollama_url());
        let embeddings = embedder
            .embed(&sentences.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .expect("embed via Ollama");

        tauri::async_runtime::block_on(async {
            for (i, (text, emb)) in sentences.iter().zip(&embeddings).enumerate() {
                let id = format!("k{i}");
                sqlx::query(
                    "INSERT INTO kb_chunks \
                         (id, collection_id, document_id, ordinal, content, embedding, model) \
                     VALUES (?1, 'c1', 'd1', ?2, ?3, ?4, 'nomic-embed-text')",
                )
                .bind(&id)
                .bind(i as i64)
                .bind(*text)
                .bind(f32_to_blob(emb))
                .execute(&pool)
                .await
                .unwrap();
                sqlx::query(
                    "INSERT INTO kb_chunks_fts (chunk_id, document_id, collection_id, content) \
                     VALUES (?1, 'd1', 'c1', ?2)",
                )
                .bind(&id)
                .bind(*text)
                .execute(&pool)
                .await
                .unwrap();
            }
        });

        // No keyword overlap with "revenue/grew/twenty" — pure semantic match.
        let hits = search_blocking(&pool, "c1", "how much did sales increase?", 3).unwrap();
        assert!(!hits.is_empty(), "expected retrieval hits");
        assert!(
            hits[0].content.contains("revenue"),
            "expected the revenue sentence ranked first, got: {}",
            hits[0].content
        );
    }
}
