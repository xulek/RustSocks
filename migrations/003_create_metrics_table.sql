-- Create metrics_snapshots table for storing historical metrics
CREATE TABLE IF NOT EXISTS metrics_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    active_sessions INTEGER NOT NULL,
    total_sessions INTEGER NOT NULL,
    bandwidth INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for efficient time-based queries
CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON metrics_snapshots(timestamp);

-- Index for cleanup queries
CREATE INDEX IF NOT EXISTS idx_metrics_created_at ON metrics_snapshots(created_at);
