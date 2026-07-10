-- F3/M5: nested pages. A note may be a child of another note; top-level notes
-- have a NULL parent. ON DELETE CASCADE means deleting a page removes its whole
-- subtree at the row level — the app layer collects the subtree's files and
-- graph edges first (see notes::delete_note_inner) so nothing is orphaned.
ALTER TABLE notes ADD COLUMN parent_note_id TEXT
    REFERENCES notes(id) ON DELETE CASCADE;

CREATE INDEX idx_notes_parent ON notes(parent_note_id);
