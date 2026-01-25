-- Create aggregated_metrics table for storing periodic metric snapshots
CREATE TABLE IF NOT EXISTS aggregated_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    period_minutes INTEGER NOT NULL DEFAULT 5,  -- Aggregation period

    -- Connection metrics
    total_connections INTEGER NOT NULL DEFAULT 0,
    active_connections INTEGER NOT NULL DEFAULT 0,
    failed_connections INTEGER NOT NULL DEFAULT 0,

    -- Latency metrics (milliseconds)
    avg_latency_ms REAL NOT NULL DEFAULT 0,
    p50_latency_ms REAL NOT NULL DEFAULT 0,
    p95_latency_ms REAL NOT NULL DEFAULT 0,
    p99_latency_ms REAL NOT NULL DEFAULT 0,
    max_latency_ms REAL NOT NULL DEFAULT 0,

    -- Throughput metrics
    bytes_sent INTEGER NOT NULL DEFAULT 0,
    bytes_received INTEGER NOT NULL DEFAULT 0,

    -- Pool metrics
    pool_hits INTEGER NOT NULL DEFAULT 0,
    pool_misses INTEGER NOT NULL DEFAULT 0,
    pool_hit_rate REAL NOT NULL DEFAULT 0,
    pool_idle_connections INTEGER NOT NULL DEFAULT 0,
    pool_in_use_connections INTEGER NOT NULL DEFAULT 0,

    -- Error metrics
    auth_failures INTEGER NOT NULL DEFAULT 0,
    connection_timeouts INTEGER NOT NULL DEFAULT 0,
    upstream_errors INTEGER NOT NULL DEFAULT 0,
    acl_denials INTEGER NOT NULL DEFAULT 0,

    -- System metrics
    memory_usage_bytes INTEGER,
    cpu_usage_percent REAL,
    open_file_descriptors INTEGER,

    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for time-based queries
CREATE INDEX IF NOT EXISTS idx_agg_metrics_timestamp ON aggregated_metrics(timestamp DESC);

-- Index for period-based queries
CREATE INDEX IF NOT EXISTS idx_agg_metrics_period ON aggregated_metrics(period_minutes, timestamp DESC);

-- Index for cleanup queries
CREATE INDEX IF NOT EXISTS idx_agg_metrics_created_at ON aggregated_metrics(created_at);
