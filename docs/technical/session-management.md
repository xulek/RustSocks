# Session Management

This document describes the session tracking, persistence, and statistics features in RustSocks.

## Overview

RustSocks provides comprehensive session management including:
- Real-time session tracking
- SQLite persistence (optional)
- Traffic metrics and statistics
- Prometheus metrics integration (optional)
- REST API for session queries

## Architecture

### In-Memory Storage

```rust
// Active sessions
DashMap<String, Session>  // Concurrent hashmap, lock-free reads

// Completed sessions (snapshots)
RwLock<Vec<Session>>      // Read-heavy workload
```

**Benefits**:
- Lock-free concurrent access for active sessions
- Efficient lookups without blocking
- Minimal write contention

### Session Lifecycle

1. **Creation** (`new_session()`):
   - Generate UUID session ID
   - Store in active sessions map
   - Initialize traffic counters

2. **Traffic Updates** (`update_traffic()`):
   - Called periodically during proxy loop
   - Increment bytes/packets counters
   - Reduces write amplification

3. **Closure** (`close_session()`):
   - Mark session as completed
   - Record duration and close reason
   - Move to completed snapshots
   - Queue for database write (if enabled)

4. **Rejection Tracking** (`track_rejected_session()`):
   - Record ACL-blocked connections
   - Store matched rule and decision
   - Useful for audit and troubleshooting

## Database Persistence

**Feature Flag**: `database`

### SQLite Backend

Uses `sqlx` for async SQLite operations:
- Connection pooling
- Async migrations
- Type-safe queries
- Transaction support

### Schema

```sql
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    user TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT,
    duration_secs INTEGER,
    source_ip TEXT NOT NULL,
    source_port INTEGER NOT NULL,
    dest_ip TEXT NOT NULL,
    dest_port INTEGER NOT NULL,
    protocol TEXT NOT NULL,
    bytes_sent INTEGER NOT NULL,
    bytes_received INTEGER NOT NULL,
    packets_sent INTEGER NOT NULL,
    packets_received INTEGER NOT NULL,
    status TEXT NOT NULL,
    close_reason TEXT,
    acl_rule_matched TEXT,
    acl_decision TEXT
);

CREATE INDEX idx_sessions_user ON sessions(user);
CREATE INDEX idx_sessions_start_time ON sessions(start_time);
CREATE INDEX idx_sessions_status ON sessions(status);
```

### Batch Writer

Efficient batch writing reduces database overhead:

```toml
[sessions]
batch_size = 100           # Sessions per batch
batch_interval_ms = 1000   # Max time between flushes
```

**Algorithm**:
1. Queue sessions in memory
2. Flush when:
   - Batch size reached, OR
   - Interval elapsed, OR
   - Shutdown initiated
3. Single transaction per batch
4. Background task handles writes

**Benefits**:
- Reduced write I/O (100x fewer transactions)
- Lower contention on database
- Graceful degradation on database failure

### Cleanup Task

Automatic cleanup of old records:

```toml
[sessions]
retention_days = 90           # Keep sessions for 90 days
cleanup_interval_hours = 24   # Run cleanup daily
```

**Algorithm**:
1. Run periodically (configurable interval)
2. Delete sessions older than retention period
3. Use index on `start_time` for efficiency
4. Runs in background, non-blocking

## Traffic Tracking

### Configuration

```toml
[sessions]
traffic_update_packet_interval = 10
```

Determines how often `update_traffic()` is called during proxying.

**Trade-offs**:
- **Lower value** (e.g., 1-10): More accurate real-time stats, higher overhead
- **Higher value** (e.g., 50-100): Less overhead, delayed stats updates

### Implementation

```rust
// In proxy.rs
let mut packet_count = 0;

loop {
    tokio::select! {
        // ... data transfer ...
    }

    packet_count += 1;
    if packet_count >= update_interval {
        session_manager.update_traffic(session_id, bytes_sent, bytes_received);
        packet_count = 0;
    }
}

// Final flush on connection close
session_manager.update_traffic(session_id, bytes_sent, bytes_received);
```

## Statistics API

### Rolling Window Aggregation

```rust
pub fn get_stats(&self, window_hours: u64) -> SessionStats {
    // Aggregate sessions from last N hours
    // Returns:
    // - Active session count
    // - Total sessions/bytes
    // - Top users (by bandwidth)
    // - Top destinations (by sessions)
}
```

### HTTP Endpoint

```
GET /api/sessions/stats?window_hours=48
```

**Response**:
```json
{
  "window_hours": 48,
  "active_sessions": 42,
  "total_sessions": 15234,
  "total_bytes_sent": 523423123,
  "total_bytes_received": 234234234,
  "top_users": [
    {"user": "alice", "sessions": 523, "bytes": 523423123},
    {"user": "bob", "sessions": 234, "bytes": 234234234}
  ],
  "top_destinations": [
    {"dest": "example.com:443", "sessions": 1234},
    {"dest": "api.github.com:443", "sessions": 456}
  ]
}
```

## Prometheus Metrics

**Feature Flag**: `metrics`

### Available Metrics

```
# Active session gauge
rustsocks_active_sessions

# Total sessions counter
rustsocks_sessions_total

# Rejected sessions counter
rustsocks_sessions_rejected_total

# Session duration histogram
rustsocks_session_duration_seconds (buckets: 0.1, 0.5, 1, 5, 10, 30, 60, 300)

# Traffic counters
rustsocks_bytes_sent_total
rustsocks_bytes_received_total

# Per-user metrics
rustsocks_user_sessions_total{user="alice"}
rustsocks_user_bandwidth_bytes_total{user="alice", direction="sent"}
rustsocks_user_bandwidth_bytes_total{user="alice", direction="received"}
```

