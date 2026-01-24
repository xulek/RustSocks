# Connection Pool & Optimization

RustSocks includes an efficient connection pool for upstream TCP connections, reducing connection establishment overhead and improving performance.

## Overview

The connection pool reuses TCP connections to frequently accessed destinations, eliminating repeated TCP handshake overhead and improving throughput.

## How It Works

1. **Pool Management**: Idle upstream connections are stored per-destination
2. **Connection Reuse**: When connecting to the same destination, pooled connections are reused
3. **Timeout Handling**: Connections expire after `idle_timeout_secs` of inactivity
4. **Background Cleanup**: Periodic cleanup task removes expired connections
5. **Capacity Limits**: Both per-destination and global limits prevent resource exhaustion

## Key Features

- ✅ LRU-style connection pooling with timeout management
- ✅ Per-destination and global connection limits
- ✅ Configurable idle timeout and connect timeout
- ✅ Background cleanup of expired connections
- ✅ Thread-safe implementation using DashMap and atomic counters
- ✅ Optional (disabled by default for backward compatibility)
- ✅ Zero-copy connection reuse
- ✅ Automatic eviction when limits are reached

## Configuration

```toml
[server.pool]
enabled = true                # Enable connection pooling
max_idle_per_dest = 4        # Max idle connections per destination
max_total_idle = 100         # Max total idle connections
idle_timeout_secs = 90       # Keep-alive duration
connect_timeout_ms = 5000    # Connection timeout
```

### Configuration Parameters

- **`enabled`**: Enable/disable the connection pool (default: `false`)
- **`max_idle_per_dest`**: Maximum idle connections per destination (default: 4)
- **`max_total_idle`**: Maximum total idle connections across all destinations (default: 100)
- **`idle_timeout_secs`**: How long to keep idle connections alive (default: 90 seconds)
- **`connect_timeout_ms`**: Timeout for establishing new connections (default: 5000ms)

## Benefits

- **Reduced Latency**: Reusing connections eliminates TCP handshake overhead
  - Savings depend on network latency and destination reuse
- **Lower CPU Usage**: Fewer connection establishments reduce CPU overhead
- **Better Resource Utilization**: Controlled connection limits prevent resource exhaustion
- **Improved Throughput**: Faster connection reuse for frequent destinations

## Implementation Details

### Location
`src/server/pool.rs`

### Key Structures

```rust
// Main pool manager
pub struct ConnectionPool {
    config: PoolConfig,
    pools: Arc<DashMap<SocketAddr, Vec<PooledConnection>>>,
    destination_metrics: Arc<DashMap<SocketAddr, DestinationMetrics>>,
    metrics: Arc<PoolMetrics>,
    active_counts: Arc<DashMap<SocketAddr, AtomicUsize>>,
}

// Wrapper with metadata
struct PooledConnection {
    stream: TcpStream,
    created_at: Instant,
    last_used: Instant,
}

// Configuration parameters
pub struct PoolConfig {
    pub enabled: bool,
    pub max_idle_per_dest: usize,
    pub max_total_idle: usize,
    pub idle_timeout_secs: u64,
    pub connect_timeout_ms: u64,
}

// Pool statistics API
pub struct PoolStats {
    pub total_idle: usize,
    pub destinations: usize,
    pub total_created: u64,
    pub total_reused: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub dropped_full: u64,
    pub expired: u64,
    pub evicted: u64,
    pub connections_in_use: u64,
    pub pending_creates: u64,
}
```

### Integration

The connection pool is integrated into the handler via `ConnectHandlerContext`:

```rust
// In handler.rs
match connect_ctx.connection_pool.get(target).await {
    Ok(stream) => stream,
    Err(e) => return Err(e.into()),
};
```

### Connection Lifecycle

1. **Get or Connect**: Check pool for idle connection matching destination
2. **Validation**: Verify connection is not closed/expired
3. **Reuse**: Return pooled connection if valid
4. **New Connection**: Establish new connection if pool empty or all expired
5. **Return to Pool**: On connection close, return to pool if limits allow
6. **Cleanup**: Background task periodically removes expired connections

## Testing

### Test Suite Overview

**Note**: Test counts change over time. Use `cargo test -- --list` for current totals.

```bash
# Run all pool tests
cargo test --all-features pool

# Run pool unit tests
cargo test --all-features --lib pool

# Run pool integration tests
cargo test --all-features --test connection_pool

# Run pool edge case tests
cargo test --all-features --test pool_edge_cases

# Run pool SOCKS integration tests
cargo test --all-features --test pool_socks_integration

# Run concurrency stress tests
cargo test --all-features --test pool_concurrency -- --ignored --nocapture
```

### Test Coverage

- **Basic integration** (`connection_pool.rs`): Connection reuse, timeout handling, disabled mode
- **Edge cases** (`pool_edge_cases.rs`):
  - Closed servers
  - Expired connections
  - Per-destination limits
  - Global limits
  - Stats accuracy
  - Concurrent operations
  - LIFO behavior
  - Cleanup tasks
