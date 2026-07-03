ALTER TABLE transcript_turns ADD COLUMN speaker TEXT;
ALTER TABLE notes ADD COLUMN calendar_context TEXT;

CREATE TABLE speaker_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    embedding BLOB NOT NULL,
    created_at TEXT NOT NULL
);
