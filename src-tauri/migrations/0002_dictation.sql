CREATE TABLE dictation_history (
    id TEXT PRIMARY KEY,
    raw_text TEXT NOT NULL,
    clean_text TEXT NOT NULL,
    app_bundle_id TEXT,
    duration_ms INTEGER NOT NULL,
    asr_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE dictionary_entries (
    id TEXT PRIMARY KEY,
    pattern TEXT NOT NULL UNIQUE,
    replacement TEXT NOT NULL,
    created_at TEXT NOT NULL
);
