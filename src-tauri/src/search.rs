//! Literal full-text search across every node kind (F14).
//!
//! Complements the semantic RAG search (`rag::rag_search`): this matches titles
//! and content by case-insensitive substring, so it works **offline** (no
//! embedder) and always returns exact hits. It spans notes (including meeting
//! transcripts) and dictations (including their on-demand translations). Mind
//! maps join when that surface lands (M12).

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

/// One literal search hit. `snippet` is a leading slice of the matched content.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TextHit {
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub snippet: String,
    pub created_at: String,
}

/// Escape a user query for a `LIKE ... ESCAPE '\'` clause.
fn like_pattern(query: &str) -> String {
    format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

pub async fn search_all_query(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<TextHit>, sqlx::Error> {
    let like = like_pattern(query);
    // Notes — title, body, manual notes, and transcript turns (a "meeting" is a
    // note with a transcript). created_at is selected so DISTINCT can ORDER BY it.
    let mut hits: Vec<TextHit> = sqlx::query_as::<_, TextHit>(
        "SELECT DISTINCT 'note' AS source_kind, n.id AS source_id, n.title AS title,
                substr(n.body_md, 1, 320) AS snippet, n.created_at AS created_at
         FROM notes n
         LEFT JOIN transcript_turns t ON t.note_id = n.id
         WHERE n.title LIKE ?1 ESCAPE '\\'
            OR n.body_md LIKE ?1 ESCAPE '\\'
            OR n.manual_notes LIKE ?1 ESCAPE '\\'
            OR t.text LIKE ?1 ESCAPE '\\'
         ORDER BY n.created_at DESC
         LIMIT ?2",
    )
    .bind(&like)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    // Dictations — cleaned/raw text and on-demand translations. Dictations have
    // no title, so a leading slice of the text stands in.
    let dictations: Vec<TextHit> = sqlx::query_as::<_, TextHit>(
        "SELECT DISTINCT 'dictation' AS source_kind, dh.id AS source_id,
                substr(dh.clean_text, 1, 60) AS title,
                substr(dh.clean_text, 1, 320) AS snippet, dh.created_at AS created_at
         FROM dictation_history dh
         LEFT JOIN dictation_translations dt ON dt.dictation_id = dh.id
         WHERE dh.clean_text LIKE ?1 ESCAPE '\\'
            OR dh.raw_text LIKE ?1 ESCAPE '\\'
            OR dt.text LIKE ?1 ESCAPE '\\'
         ORDER BY dh.created_at DESC
         LIMIT ?2",
    )
    .bind(&like)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    hits.extend(dictations);
    // Newest-first across both kinds.
    hits.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(hits)
}

/// Literal title+content search across all node kinds (F14). Works offline.
#[tauri::command]
pub async fn search_all(
    pool: State<'_, SqlitePool>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<TextHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    search_all_query(&pool, q, limit.unwrap_or(20))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn matches_notes_and_dictations_by_title_and_content() {
        let pool = test_pool().await;
        let note = crate::notes::insert_note(&pool, "Budget planning")
            .await
            .unwrap();
        crate::notes::update_note_fields(
            &pool,
            &note.id,
            None,
            Some("quarterly budget review"),
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO dictation_history (id, raw_text, clean_text, duration_ms, asr_ms, created_at)
             VALUES ('d1', 'raw', 'the budget is tight', 0, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let hits = search_all_query(&pool, "budget", 20).await.unwrap();
        assert!(hits
            .iter()
            .any(|h| h.source_kind == "note" && h.source_id == note.id));
        assert!(hits
            .iter()
            .any(|h| h.source_kind == "dictation" && h.source_id == "d1"));

        // No match, and an empty query returns nothing.
        assert!(search_all_query(&pool, "zzz-nope", 20)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn transcript_and_translation_text_is_searchable() {
        let pool = test_pool().await;
        let note = crate::notes::insert_note(&pool, "Standup").await.unwrap();
        sqlx::query(
            "INSERT INTO transcript_turns (id, note_id, source, turn_index, start_ms, end_ms, text)
             VALUES ('t1', ?1, 'microphone', 0, 0, 1000, 'we shipped the widget')",
        )
        .bind(&note.id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO dictation_history (id, raw_text, clean_text, duration_ms, asr_ms, created_at)
             VALUES ('d1', 'r', 'hello', 0, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO dictation_translations (id, dictation_id, lang, text, model, created_at)
             VALUES ('x1', 'd1', 'German', 'hallo Widget', 'm', strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let hits = search_all_query(&pool, "widget", 20).await.unwrap();
        assert!(
            hits.iter().any(|h| h.source_id == note.id),
            "transcript text matched"
        );
        assert!(
            hits.iter().any(|h| h.source_id == "d1"),
            "translation text matched"
        );
    }
}
