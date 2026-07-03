CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT 'New session',
    model TEXT NOT NULL,
    mode TEXT NOT NULL,                -- sandboxed | unrestricted
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE agent_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,                -- user | assistant
    content_json TEXT NOT NULL,        -- {text, reasoning?, tools:[{name,args,result}]}
    created_at TEXT NOT NULL
);

CREATE INDEX idx_agent_messages_session ON agent_messages(session_id, created_at);