### Integration

Metrics are automatically updated:
- On session creation
- On traffic updates
- On session closure
- On ACL rejection

### Querying

```bash
# Prometheus scrape endpoint
curl http://127.0.0.1:9090/metrics

# Example queries
# Average session duration
rate(rustsocks_session_duration_seconds_sum[5m]) / rate(rustsocks_session_duration_seconds_count[5m])

# Bandwidth by user
rate(rustsocks_user_bandwidth_bytes_total{user="alice"}[5m])

# Rejection rate
rate(rustsocks_sessions_rejected_total[5m]) / rate(rustsocks_sessions_total[5m])
```

## Configuration

### Complete Example

```toml
[sessions]
enabled = true
storage = "sqlite"  # or "memory"

# Database settings
database_url = "sqlite://data/sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24

# Traffic tracking
traffic_update_packet_interval = 10

# Statistics API
stats_api_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
stats_window_hours = 24
```

## Database Operations

### Running Migrations

Migrations are applied automatically on startup:

```bash
# Manual migration (for testing)
sqlx migrate run --database-url sqlite://sessions.db
```

Migrations are located in `migrations/` directory.

### Querying Session Data

```sql
-- Active sessions
SELECT user, dest_ip, dest_port, bytes_sent, bytes_received
FROM sessions WHERE status = 'active';

-- Rejected by ACL
SELECT user, dest_ip, dest_port, acl_rule_matched
FROM sessions WHERE status = 'rejected_by_acl';

-- Top users by traffic
SELECT user,
       SUM(bytes_sent + bytes_received) as total_bytes,
       COUNT(*) as sessions
FROM sessions
GROUP BY user
ORDER BY total_bytes DESC
LIMIT 10;

-- Sessions in last hour
SELECT * FROM sessions
WHERE datetime(start_time) >= datetime('now', '-1 hour');

-- Traffic over time (hourly buckets)
SELECT
    strftime('%Y-%m-%d %H:00', start_time) as hour,
    COUNT(*) as sessions,
    SUM(bytes_sent + bytes_received) as total_bytes
FROM sessions
WHERE datetime(start_time) >= datetime('now', '-24 hours')
GROUP BY hour
ORDER BY hour;
```

### Backup and Export

```bash
# Backup SQLite database
sqlite3 sessions.db ".backup sessions_backup.db"

# Export to CSV
sqlite3 sessions.db -csv -header "SELECT * FROM sessions" > sessions.csv

# Export specific query
sqlite3 sessions.db -csv -header \
  "SELECT user, dest_ip, dest_port, bytes_sent, bytes_received FROM sessions" \
  > traffic_report.csv
```

## Performance Characteristics

| Operation | Latency | Notes |
|-----------|---------|-------|
| Session creation | <100µs | In-memory only |
| Traffic update | <50µs | DashMap lock-free read |
| Session closure | <200µs | Includes snapshot |
| Database batch write | 5-50ms | 100 sessions per batch |
| Stats query (24h window) | 10-100ms | Depends on session count |

### Memory Usage

Approximate memory per session:
- **Active session**: ~500 bytes (in DashMap)
- **Snapshot**: ~300 bytes (in Vec)
- **Database record**: ~200 bytes (on disk)

For 10,000 active sessions:
- In-memory: ~5 MB
- With snapshots: ~8 MB
- Database: ~2 MB (compressed)

## Best Practices

### When to Enable Database Persistence

Enable SQLite persistence when:
- Need audit trail of all connections
- Want historical analysis/reporting
- Compliance requires session logs
- Using statistics API extensively

### When to Use Memory-Only Mode

Use memory-only mode when:
- Temporary/development deployments
- Privacy requirements (no persistence)
- High throughput (>10k sessions/sec)
- Limited disk space

### Tuning Guidelines

1. **Batch Size** (`batch_size`):
   - Larger batches: Better throughput, higher latency
   - Smaller batches: Lower latency, more I/O overhead
   - Recommended: 50-200 for most workloads

2. **Batch Interval** (`batch_interval_ms`):
   - Lower interval: More real-time updates, more writes
   - Higher interval: Better batching, delayed persistence
   - Recommended: 1000-5000ms

3. **Traffic Update Interval** (`traffic_update_packet_interval`):
   - Lower: More accurate real-time stats, higher CPU
   - Higher: Lower overhead, delayed updates
   - Recommended: 10-50 packets

4. **Retention Period** (`retention_days`):
   - Balance compliance requirements with disk space
   - Monitor database size growth
   - Consider archiving to external storage

## Troubleshooting

### Problem: High database write latency

**Symptoms**: Slow session closures, growing batch queue

**Solutions**:
- Increase `batch_size` (fewer transactions)
- Increase `batch_interval_ms` (better batching)
- Check disk I/O performance
- Consider moving database to faster storage

### Problem: Memory usage growing

**Symptoms**: High memory consumption, OOM crashes

**Causes**:
- Too many completed session snapshots
- Batch writer queue overflow
- Database connection failure

**Solutions**:
- Limit snapshot retention (not implemented yet)
- Check database connectivity
- Monitor batch writer queue size
- Reduce `traffic_update_packet_interval`

### Problem: Statistics API slow

**Symptoms**: High latency on `/api/sessions/stats`

**Causes**:
- Large number of sessions in window
- Complex aggregation queries
- Database not indexed

**Solutions**:
- Reduce `stats_window_hours`
- Ensure indexes exist on database
- Consider caching statistics
- Use Prometheus instead for aggregations

## Related Documentation

- [Architecture Overview](architecture.md)
- [Connection Pool](connection-pool.md)
- [Web Dashboard Guide](../guides/web-dashboard.md)
- [Testing Guide](../guides/testing.md)
