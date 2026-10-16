-- Speed up the "recent notes" sidebar list and the timeline search hits.
-- list_notes / search / notebooks all ORDER BY notes.created_at DESC (often
-- with a secondary tiebreak for stable ordering inside the same second).
-- Without an index that's a full scan + filesort on every refresh; on a
-- long-running user this gets measurable once notes.count > a few thousand.
-- Note: SQLite rejects bare `rowid` in CREATE INDEX column lists (the implicit
-- rowid alias is only resolved in table context), so the tie-break column is
-- `id` (TEXT PK) — already unique, already indexed as the PK, so it provides
-- a stable secondary order without an extra sort.
CREATE INDEX IF NOT EXISTS idx_notes_created_at_desc
    ON notes (created_at DESC, id DESC);