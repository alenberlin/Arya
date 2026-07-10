//! Grounded, per-collection chat.
//!
//! A question retrieves the most relevant chunks from the collection (hybrid
//! search), and the local model answers **only** from those passages, citing
//! each claim with a `[D#]` tag that maps to a source. Nothing leaves the Mac.
//! Sessions and messages (with their citations) are persisted so a conversation
//! survives restarts.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

use super::retrieve::KbHit;

/// The guardrail that keeps answers grounded — while still producing a real,
/// readable answer rather than a citation-laden meta-summary.
const SYSTEM_PROMPT: &str = "You are Arya's knowledge-base assistant. Using ONLY the provided \
    passages from the user's own documents, write a clear, comprehensive, well-organized answer to \
    their question.\n\
    - Answer directly and substantively: explain and synthesize what the passages actually say. Do \
    NOT merely state that the documents \"mention\" or \"contain\" something — give the real content.\n\
    - Present lists as lists; use short paragraphs and bullet points where they aid clarity.\n\
    - Ground every statement in the passages; never add outside knowledge or invent details. If the \
    passages only partly answer the question, give what they support and briefly note what's missing.\n\
    - Support the answer with [D#] citations placed at the END of the sentence or bullet they back \
    up — citations support the answer, they don't replace it. The answer must read naturally on its own.\n\
    - Only if the passages truly don't address the question, say so plainly.";

/// How many chunks feed one answer. Generous so the answer can be comprehensive
/// rather than starved of context — and so a recipe/section split across several
/// chunks arrives whole rather than half-retrieved.
const RETRIEVE_K: usize = 10;
/// Longest a cited quote is shown/stored.
const QUOTE_CHARS: usize = 280;

/// A source backing an assistant answer, keyed by its inline `[D#]` tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub key: String,
    pub document_id: String,
    pub filename: String,
    pub page: Option<i64>,
    pub quote: String,
}

/// A chat session scoped to one collection.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct KbSession {
    pub id: String,
    pub collection_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A persisted message with its parsed citations.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub citations: Vec<Citation>,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct KbMessageRow {
    id: String,
    session_id: String,
    role: String,
    content: String,
    citations_json: String,
    created_at: String,
}

impl From<KbMessageRow> for KbMessage {
    fn from(r: KbMessageRow) -> Self {
        KbMessage {
            id: r.id,
            session_id: r.session_id,
            role: r.role,
            content: r.content,
            citations: serde_json::from_str(&r.citations_json).unwrap_or_default(),
            created_at: r.created_at,
        }
    }
}

/// The pair of messages produced by one question, returned so the UI can append
/// them directly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KbAnswer {
    pub user_message: KbMessage,
    pub assistant_message: KbMessage,
}

