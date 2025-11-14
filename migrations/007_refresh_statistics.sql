-- Refresh optimizer statistics after adding new indexes
-- Migration: 007_refresh_statistics
-- Created: 2025-11-09
-- Purpose: ensure SQLite's query planner is aware of the latest indexes and heuristics.

ANALYZE sessions;
ANALYZE metrics_snapshots;
