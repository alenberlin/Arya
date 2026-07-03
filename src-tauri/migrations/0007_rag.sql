-- Local semantic index. Embeddings stored as f32 LE blobs; search is
-- brute-force cosine in Rust (fast enough for a personal workspace, and
-- avoids a SQLite extension dependency). One row per indexed chunk.
CREATE TABLE rag_chunks (
    id TEXT PRIMARY KEY,
    source_kind TEXT NOT NULL,        -- note | transcript | dictation | session
    source_id TEXT NOT NULL,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding BLOB NOT NULL,
    model TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_rag_source ON rag_chunks(source_kind, source_id);
