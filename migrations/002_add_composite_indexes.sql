-- Add composite indexes for common query patterns
-- Migration: 002_add_composite_indexes
-- Created: 2025-11-02
-- Purpose: Optimize filtered queries for status+time, destination+user, and duration-based analytics

-- Index for "closed sessions in last 24h" type queries
CREATE INDEX IF NOT EXISTS idx_sessions_status_start
ON sessions(status, start_time DESC);

-- Index for per-user destination analysis
CREATE INDEX IF NOT EXISTS idx_sessions_dest_user
ON sessions(dest_ip, user);

-- Index for duration-based analytics (only non-NULL values)
CREATE INDEX IF NOT EXISTS idx_sessions_duration
ON sessions(duration_secs)
WHERE duration_secs IS NOT NULL;

-- Index for ACL decision analysis
CREATE INDEX IF NOT EXISTS idx_sessions_acl_decision_start
ON sessions(acl_decision, start_time DESC);

-- Composite index for user stats queries (common in API)
CREATE INDEX IF NOT EXISTS idx_sessions_user_status_start
ON sessions(user, status, start_time DESC);
