-- Create alert_thresholds table for configurable alert settings
CREATE TABLE IF NOT EXISTS alert_thresholds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    metric_name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    category TEXT NOT NULL,  -- 'performance', 'errors', 'resources', 'limits'
    warning_threshold REAL,
    error_threshold REAL,
    comparison TEXT NOT NULL DEFAULT 'gt',  -- 'gt', 'lt', 'gte', 'lte'
    enabled INTEGER NOT NULL DEFAULT 1,
    unit TEXT,  -- 'ms', '%', 'bytes', 'count', etc.
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Insert default thresholds
INSERT OR IGNORE INTO alert_thresholds (metric_name, display_name, category, warning_threshold, error_threshold, comparison, unit, description) VALUES
    -- Performance thresholds
    ('avg_latency_ms', 'Average Latency', 'performance', 100, 500, 'gt', 'ms', 'Average connection latency'),
    ('p95_latency_ms', 'P95 Latency', 'performance', 200, 1000, 'gt', 'ms', '95th percentile latency'),
    ('p99_latency_ms', 'P99 Latency', 'performance', 500, 2000, 'gt', 'ms', '99th percentile latency'),

    -- Error thresholds (per minute rates)
    ('error_rate', 'Error Rate', 'errors', 5, 15, 'gt', '%', 'Percentage of failed connections'),
    ('auth_failure_rate', 'Auth Failure Rate', 'errors', 10, 25, 'gt', '%', 'Authentication failure rate'),
    ('timeout_rate', 'Timeout Rate', 'errors', 5, 10, 'gt', '%', 'Connection timeout rate'),

    -- Resource thresholds
    ('memory_usage_percent', 'Memory Usage', 'resources', 70, 90, 'gt', '%', 'Memory usage percentage'),
    ('cpu_usage_percent', 'CPU Usage', 'resources', 70, 90, 'gt', '%', 'CPU usage percentage'),
    ('fd_usage_percent', 'File Descriptors', 'resources', 70, 90, 'gt', '%', 'File descriptor usage percentage'),

    -- Limit thresholds
    ('active_connections_percent', 'Active Connections', 'limits', 70, 90, 'gt', '%', 'Percentage of max connections in use'),
    ('pool_utilization', 'Pool Utilization', 'limits', 80, 95, 'gt', '%', 'Connection pool utilization'),
    ('pool_hit_rate', 'Pool Hit Rate', 'limits', 50, 30, 'lt', '%', 'Connection pool hit rate (lower is worse)');

-- Create alert_history table for tracking triggered alerts
CREATE TABLE IF NOT EXISTS alert_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    metric_name TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('warning', 'error')),
    current_value REAL NOT NULL,
    threshold_value REAL NOT NULL,
    message TEXT NOT NULL,
    triggered_at TEXT NOT NULL,
    resolved_at TEXT,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    acknowledged_at TEXT,
    acknowledged_by TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for active alerts
CREATE INDEX IF NOT EXISTS idx_alert_history_active ON alert_history(resolved_at) WHERE resolved_at IS NULL;

-- Index for time-based queries
CREATE INDEX IF NOT EXISTS idx_alert_history_triggered ON alert_history(triggered_at DESC);

-- Index for metric-based queries
CREATE INDEX IF NOT EXISTS idx_alert_history_metric ON alert_history(metric_name, triggered_at DESC);
