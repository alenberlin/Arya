CREATE TABLE mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    command TEXT NOT NULL,
    args_json TEXT NOT NULL DEFAULT '[]',
    env_json TEXT NOT NULL DEFAULT '{}',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE routines (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    prompt TEXT NOT NULL,
    model TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'sandboxed',
    -- interval in minutes; simple, robust, and enough for "every N".
    interval_minutes INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    next_run_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE routine_runs (
    id TEXT PRIMARY KEY,
    routine_id TEXT NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
    session_id TEXT,
    status TEXT NOT NULL,             -- running | done | failed
    detail TEXT,
    started_at TEXT NOT NULL
);

CREATE INDEX idx_routine_runs ON routine_runs(routine_id, started_at DESC);