#[tauri::command]
pub async fn kb_list_sessions(
    pool: State<'_, SqlitePool>,
    collection_id: String,
) -> Result<Vec<KbSession>, String> {
    sqlx::query_as::<_, KbSession>(
        "SELECT id, collection_id, title, created_at, updated_at FROM kb_sessions \
         WHERE collection_id = ?1 ORDER BY updated_at DESC",
    )
    .bind(&collection_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_create_session(
    pool: State<'_, SqlitePool>,
    collection_id: String,
) -> Result<KbSession, String> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO kb_sessions (id, collection_id) VALUES (?1, ?2)")
        .bind(&id)
        .bind(&collection_id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query_as::<_, KbSession>(
        "SELECT id, collection_id, title, created_at, updated_at FROM kb_sessions WHERE id = ?1",
    )
    .bind(&id)
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_delete_session(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    // Messages cascade on the session's foreign key.
    sqlx::query("DELETE FROM kb_sessions WHERE id = ?1")
        .bind(&id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kb_get_messages(
    pool: State<'_, SqlitePool>,
    session_id: String,
) -> Result<Vec<KbMessage>, String> {
    let rows = sqlx::query_as::<_, KbMessageRow>(
        "SELECT id, session_id, role, content, citations_json, created_at \
         FROM kb_session_messages WHERE session_id = ?1 ORDER BY created_at",
    )
    .bind(&session_id)
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(KbMessage::from).collect())
}

/// Ask a grounded question in a session: retrieve, answer from the passages
/// only, and persist the exchange. Returns both messages.
#[tauri::command]
pub async fn kb_ask(
    pool: State<'_, SqlitePool>,
    session_id: String,
    query: String,
    model: Option<String>,
) -> Result<KbAnswer, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("ask a question first".into());
    }
    let collection_id: String =
        sqlx::query_scalar("SELECT collection_id FROM kb_sessions WHERE id = ?1")
            .bind(&session_id)
            .fetch_optional(&*pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("chat session not found")?;

    // Retrieve + answer off the async runtime; only persist once it succeeds so
    // a failed answer never leaves a dangling question in the history.
    let (answer, citations) = {
        let pool = pool.inner().clone();
        let query = query.clone();
        tauri::async_runtime::spawn_blocking(move || {
            answer_blocking(&pool, &collection_id, &query, model)
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let user_id = uuid::Uuid::new_v4().to_string();
    let assistant_id = uuid::Uuid::new_v4().to_string();
    insert_message(&pool, &user_id, &session_id, "user", &query, "[]").await?;
    let citations_json = serde_json::to_string(&citations).unwrap_or_else(|_| "[]".into());
    insert_message(
        &pool,
        &assistant_id,
        &session_id,
        "assistant",
        &answer,
        &citations_json,
    )
    .await?;
    autotitle_session(&pool, &session_id, &query).await;

    Ok(KbAnswer {
        user_message: fetch_message(&pool, &user_id).await?,
        assistant_message: fetch_message(&pool, &assistant_id).await?,
    })
}

/// Retrieve the top passages and have the local model answer from them. Returns
/// a plain "not found" answer (no LLM call) when the collection has no match.
fn answer_blocking(
    pool: &SqlitePool,
    collection_id: &str,
    query: &str,
    model: Option<String>,
) -> Result<(String, Vec<Citation>), String> {
    let hits = super::retrieve::search_blocking(pool, collection_id, query, RETRIEVE_K)?;
    if hits.is_empty() {
        return Ok((
            "I couldn't find anything about that in this collection's documents. \
             Try adding more documents, or rephrasing your question."
                .to_string(),
            Vec::new(),
        ));
    }
    let (context, citations) = build_context(&hits);
    let user = format!(
        "Passages from the user's documents:\n\n{context}\nQuestion: {query}\n\n\
         Write the answer now, grounded in the passages, with [D#] citations at the end of the \
         statements they support."
    );
    let model = model.unwrap_or_else(|| crate::translate::DEFAULT_LOCAL_MODEL.to_string());
    // num_ctx gives headroom for the retrieved passages plus a comprehensive
    // answer; a longer timeout suits the fuller responses this prompt asks for.
    let answer = crate::http::ollama_chat_ex(
        &crate::http::blocking_client(),
        &crate::transform::ollama_url(),
        &model,
        SYSTEM_PROMPT,
        &user,
        0.2,
        Duration::from_secs(180),
        Some(16384),
        None,
    )
    .ok_or("the local model didn't respond — is Ollama running?")?;
    Ok((answer, citations))
}

/// Build the `[D#]` context block and the matching citation list from hits.
fn build_context(hits: &[KbHit]) -> (String, Vec<Citation>) {
    let mut context = String::new();
    let mut citations = Vec::new();
    for (i, hit) in hits.iter().enumerate() {
        let key = format!("D{}", i + 1);
        let location = match hit.page {
            Some(p) => format!("{}, p.{p}", hit.filename),
            None => hit.filename.clone(),
        };
        let quote = clip(&hit.content, QUOTE_CHARS);
        context.push_str(&format!("[{key}] ({location})\n{quote}\n\n"));
        citations.push(Citation {
            key,
            document_id: hit.document_id.clone(),
            filename: hit.filename.clone(),
            page: hit.page,
            quote,
        });
    }
    (context, citations)
}

/// Collapse whitespace and cap at `max` characters (char-safe), adding an
/// ellipsis when truncated.
fn clip(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let head: String = collapsed.chars().take(max).collect();
        format!("{head}…")
    }
}

async fn insert_message(
    pool: &SqlitePool,
    id: &str,
    session_id: &str,
    role: &str,
    content: &str,
    citations_json: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO kb_session_messages (id, session_id, role, content, citations_json) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id)
    .bind(session_id)
    .bind(role)
    .bind(content)
    .bind(citations_json)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

async fn fetch_message(pool: &SqlitePool, id: &str) -> Result<KbMessage, String> {
    sqlx::query_as::<_, KbMessageRow>(
        "SELECT id, session_id, role, content, citations_json, created_at \
         FROM kb_session_messages WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map(KbMessage::from)
    .map_err(|e| e.to_string())
}

/// Name a still-untitled session after its first question, and bump its
/// updated_at so it sorts to the top.
async fn autotitle_session(pool: &SqlitePool, session_id: &str, query: &str) {
    let title = clip(query, 48);
    let _ = sqlx::query(
        "UPDATE kb_sessions \
         SET title = CASE WHEN title = 'New chat' THEN ?2 ELSE title END, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE id = ?1",
    )
    .bind(session_id)
    .bind(&title)
    .execute(pool)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    fn hit(id: &str, filename: &str, page: Option<i64>, content: &str) -> KbHit {
        KbHit {
            chunk_id: id.into(),
            document_id: format!("doc-{id}"),
            filename: filename.into(),
            page,
            content: content.into(),
            score: 1.0,
        }
    }

    #[test]
    fn build_context_numbers_and_cites_each_hit() {
        let hits = vec![
            hit("c1", "report.pdf", Some(3), "Revenue grew 20% in Q4."),
            hit("c2", "notes.txt", None, "The hiring freeze was lifted."),
        ];
        let (context, citations) = build_context(&hits);
        assert!(context.contains("[D1] (report.pdf, p.3)"));
        assert!(context.contains("[D2] (notes.txt)"));
        assert!(context.contains("Revenue grew 20%"));
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].key, "D1");
        assert_eq!(citations[0].document_id, "doc-c1");
        assert_eq!(citations[1].page, None);
    }

    #[test]
    fn clip_collapses_and_truncates() {
        assert_eq!(clip("  a   b  ", 10), "a b");
        let long = "word ".repeat(100);
        let out = clip(&long, 20);
        assert_eq!(out.chars().count(), 21); // 20 + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn message_round_trip_parses_citations_and_autotitles() {
        let pool = tauri::async_runtime::block_on(test_pool());
        tauri::async_runtime::block_on(async {
            sqlx::query("INSERT INTO kb_collections (id, name) VALUES ('c1','C')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO kb_sessions (id, collection_id) VALUES ('s1','c1')")
                .execute(&pool)
                .await
                .unwrap();

            let citations = vec![Citation {
                key: "D1".into(),
                document_id: "doc-1".into(),
                filename: "a.pdf".into(),
                page: Some(2),
                quote: "hello".into(),
            }];
            let cj = serde_json::to_string(&citations).unwrap();
            insert_message(&pool, "m1", "s1", "user", "What is the revenue?", "[]")
                .await
                .unwrap();
            insert_message(&pool, "m2", "s1", "assistant", "It grew [D1].", &cj)
                .await
                .unwrap();
            autotitle_session(&pool, "s1", "What is the revenue?").await;

            let msg = fetch_message(&pool, "m2").await.unwrap();
            assert_eq!(msg.citations.len(), 1);
            assert_eq!(msg.citations[0].filename, "a.pdf");
            assert_eq!(msg.citations[0].page, Some(2));

            let title: String = sqlx::query_scalar("SELECT title FROM kb_sessions WHERE id='s1'")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(title, "What is the revenue?");
        });
    }

    /// End-to-end grounded answer against real local models: a seeded fact is
    /// embedded (nomic-embed-text), retrieved, and answered by the chat model
    /// (SuperGemma4). The answer — or at least a cited quote — must carry the
    /// fact, proving retrieval + grounding + citation work on-device.
    /// Run with: `cargo test -p arya --lib -- --ignored real_grounded`.
    #[ignore = "requires a local Ollama with nomic-embed-text + the chat model"]
    #[test]
    fn real_grounded_answer_carries_the_document_fact() {
        use crate::rag::embed::{Embedder, OllamaEmbedder};
        use crate::vecmath::f32_to_blob;

        let pool = tauri::async_runtime::block_on(test_pool());
        let fact = "The office guest WiFi password is banana-hotel-42.";
        let embedder = OllamaEmbedder::new(crate::transform::ollama_url());
        let emb = embedder
            .embed(&[fact.to_string()])
            .expect("embed")
            .remove(0);

        tauri::async_runtime::block_on(async {
            sqlx::query("INSERT INTO kb_collections (id, name) VALUES ('c1', 'C')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO kb_documents (id, collection_id, filename, status) \
                 VALUES ('d1', 'c1', 'handbook.pdf', 'ready')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO kb_chunks (id, collection_id, document_id, ordinal, page, content, embedding, model) \
                 VALUES ('k1', 'c1', 'd1', 0, 7, ?1, ?2, 'nomic-embed-text')",
            )
            .bind(fact)
            .bind(f32_to_blob(&emb))
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO kb_chunks_fts (chunk_id, document_id, collection_id, content) \
                 VALUES ('k1', 'd1', 'c1', ?1)",
            )
            .bind(fact)
            .execute(&pool)
            .await
            .unwrap();
        });

        let (answer, citations) =
            answer_blocking(&pool, "c1", "What is the guest wifi password?", None).unwrap();
        assert!(!citations.is_empty(), "answer should cite the source");
        let hay = format!("{answer} {}", citations[0].quote).to_lowercase();
        assert!(
            hay.contains("banana-hotel-42"),
            "grounded answer/citation should carry the fact; got answer: {answer}"
        );
    }
}
