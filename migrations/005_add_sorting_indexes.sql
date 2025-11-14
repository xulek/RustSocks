-- Add indexes for commonly sorted columns to speed up ORDER BY queries
-- Migration: 005_add_sorting_indexes.sql

-- Index for sorting by bytes_sent (traffic analysis)
CREATE INDEX IF NOT EXISTS idx_sessions_bytes_sent ON sessions(bytes_sent);

-- Index for sorting by bytes_received (traffic analysis)
CREATE INDEX IF NOT EXISTS idx_sessions_bytes_received ON sessions(bytes_received);

-- Index for sorting by duration (performance analysis)
CREATE INDEX IF NOT EXISTS idx_sessions_duration ON sessions(duration_secs);

-- Index for sorting by destination IP (connection analysis)
CREATE INDEX IF NOT EXISTS idx_sessions_dest_ip ON sessions(dest_ip);

-- Index for sorting by protocol
CREATE INDEX IF NOT EXISTS idx_sessions_protocol ON sessions(protocol);

-- Index for sorting by ACL decision
CREATE INDEX IF NOT EXISTS idx_sessions_acl_decision ON sessions(acl_decision);

-- Update statistics for query optimizer
ANALYZE sessions;
