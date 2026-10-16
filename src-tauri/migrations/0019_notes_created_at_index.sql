-- Speed up the "recent notes" sidebar list and the timeline search hits.
-- list_notes / search / notebooks all ORDER BY notes.created_at DESC (often
-- with a secondary rowid DESC tiebreak for stable ordering inside the same
-- second). Without an index that's a full scan + filesort on every refresh;
-- on a long-running user this gets measurable once notes.count > a few thousand.
CREATE INDEX IF NOT EXISTS idx_notes_created_at_desc
    ON notes (created_at DESC, rowid DESC);