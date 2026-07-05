CREATE TABLE note_attachments (
    id          TEXT PRIMARY KEY,
    note_id     TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,        -- original filename (with extension)
    path        TEXT NOT NULL,        -- absolute stored path in the workspace
    size_bytes  INTEGER NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE INDEX idx_note_attachments_note ON note_attachments(note_id);
