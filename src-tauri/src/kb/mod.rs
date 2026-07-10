//! On-device Knowledge Base: user-uploaded documents organized into named
//! collections, ingested locally (extract → chunk → embed) and queried with
//! grounded, source-cited chat.
//!
//! The KB reuses the workspace RAG engine's building blocks — the local
//! [`crate::rag::embed::Embedder`] (nomic-embed-text, 768-dim) and the shared
//! [`crate::vecmath`] cosine + f32⇄blob encoding — so the embedding model and
//! vector encoding can never drift from the rest of the app. It stays in its own
//! tables (`kb_*`) rather than the workspace `rag_chunks` index so external
//! documents are never mixed into the personal brain and every query is scoped
//! to one collection.

pub mod chat;
pub mod commands;
pub mod extract;
pub mod ingest;
pub mod ocr;
pub mod retrieve;

use serde::Serialize;

/// A named knowledge base: a set of uploaded documents the user chats against in
/// isolation. `document_count` / `ready_count` are derived per query so the UI
/// can show ingestion progress without a second round-trip.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub document_count: i64,
    pub ready_count: i64,
}
