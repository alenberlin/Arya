-- Enforce notes.folder_id -> folders.id so a dangling folder id can never make
-- a note invisible in every list. ON DELETE SET NULL makes folder deletion
-- revert its notes to "unfiled" at the row level (the app already nulls the
-- column, but this closes the crash / manual-edit gap). Recreating the table is
-- required because SQLite cannot add a FOREIGN KEY to an existing column.

PRAGMA foreign_keys = OFF;

-- Re-point any dangling id to "unfiled" before the FK is enforced.
UPDATE notes
SET folder_id = NULL
WHERE folder_id IS NOT NULL
  AND folder_id NOT IN (SELECT id FROM folders);

CREATE TABLE notes_new (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    body_md TEXT NOT NULL DEFAULT '',
    manual_notes TEXT NOT NULL DEFAULT '',
    processing_status TEXT NOT NULL DEFAULT 'idle',
    processing_error TEXT,
    folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
    updated_at TEXT,
    calendar_context TEXT,
    document_json TEXT NOT NULL DEFAULT '',
    parent_note_id TEXT REFERENCES notes(id) ON DELETE CASCADE
);

INSERT INTO notes_new
    (id, title, created_at, body_md, manual_notes, processing_status,
     processing_error, folder_id, updated_at, calendar_context, document_json,
     parent_note_id)
SELECT id, title, created_at, body_md, manual_notes, processing_status,
       processing_error, folder_id, updated_at, calendar_context, document_json,
       parent_note_id
FROM notes;

DROP TABLE notes;
ALTER TABLE notes_new RENAME TO notes;

CREATE INDEX idx_notes_folder ON notes(folder_id);
CREATE INDEX idx_notes_parent ON notes(parent_note_id);

PRAGMA foreign_keys = ON;
