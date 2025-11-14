-- Optimize SQLite query planner statistics
-- Migration: 004_optimize_statistics
-- Created: 2025-11-08
-- Purpose: Update query planner statistics for better performance with large datasets

-- Analyze the sessions table to update optimizer statistics
ANALYZE sessions;

-- Analyze metrics_snapshots table as well
ANALYZE metrics_snapshots;
