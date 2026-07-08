-- F11/M12: mind maps. The whole canvas (nodes, edges, viewport) is stored as one
-- opaque JSON document — React Flow owns its shape, and the app treats it as a
-- blob (like a note's document_json). Title is separate for listing.
CREATE TABLE mindmaps (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    doc_json    TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX idx_mindmaps_updated ON mindmaps(updated_at DESC);
