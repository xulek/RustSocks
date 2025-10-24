CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    user TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT,
    duration_secs INTEGER,

    source_ip TEXT NOT NULL,
    source_port INTEGER NOT NULL,
    dest_ip TEXT NOT NULL,
    dest_port INTEGER NOT NULL,
    protocol TEXT NOT NULL,

    bytes_sent INTEGER DEFAULT 0,
    bytes_received INTEGER DEFAULT 0,
    packets_sent INTEGER DEFAULT 0,
    packets_received INTEGER DEFAULT 0,

    status TEXT NOT NULL,
    close_reason TEXT,

    acl_rule_matched TEXT,
    acl_decision TEXT NOT NULL,

    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user);
CREATE INDEX IF NOT EXISTS idx_sessions_start_time ON sessions(start_time DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_dest_ip ON sessions(dest_ip);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_user_start ON sessions(user, start_time DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_start_time_asc ON sessions(start_time ASC);
