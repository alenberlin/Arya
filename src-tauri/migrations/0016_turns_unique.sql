-- Prevent duplicate turns from concurrent finish/retry processing threads.
-- First collapse any existing (note_id, turn_index) collisions, keeping the
-- earliest row; then enforce uniqueness so a future race fails cleanly instead
-- of storing a second set of turns.
DELETE FROM transcript_turns
WHERE id NOT IN (
    SELECT MIN(id) FROM transcript_turns GROUP BY note_id, turn_index
);

DROP INDEX IF EXISTS idx_turns_note;
CREATE UNIQUE INDEX idx_turns_unique ON transcript_turns(note_id, turn_index);
