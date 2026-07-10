-- The connected-brain edge store (F1). One polymorphic table of directed edges
-- between nodes, where a node is any first-class item — a note, dictation,
-- meeting, or mind map — named by (kind, id). Edges are deliberately NOT
-- enforced by SQL foreign keys: a target may be of any kind, and a dangling
-- target is permitted and resolved at read time. Referential cleanup when a
-- node is deleted is handled in the app layer (see links::delete_for_node).
CREATE TABLE links (
    id           TEXT PRIMARY KEY,
    source_kind  TEXT NOT NULL,                    -- note | dictation | meeting | mindmap
    source_id    TEXT NOT NULL,
    target_kind  TEXT NOT NULL,                    -- note | dictation | meeting | mindmap
    target_id    TEXT NOT NULL,
    relation     TEXT NOT NULL DEFAULT 'mention',  -- mention | semantic | manual | ...
    origin       TEXT NOT NULL DEFAULT 'user',     -- user | agent | system
    weight       REAL NOT NULL DEFAULT 1.0,
    created_at   TEXT NOT NULL
);

-- One edge per (source, target, relation): creating the same edge again is
-- idempotent (the insert upserts), so reconciling a document's mentions never
-- duplicates. Distinct relations between the same pair coexist.
CREATE UNIQUE INDEX idx_links_edge
    ON links (source_kind, source_id, target_kind, target_id, relation);

-- Neighbourhood lookups from either end; backlinks read the target side.
CREATE INDEX idx_links_source ON links (source_kind, source_id);
CREATE INDEX idx_links_target ON links (target_kind, target_id);
