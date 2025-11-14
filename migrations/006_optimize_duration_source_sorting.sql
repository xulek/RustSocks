-- Migration: 006_optimize_duration_source_sorting.sql
-- Purpose: Accelerate ORDER BY queries on duration_secs and source_ip, and fix legacy duration data
-- Date: 2025-11-08

-- Normalize invalid duration values that were stored as negative numbers (should be NULL).
UPDATE sessions
SET duration_secs = NULL
WHERE duration_secs < 0;

-- Replace the partial duration index with a covering one that keeps ordering stable.
DROP INDEX IF EXISTS idx_sessions_duration;
CREATE INDEX IF NOT EXISTS idx_sessions_duration_order
ON sessions(duration_secs, session_id);

-- Add covering index for source_ip sorting (commonly used for filtering/sorting in dashboard).
CREATE INDEX IF NOT EXISTS idx_sessions_source_ip
ON sessions(source_ip, session_id);

-- Refresh statistics for the query planner.
ANALYZE sessions;