- **SOCKS5 integration** (`pool_socks_integration.rs`):
  - Full SOCKS5 flows with pooling
  - Error handling
  - Stats reflection
- **Stress tests** (`pool_concurrency.rs`):
  - 200-500 concurrent operations
  - Contention benchmarks

## Performance Under Load

Performance depends on your destination mix, idle timeouts, and reuse rate. For current results, run the load tests in `loadtests/` and observe the pool metrics and telemetry.

### What to Measure

- Reuse rate (`total_reused` vs `total_created`)
- Pool hits vs misses
- Evictions and expirations
- Connection latency under load
- Telemetry warnings for pool pressure

### Why DashMap + Atomics

The pool uses per-destination sharding (`DashMap`) and atomic counters to avoid a global lock on hot paths while keeping statistics cheap to update.

## Best Practices

### When to Enable

Enable connection pooling when:
- Clients repeatedly connect to same destinations
- Network latency to destinations is significant (>5ms)
- Connection establishment overhead is noticeable
- You want to reduce CPU usage for connection setup

### When to Disable

Consider disabling when:
- Destinations are highly varied (low reuse rate)
- Upstream servers close idle connections quickly
- Memory is constrained (pool adds overhead)
- Connections are long-lived (less benefit from pooling)

### Tuning Guidelines

1. **Per-Destination Limit** (`max_idle_per_dest`):
   - Start with 4 for most workloads
   - Increase to 8-16 for high-traffic destinations
   - Decrease to 2 if memory is constrained

2. **Global Limit** (`max_total_idle`):
   - Set to `max_idle_per_dest × typical_destinations`
   - Example: 4 × 50 = 200 for 50 frequent destinations
   - Monitor actual pool size with statistics API

3. **Idle Timeout** (`idle_timeout_secs`):
   - Set lower than upstream server's idle timeout
   - Typical values: 60-120 seconds
   - Shorter timeout reduces stale connections
   - Longer timeout improves reuse rate

4. **Connect Timeout** (`connect_timeout_ms`):
   - Balance between retry speed and failure detection
   - Typical values: 3000-10000ms
   - Increase for high-latency networks
   - Decrease for low-latency, reliable networks

### Monitoring

Use the pool statistics API to monitor performance (requires `sessions.stats_api_enabled = true`):

```bash
curl http://127.0.0.1:9090/api/pool/stats
```

Key fields in the response:
- `total_idle`, `destinations`, `connections_in_use`
- `total_created`, `total_reused`, `pool_hits`, `pool_misses`
- `dropped_full`, `evicted`, `expired`, `pending_creates`

Key metrics to watch:
- **Reuse rate**: `total_reused / total_created` should be high for benefit
- **Pool utilization**: `total_idle / max_total_idle` indicates capacity usage
- **Per-destination distribution**: Check if limits are hit frequently

The new operational telemetry endpoint (`GET /api/telemetry/events`) surfaces warnings
when connections are dropped because a per-destination cap was hit or when the global idle
limit forces an eviction. Pair that feed with the stats API for quick diagnostics in the dashboard.

## Troubleshooting

### Problem: Low reuse rate

**Symptoms**: `total_created` >> `total_reused`

**Causes**:
- Destinations too varied (many unique destinations)
- Idle timeout too short (connections expire before reuse)
- Upstream servers closing connections

**Solutions**:
- Increase `idle_timeout_secs`
- Check upstream server keep-alive settings
- Monitor destination distribution

### Problem: Connection failures after reuse

**Symptoms**: Errors immediately after `get_or_connect()`

**Causes**:
- Upstream server closed connection while idle
- Connection expired but not yet cleaned up

**Solutions**:
- Pool validates connections before reuse
- Decrease `idle_timeout_secs` to match upstream
- Check server logs for connection reset errors

### Problem: High memory usage

**Symptoms**: Pool growing unbounded

**Causes**:
- `max_total_idle` set too high
- Many unique destinations (pool keeps connections for each)
- Cleanup task not running

**Solutions**:
- Reduce `max_total_idle` and `max_idle_per_dest`
- Monitor pool size with statistics API
- Verify cleanup task is running

### Problem: Contention under heavy load

**Symptoms**: High CPU usage, poor throughput scaling

**Causes**:
- Very high concurrency (1000+ simultaneous connections)
- All connections to same destination (hot-spot contention)

**Solutions**:
- Profile with `cargo flamegraph` to confirm contention
- Consider sharding pool by destination hash
- Check if workload suits pooling (varied vs single destination)

## Related Documentation

- [Architecture Overview](architecture.md)
- [Session Management](session-management.md)
- [Testing Guide](../guides/testing.md)
- [Load Testing](../references/loadtests-manual.md)
