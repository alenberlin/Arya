ALTER TABLE notes ADD COLUMN body_md TEXT NOT NULL DEFAULT '';
ALTER TABLE notes ADD COLUMN manual_notes TEXT NOT NULL DEFAULT '';
ALTER TABLE notes ADD COLUMN processing_status TEXT NOT NULL DEFAULT 'idle';
ALTER TABLE notes ADD COLUMN processing_error TEXT;
ALTER TABLE notes ADD COLUMN folder_id TEXT;
ALTER TABLE notes ADD COLUMN updated_at TEXT;

CREATE TABLE folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE recording_sessions (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    status TEXT NOT NULL,               -- recording | paused | finished | discarded
    source_mode TEXT NOT NULL,          -- microphone-only (mic+system arrives in M5)
    sample_rate INTEGER NOT NULL,
    channels INTEGER NOT NULL,
    elapsed_ms INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE audio_artifacts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES recording_sessions(id) ON DELETE CASCADE,
    source TEXT NOT NULL,               -- microphone | system
    path TEXT NOT NULL,
    status TEXT NOT NULL,               -- partial | final
    size_bytes INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE transcript_turns (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    turn_index INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    text TEXT NOT NULL
);

CREATE INDEX idx_notes_folder ON notes(folder_id);
CREATE INDEX idx_sessions_note ON recording_sessions(note_id);
CREATE INDEX idx_turns_note ON transcript_turns(note_id, turn_index);
