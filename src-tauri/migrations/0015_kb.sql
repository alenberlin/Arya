-- Knowledge Base: on-device RAG over user-uploaded documents, organized into
-- named collections. Kept deliberately separate from `rag_chunks` (the
-- workspace-wide semantic index over notes/dictation/agent chats) so external
-- documents never pollute the personal brain, and so retrieval can be scoped to
-- a single collection the user chats against.

CREATE TABLE kb_collections (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE kb_documents (
    id            TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL REFERENCES kb_collections(id) ON DELETE CASCADE,
    filename      TEXT NOT NULL,
    ext           TEXT NOT NULL DEFAULT '',
    byte_size     INTEGER NOT NULL DEFAULT 0,
    -- pending | processing | ready | failed
    status        TEXT NOT NULL DEFAULT 'pending',
    -- how the text was recovered, shown as a badge: text | ocr | mixed
    extractor     TEXT NOT NULL DEFAULT '',
    page_count    INTEGER NOT NULL DEFAULT 0,
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    error         TEXT,
    -- absolute path of the raw file copied into the workspace (for re-index)
    stored_path   TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_kb_documents_collection ON kb_documents(collection_id);

CREATE TABLE kb_chunks (
    id            TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL,
    document_id   TEXT NOT NULL REFERENCES kb_documents(id) ON DELETE CASCADE,
    ordinal       INTEGER NOT NULL,
    -- 1-based source page when the extractor preserves it (PDF); NULL otherwise
    page          INTEGER,
    content       TEXT NOT NULL,
    embedding     BLOB NOT NULL,
    model         TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_kb_chunks_collection ON kb_chunks(collection_id);
CREATE INDEX idx_kb_chunks_document ON kb_chunks(document_id);

-- Keyword leg of hybrid retrieval. A standalone FTS5 table (not external-content)
-- keyed by our TEXT chunk id, populated and pruned alongside kb_chunks. At query
-- time its BM25 ranking is fused with vector cosine via Reciprocal-Rank Fusion.
-- FTS5 has no foreign keys, so callers delete its rows by document_id explicitly.
CREATE VIRTUAL TABLE kb_chunks_fts USING fts5(
    chunk_id UNINDEXED,
    document_id UNINDEXED,
    collection_id UNINDEXED,
    content,
    tokenize = 'porter unicode61'
);

-- Grounded chat, scoped to one collection.
CREATE TABLE kb_sessions (
    id            TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL REFERENCES kb_collections(id) ON DELETE CASCADE,
    title         TEXT NOT NULL DEFAULT 'New chat',
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_kb_sessions_collection ON kb_sessions(collection_id);

CREATE TABLE kb_session_messages (
    id             TEXT PRIMARY KEY,
    session_id     TEXT NOT NULL REFERENCES kb_sessions(id) ON DELETE CASCADE,
    role           TEXT NOT NULL,            -- user | assistant
    content        TEXT NOT NULL,
    -- JSON array of the sources cited by an assistant turn ([{key,documentId,...}])
    citations_json TEXT NOT NULL DEFAULT '[]',
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_kb_session_messages_session ON kb_session_messages(session_id);
