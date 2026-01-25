-- Create telemetry_events table for persistent telemetry storage
CREATE TABLE IF NOT EXISTS telemetry_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'error')),
    category TEXT NOT NULL,
    message TEXT NOT NULL,
    details TEXT,  -- JSON blob
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for time-based queries
CREATE INDEX IF NOT EXISTS idx_telemetry_timestamp ON telemetry_events(timestamp DESC);

-- Index for severity filtering
CREATE INDEX IF NOT EXISTS idx_telemetry_severity ON telemetry_events(severity);

-- Index for category filtering
CREATE INDEX IF NOT EXISTS idx_telemetry_category ON telemetry_events(category);

-- Composite index for common query pattern (time + severity)
CREATE INDEX IF NOT EXISTS idx_telemetry_time_severity ON telemetry_events(timestamp DESC, severity);

-- Index for cleanup queries
CREATE INDEX IF NOT EXISTS idx_telemetry_created_at ON telemetry_events(created_at);
