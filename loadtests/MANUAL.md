# RustSocks Load Testing Manual

Complete guide to the RustSocks load testing framework. This document describes all available test scenarios, how to run them, and how to interpret results.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Test Scenarios](#test-scenarios)
3. [Running Tests](#running-tests)
4. [Performance Targets](#performance-targets)
5. [Interpreting Results](#interpreting-results)
6. [Configuration Requirements](#configuration-requirements)
7. [Troubleshooting](#troubleshooting)

---

## Quick Start

### Prerequisites

```bash
# Build release binaries
cargo build --release --example loadtest --example echo_server

# Start echo server (required for all tests)
./target/release/examples/echo_server --port 9999 &

# Start RustSocks proxy
./target/release/rustsocks --config config/rustsocks.toml &
```

### Run Your First Test

```bash
# Quick handshake throughput test
./target/release/examples/loadtest \
    --scenario handshake-only \
    --duration 10 \
    --proxy 127.0.0.1:1080 \
    --upstream 127.0.0.1:9999
```

### Run All Automated Tests

```bash
# Full test suite (10-15 minutes)
bash loadtests/run_loadtests.sh --socks

# Quick mode (reduced duration, 3-5 minutes)
bash loadtests/run_loadtests.sh --socks --quick
```

---

## Test Scenarios

### Core Pipeline Tests

#### 1. `minimal-pipeline` - Pure SOCKS5 Overhead
**Measures:** Baseline SOCKS5 proxy performance without any features
**Config Requirements:** `acl.enabled=false`, `sessions.enabled=false`, `qos.enabled=false`
**Expected:** <10ms latency, >5000 conn/s
**Purpose:** Establish baseline performance for comparison

```bash
./target/release/examples/loadtest --scenario minimal-pipeline --duration 30
```

**Result Interpretation:**
- This represents the absolute minimum overhead of the SOCKS5 proxy
- Use this as a baseline to measure the cost of additional features
- Latency >10ms indicates potential TCP/networking issues

---

#### 2. `full-pipeline` - Complete Production Configuration
**Measures:** End-to-end performance with all features enabled
**Config Requirements:** `acl.enabled=true`, `sessions.enabled=true`, `qos.enabled=true`, `storage=sqlite`
**Expected:** <100ms latency, >1000 conn/s
**Purpose:** Measure real-world production performance

```bash
./target/release/examples/loadtest --scenario full-pipeline --duration 30
```

**Result Interpretation:**
- This represents actual production performance
- Compare with `minimal-pipeline` to measure feature overhead
- Latency breakdown: TCP (1ms) + SOCKS5 (1ms) + ACL (1-5ms) + Session (1ms) + QoS (<1ms) + DB (async)

---

#### 3. `handshake-only` - Connection Establishment
**Measures:** Pure connection throughput without data transfer
**Config Requirements:** Any
**Expected:** >1000 conn/s
**Purpose:** Test connection handling capacity

```bash
./target/release/examples/loadtest --scenario handshake-only --duration 30
```

**Result Interpretation:**
- High throughput (>5000 conn/s) indicates excellent connection handling
- Low throughput may indicate exhausted file descriptors or connection limits
- Compare with `full-pipeline` to see data transfer impact

---

### Data Transfer Tests

#### 4. `data-transfer` - Sustained Bandwidth
**Measures:** Proxy bandwidth with bidirectional traffic
**Config Requirements:** Any
**Expected:** >100MB/s (hardware dependent)
**Purpose:** Measure data proxying throughput

```bash
./target/release/examples/loadtest --scenario data-transfer --duration 30
```

**Result Interpretation:**
- Bandwidth is highly dependent on hardware and network
- WSL2 localhost: ~10-50 MB/s (limited by virtualization)
- Native Linux/bare metal: >100 MB/s
- Check `Total Transfer` field for total MB transferred

---

#### 5. `large-transfer` - Multi-MB Transfers
**Measures:** Throughput with 10 MB transfers per connection
**Config Requirements:** Any
**Expected:** >100MB/s sustained
**Purpose:** Test large file proxying

```bash
./target/release/examples/loadtest --scenario large-transfer --duration 30
```

**Result Interpretation:**
- Focuses on sustained large transfers (like file downloads)
- Lower connection count (10 concurrent) to emphasize bandwidth
- Check handshake latency separately - should be <5ms

---

### Component-Specific Tests

#### 6. `acl-evaluation` - ACL Rule Matching
**Measures:** ACL engine performance with rule evaluation
**Config Requirements:** `acl.enabled=true` with multiple rules and groups
**Expected:** <5ms per evaluation, >1000 eval/s
**Purpose:** Measure ACL overhead in isolation

```bash
./target/release/examples/loadtest --scenario acl-evaluation --duration 30
```

**Result Interpretation:**
- **Target:** <5ms average latency
- ACL evaluation happens on every connection
- Latency >5ms may indicate:
  - Too many rules (consider rule consolidation)
  - RwLock contention (check with profiling)
  - Complex regex patterns in rule matchers

---

#### 7. `pool-efficiency` - Connection Pool
**Measures:** Connection pool hit rate and reuse effectiveness
**Config Requirements:** `server.pool.enabled=true`
**Expected:** 30-50% lower latency vs full-pipeline
**Purpose:** Verify pool is working correctly

```bash
./target/release/examples/loadtest --scenario pool-efficiency --duration 30
```

**Result Interpretation:**
- **Compare** latency with `full-pipeline` test
- Effective pool reuse shows 30-50% latency reduction
- Example: 1.3ms (pool) vs 2.5ms (full) = 48% improvement
- Check proxy logs for pool hit/miss statistics
- Low improvement indicates:
  - Pool disabled or misconfigured
  - Connections timing out too quickly (increase `idle_timeout_secs`)
  - Pool size too small (increase `max_idle_per_dest`)

---

#### 8. `session-churn` - Database Write Throughput
**Measures:** SQLite batch write performance with rapid session turnover
**Config Requirements:** `sessions.enabled=true`, `storage=sqlite`
**Expected:** >1000 sessions/sec write throughput
**Purpose:** Stress test database batch writer

```bash
./target/release/examples/loadtest --scenario session-churn --duration 30
```

**Result Interpretation:**
- 200 concurrent short-lived connections create high DB write pressure
- **Target:** >1000 sessions/sec
- Low throughput may indicate:
  - Small `batch_size` (increase to 500-1000)
  - Long `batch_interval_ms` (decrease to 500-1000ms)
  - Disk I/O bottleneck (use SSD, increase cache)
  - SQLite lock contention (consider `journal_mode=WAL`)

---

### Stability & Stress Tests

#### 9. `long-lived` - Connection Stability
**Measures:** Connection stability over extended duration
**Config Requirements:** Any
**Expected:** 0% connection drops, stable latency
**Purpose:** Test connection keepalive and stability

```bash
./target/release/examples/loadtest --scenario long-lived --duration 60
```

**Result Interpretation:**
- 20 connections maintained for full test duration
- Sends keepalive every 2 seconds
- **Success:** All connections remain stable (20/20 successful)
- Connection drops may indicate:
  - TCP keepalive timeout issues
  - Proxy shutdown/restart
  - Network instability
  - Memory leaks causing crashes

---

#### 10. `concurrent-1000` - Medium Concurrency
**Measures:** Concurrent connection handling (1000 connections)
**Config Requirements:** `max_connections >= 1000`
**Expected:** 100% success rate
**Purpose:** Test concurrent connection capacity

```bash
./target/release/examples/loadtest --scenario concurrent-1000
```

**Result Interpretation:**
- Establishes 1000 connections in batches of 50
- **Target:** 100% success rate (1000/1000 successful)
- Failures may indicate:
  - `max_connections` limit reached
  - File descriptor limits (ulimit -n)
  - System resource exhaustion

---

#### 11. `concurrent-5000` - High Concurrency
**Measures:** High concurrent connection handling (5000 connections)
**Config Requirements:** `max_connections >= 5000`
**Expected:** 100% success rate
**Purpose:** Stress test connection capacity

```bash
./target/release/examples/loadtest --scenario concurrent-5000
```

**Result Interpretation:**
- Establishes 5000 connections in batches of 100
- **Target:** 100% success rate (5000/5000 successful)
- System preparation required:
  ```bash
  # Increase file descriptor limit
  ulimit -n 65536

  # Adjust system limits
  sudo sysctl -w net.core.somaxconn=4096
  sudo sysctl -w net.ipv4.ip_local_port_range="1024 65535"
  ```

---

### Manual Comparison Tests

These tests require running twice with different configurations to measure overhead.

#### 12. `auth-overhead` - Authentication Method Comparison
**Measures:** Authentication overhead by comparing NoAuth vs UserPass
**Config Requirements:** Run twice
**Purpose:** Measure authentication cost

```bash
# Test 1: No authentication
# config: auth.socks_method = "none"
./target/release/examples/loadtest --scenario auth-overhead --duration 30

# Test 2: Username/password authentication
# config: auth.socks_method = "userpass"
./target/release/examples/loadtest --scenario auth-overhead --duration 30 \
    --username alice --password secret123
```

**Result Interpretation:**
- Compare latency between both runs
- Expected overhead: 1-2ms for username/password auth
- Higher overhead may indicate password hashing issues

---

#### 13. `qos-limiting` - Rate Limiting Verification
**Measures:** QoS bandwidth throttling effectiveness
**Config Requirements:** Configure `qos.htb.max_bandwidth_bytes_per_sec`
**Purpose:** Verify rate limiting works correctly

```bash
# Configure qos.htb.max_bandwidth_bytes_per_sec = 1048576 (1 MB/s)
./target/release/examples/loadtest --scenario qos-limiting --duration 30
```

**Result Interpretation:**
- Bandwidth should not exceed configured limit
- Check `Total Transfer` and test duration to calculate bandwidth
- Formula: `(Total Transfer MB) / (Duration seconds) = Bandwidth MB/s`

---

#### 14. `dns-resolution` - DNS Resolver Performance
**Measures:** DNS resolution speed for IPv4, IPv6, and domains
**Config Requirements:** Vary `--upstream` parameter
**Purpose:** Test DNS resolver

```bash
# IPv4 resolution (baseline)
./target/release/examples/loadtest --scenario dns-resolution \
    --upstream 127.0.0.1:9999 --duration 30

# Domain resolution
./target/release/examples/loadtest --scenario dns-resolution \
    --upstream google.com:80 --duration 30

# IPv6 resolution (if available)
./target/release/examples/loadtest --scenario dns-resolution \
    --upstream [::1]:9999 --duration 30
```

**Result Interpretation:**
- IPv4 direct: Fastest (baseline)
- Domain names: Add DNS lookup overhead (1-50ms depending on cache)
- IPv6: Similar to IPv4 if properly configured
- High latency may indicate DNS resolver issues

---

#### 15. `metrics-overhead` - Prometheus Metrics Impact
**Measures:** Overhead of Prometheus metrics collection
**Config Requirements:** Run twice
**Purpose:** Measure metrics collection cost

```bash
# Test 1: Metrics enabled
# config: metrics.enabled = true
./target/release/examples/loadtest --scenario metrics-overhead --duration 30

# Test 2: Metrics disabled
# config: metrics.enabled = false
./target/release/examples/loadtest --scenario metrics-overhead --duration 30
```

**Result Interpretation:**
- Compare throughput and latency between both runs
- Expected overhead: <5% performance impact
- High overhead may indicate:
  - Too many metrics being collected
  - Metrics collection interval too frequent
  - Lock contention on metric updates

---

## Running Tests

### Using the Load Test Tool Directly

```bash
./target/release/examples/loadtest [OPTIONS]

Options:
  -s, --scenario <SCENARIO>    Test scenario to run (required)
  -p, --proxy <PROXY>          SOCKS5 proxy address [default: 127.0.0.1:1080]
  -u, --upstream <UPSTREAM>    Upstream test server [default: 127.0.0.1:9999]
  -d, --duration <DURATION>    Test duration in seconds [default: 30]
  -U, --username <USERNAME>    Username for authenticated tests
      --password <PASSWORD>    Password for authenticated tests
  -o, --output <OUTPUT>        Output results to JSON file
```

### Using the Test Runner Script

The test runner script provides automated test execution with proper setup and cleanup.

```bash
bash loadtests/run_loadtests.sh [OPTIONS]

Options:
  --all     Run all tests (default)
  --socks   Run only SOCKS5 proxy tests
  --api     Run only API tests (requires k6)
  --quick   Run quick version (reduced duration)
```

**Features:**
- Automatically starts echo server
- Automatically starts RustSocks proxy
- Captures logs to `loadtests/results/`
- Generates summary report
- Cleans up processes on exit

---

## Performance Targets

### Latency Targets

| Component | Target | Acceptable | Poor |
|-----------|--------|------------|------|
| Pure SOCKS5 | <5ms | <10ms | >10ms |
| Full Pipeline | <50ms | <100ms | >100ms |
| ACL Evaluation | <3ms | <5ms | >5ms |
| Session Tracking | <1ms | <2ms | >2ms |
| Pool Reuse | <2ms | <5ms | >5ms |

### Throughput Targets

| Test | Target | Acceptable | Poor |
|------|--------|------------|------|
| Handshake-Only | >5000 conn/s | >1000 conn/s | <1000 conn/s |
| Full Pipeline | >1000 conn/s | >500 conn/s | <500 conn/s |
| Data Transfer | >100 MB/s | >50 MB/s | <50 MB/s |
| DB Writes | >1000 sess/s | >500 sess/s | <500 sess/s |

### Stability Targets

| Test | Target | Acceptable | Poor |
|------|--------|------------|------|
| Concurrent 1000 | 100% success | >99% | <99% |
| Concurrent 5000 | 100% success | >98% | <98% |
| Long-Lived | 0% drops | <1% drops | >1% drops |

---

## Interpreting Results

### Understanding Metrics

#### Connection Statistics
- **Total Connections:** Number of connection attempts
- **Successful:** Connections that completed successfully
- **Failed:** Connections that failed or timed out
- **Throughput:** Successful connections per second

#### Latency Statistics
**IMPORTANT:** Latency measures the complete SOCKS5 handshake time, including:
1. TCP connect to proxy
2. SOCKS5 method negotiation (1 RTT)
3. Authentication (if enabled, 1 RTT)
4. SOCKS5 CONNECT request (1 RTT)
5. ACL evaluation (if enabled)
6. QoS checks (if enabled)
7. Session creation (if enabled)
8. Upstream TCP connect
9. SOCKS5 response to client

- **Average:** Mean handshake time across all connections
- **Minimum:** Fastest handshake (best case)
- **Maximum:** Slowest handshake (worst case)

**Latency does NOT include:**
- Data transfer time
- Database write time (async batched)
- Metrics collection (async)

#### Data Transfer
- **Bytes Sent:** Total bytes sent from client to server
- **Bytes Received:** Total bytes received from server
- **Total Transfer:** Sum of sent + received

### Comparing Results

#### Feature Overhead Calculation
```
Feature Overhead = (Full Pipeline Latency) - (Minimal Pipeline Latency)

Example:
- Minimal Pipeline: 2.5ms
- Full Pipeline: 2.8ms
- Feature Overhead: 0.3ms (ACL + Sessions + QoS)
```

#### Pool Effectiveness
```
Pool Improvement = ((Full Pipeline Latency) - (Pool Latency)) / (Full Pipeline Latency)

Example:
- Full Pipeline: 2.5ms
- Pool Efficiency: 1.3ms
- Pool Improvement: 48% faster
```

#### Bandwidth Calculation
```
Bandwidth = (Total Transfer MB) / (Test Duration seconds)

Example:
- Total Transfer: 200 MB
- Duration: 16.58 seconds
- Bandwidth: 12.06 MB/s
```

---

## Configuration Requirements

### Minimal Configuration

For baseline tests (`minimal-pipeline`, `handshake-only`):

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 10000

[auth]
client_method = "none"
socks_method = "none"

[acl]
enabled = false

[sessions]
enabled = false

[qos]
enabled = false
```

### Full Production Configuration

For production tests (`full-pipeline`, `acl-evaluation`, `session-churn`):

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 10000

[auth]
client_method = "none"
socks_method = "none"

[acl]
enabled = true
config_file = "config/acl.toml"
watch = true
anonymous_user = "anonymous"

[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite://sessions.db"
batch_size = 500
batch_interval_ms = 1000
traffic_update_packet_interval = 10

[server.pool]
enabled = true
max_idle_per_dest = 8
max_total_idle = 200
idle_timeout_secs = 120
connect_timeout_ms = 3000

[qos]
enabled = true
algorithm = "htb"

[qos.htb]
global_bandwidth_bytes_per_sec = 125000000
guaranteed_bandwidth_bytes_per_sec = 131072
max_bandwidth_bytes_per_sec = 12500000
burst_size_bytes = 1048576
refill_interval_ms = 50

[metrics]
enabled = true
storage = "sqlite"
```

### System Requirements

```bash
# File descriptor limits
ulimit -n 65536

# Network tuning (optional, for high-concurrency tests)
sudo sysctl -w net.core.somaxconn=4096
sudo sysctl -w net.ipv4.ip_local_port_range="1024 65535"
sudo sysctl -w net.ipv4.tcp_tw_reuse=1
```

---

## Troubleshooting

### Common Issues

#### 1. "Connection refused" Errors

**Symptoms:** High failure rate, "Connection refused" in logs

**Causes:**
- Proxy not running
- Wrong port configuration
- Firewall blocking connections

**Solution:**
```bash
# Check if proxy is running
lsof -i :1080

# Check if echo server is running
lsof -i :9999

# Restart services
./target/release/rustsocks --config config/rustsocks.toml &
./target/release/examples/echo_server --port 9999 &
```

---

#### 2. "Too many open files" Errors

**Symptoms:** Tests fail at high connection counts

**Causes:**
- File descriptor limit too low

**Solution:**
```bash
# Check current limit
ulimit -n

# Increase limit (temporary)
ulimit -n 65536

# Increase limit (permanent)
# Add to /etc/security/limits.conf:
* soft nofile 65536
* hard nofile 65536
```

---

#### 3. Low Throughput

**Symptoms:** Throughput much lower than expected

**Causes:**
- Resource exhaustion (CPU, memory, network)
- Connection limits reached
- QoS limiting enabled

**Solution:**
```bash
# Check resource usage
top
htop

# Check connection limits in config
grep max_connections config/rustsocks.toml

# Check QoS settings
grep qos config/rustsocks.toml

# Disable QoS for testing
# config: qos.enabled = false
```

---

#### 4. High Latency

**Symptoms:** Latency >100ms in minimal pipeline test

**Causes:**
- Network issues (especially in WSL2)
- Slow DNS resolution
- System resource contention

**Solution:**
```bash
# Test localhost latency
ping 127.0.0.1

# Test TCP loopback
nc -l 9999 &
time nc 127.0.0.1 9999

# Check system load
uptime
```

---

#### 5. Connection Pool Not Working

**Symptoms:** `pool-efficiency` latency same as `full-pipeline`

**Causes:**
- Pool disabled in config
- Idle timeout too short
- Pool size too small

**Solution:**
```toml
[server.pool]
enabled = true  # Must be true
max_idle_per_dest = 8  # Increase if needed
max_total_idle = 200
idle_timeout_secs = 120  # Increase if connections expire too quickly
```

---

#### 6. Database Write Bottleneck

**Symptoms:** `session-churn` throughput <500 sessions/sec

**Causes:**
- Small batch size
- Slow disk I/O
- SQLite lock contention

**Solution:**
```toml
[sessions]
batch_size = 1000  # Increase for better throughput
batch_interval_ms = 1000  # Decrease for more frequent writes

# Use WAL mode for better concurrency
# Add to SQLite connection:
# PRAGMA journal_mode=WAL;
```

---

## Advanced Usage

### Custom Test Scenarios

You can create custom test scenarios by modifying the load test tool:

1. Edit `examples/loadtest.rs`
2. Add new scenario to `Scenario` enum
3. Implement test function
4. Add to main match statement
5. Rebuild: `cargo build --release --example loadtest`

### Profiling Performance

```bash
# Install profiling tools
cargo install flamegraph

# Generate flamegraph
sudo cargo flamegraph --example loadtest -- \
    --scenario full-pipeline --duration 60

# View flamegraph.svg in browser
```

### Continuous Performance Monitoring

```bash
# Run tests periodically
while true; do
    ./target/release/examples/loadtest \
        --scenario full-pipeline \
        --duration 60 \
        --output "results_$(date +%s).json"
    sleep 300  # Wait 5 minutes
done
```

---

## Summary Table

| Scenario | Purpose | Target | Config | Duration |
|----------|---------|--------|--------|----------|
| minimal-pipeline | Baseline SOCKS5 | <10ms | Features OFF | 30s |
| full-pipeline | Production config | <100ms | Features ON | 30s |
| handshake-only | Connection throughput | >1000/s | Any | 30s |
| data-transfer | Bandwidth | >100MB/s | Any | 30s |
| large-transfer | Large files | >100MB/s | Any | 30s |
| acl-evaluation | ACL performance | <5ms | ACL ON | 30s |
| pool-efficiency | Pool effectiveness | 30-50% faster | Pool ON | 30s |
| session-churn | DB throughput | >1000/s | Sessions ON | 30s |
| long-lived | Stability | 0% drops | Any | 60s+ |
| concurrent-1000 | Concurrency | 100% success | Any | Auto |
| concurrent-5000 | High concurrency | 100% success | Any | Auto |
| auth-overhead | Auth cost | 1-2ms | Manual | 30s |
| qos-limiting | Rate limiting | Config limit | Manual | 30s |
| dns-resolution | DNS speed | <50ms | Manual | 30s |
| metrics-overhead | Metrics cost | <5% | Manual | 30s |

---

## Support

For issues or questions:
- Check GitHub issues: https://github.com/xulek/RustSocks/issues
- Review CLAUDE.md for architecture details
- Review source code: `examples/loadtest.rs`

---

**Last Updated:** 2025-11-05
**Version:** RustSocks v0.9.0
