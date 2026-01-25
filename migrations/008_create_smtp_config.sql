-- SQLite/MySQL compatible migration for SMTP configuration
CREATE TABLE IF NOT EXISTS smtp_config (
    id INTEGER PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 0,
    mode TEXT NOT NULL DEFAULT 'starttls_auth',
    host TEXT NOT NULL DEFAULT '',
    port INTEGER NOT NULL DEFAULT 587,
    from_address TEXT NOT NULL DEFAULT '',
    from_name TEXT,
    username TEXT,
    password_encrypted TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Insert default row (singleton)
INSERT OR IGNORE INTO smtp_config (id, enabled, mode, host, port, from_address)
VALUES (1, 0, 'starttls_auth', '', 587, '');
