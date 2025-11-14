# RustSocks Load Testing Suite

This directory contains comprehensive load testing tools for the RustSocks SOCKS5 proxy server.

## Overview

The load testing suite includes:

1. **SOCKS5 Proxy Load Tests** - Custom Rust-based load testing tool
2. **REST API Load Tests** - k6-based HTTP API testing
3. **Automated Test Runner** - Bash script to orchestrate all tests
4. **Echo Server** - Simple TCP echo server for testing data transfer

## Directory Structure

```
loadtests/
├── k6/                          # k6 test scripts
│   └── api_load_test.js        # REST API load test
├── scripts/                     # Helper scripts (reserved)
├── results/                     # Test results and logs
├── run_loadtests.sh            # Main test runner script
└── README.md                   # This file
```

## Prerequisites

### Required

- **Rust** - For building the load test tool and echo server
- **RustSocks** - Built in release mode (`cargo build --release`)

### Optional

- **k6** - For API load testing ([installation guide](https://k6.io/docs/get-started/installation/))
  - macOS: `brew install k6`
  - Linux: `sudo apt-get install k6` or download from k6.io
  - Windows: `choco install k6` or download from k6.io

## Test Scenarios

### SOCKS5 Proxy Tests

Minimal-profile scenarios automatically launch RustSocks with
`config/rustsocks_minimal.toml`, which disables ACL, sessions, QoS, metrics,
and connection pooling while setting logging to `warn`. Copy that file and
point the runner at your own profile if you need additional features.

#### 1. Concurrent Connections Test (1000)
- **Purpose**: Test proxy stability under 1000 concurrent connections
- **Metrics**: Success rate, throughput, latency
- **Target**: >99% success rate, <25ms average handshake

#### 2. Concurrent Connections Test (5000)
- **Purpose**: Stress test with 5000 concurrent connections
- **Metrics**: Success rate, throughput, latency
- **Target**: >98% success rate, <45ms average handshake

#### 3. ACL Performance Test
- **Purpose**: Measure ACL evaluation overhead under sustained load
- **Duration**: 30 seconds (10s in quick mode)
- **Workers**: 100 concurrent workers
- **Target**: <5ms ACL evaluation time

#### 4. Session Tracking Overhead
- **Purpose**: Measure session tracking and traffic metering overhead
- **Duration**: 30 seconds (10s in quick mode)
- **Workers**: 50 concurrent workers
- **Target**: <2ms session tracking overhead

#### 5. Database Write Throughput
- **Purpose**: Test SQLite database write performance with high session churn
- **Duration**: 30 seconds (10s in quick mode)
- **Workers**: 200 concurrent workers
- **Target**: >1100 sessions/second write throughput

### REST API Tests

#### HTTP API Load Test
- **Purpose**: Test REST API endpoints under load
- **Tool**: k6
- **Stages**:
  1. Ramp up to 10 users (10s)
  2. Ramp up to 50 users (30s)
  3. Spike to 100 users (20s)
  4. Scale down to 50 users (30s)
  5. Ramp down to 0 (10s)
- **Endpoints Tested**:
  - `GET /health`
  - `GET /api/sessions/active`
  - `GET /api/sessions/history`
  - `GET /api/sessions/stats`
  - `GET /metrics`
  - `GET /api/acl/rules`
- **Targets**:
  - p95 response time: <500ms
  - p99 response time: <1000ms
  - Error rate: <5%

## Quick Start

### 1. Build the Load Testing Tools

```bash
# Build release binaries
cargo build --release --example loadtest --example echo_server

# Or let the runner script build them
./loadtests/run_loadtests.sh --all
```

### 2. Run All Tests

```bash
# Full test suite (may take 5-10 minutes)
./loadtests/run_loadtests.sh --all

# Quick test suite (reduced duration, ~2 minutes)
./loadtests/run_loadtests.sh --all --quick
```

### 3. Run Specific Test Types

```bash
# SOCKS5 proxy tests only
./loadtests/run_loadtests.sh --socks

# API tests only (requires k6)
./loadtests/run_loadtests.sh --api

# Quick SOCKS tests
./loadtests/run_loadtests.sh --socks --quick
```

## Manual Testing

### SOCKS5 Load Test Tool

```bash
# Build the load test tool
cargo build --release --example loadtest

# Run specific scenario
./target/release/examples/loadtest \
  --scenario concurrent-1000 \
  --proxy 127.0.0.1:1080 \
  --upstream 127.0.0.1:9999

# Available scenarios:
#   concurrent-1000   - 1000 concurrent connections
#   concurrent-5000   - 5000 concurrent connections
#   acl-perf         - ACL performance test
#   session-overhead - Session tracking overhead
#   db-throughput    - Database write throughput
#   all              - Run all scenarios

# With authentication
./target/release/examples/loadtest \
  --scenario concurrent-1000 \
  --proxy 127.0.0.1:1080 \
  --upstream 127.0.0.1:9999 \
  --username alice \
  --password secret123

# Custom duration for sustained tests
./target/release/examples/loadtest \
  --scenario acl-perf \
  --proxy 127.0.0.1:1080 \
  --upstream 127.0.0.1:9999 \
  --duration 60  # 60 seconds
```

### Echo Server

```bash
# Start echo server
cargo run --release --example echo_server -- --port 9999

# Custom bind address
cargo run --release --example echo_server -- --bind 0.0.0.0 --port 9999
```

### k6 API Tests

```bash
# Run API load test with k6
export API_URL=http://127.0.0.1:9090
k6 run --vus 50 --duration 30s loadtests/k6/api_load_test.js

# Save results to JSON
k6 run --out json=results/api_test.json loadtests/k6/api_load_test.js

# Custom stages
k6 run \
  --stage 10s:10 \
  --stage 30s:50 \
  --stage 10s:0 \
  loadtests/k6/api_load_test.js
```

## Test Results

Test results are saved to `loadtests/results/` with timestamps:

```
results/
├── concurrent_1000_20251026_120000.log
├── acl_perf_20251026_120230.log
├── session_overhead_20251026_120530.log
├── db_throughput_20251026_120830.log
├── concurrent_5000_20251026_121130.log
├── k6_api_20251026_121530.json
├── k6_api_20251026_121530.log
├── echo_server_20251026_120000.log
└── rustsocks_20251026_120000.log
```

### Analyzing Results

#### SOCKS5 Test Results

Each SOCKS5 test log contains:
- Total connections
- Success/failure counts and rates
- Connection throughput (conn/s)
- Latency statistics (min/avg/max)
- Data transfer statistics

Example:
```
================================================================================
📊 Load Test Results: 1000 Concurrent Connections
================================================================================

⏱️  Test Duration: 10.23s

📈 Connection Statistics:
  Total Connections:      1000
  ✅ Successful:          982 (98.20%)
  ❌ Failed:              18
  🔄 Throughput:          97.8 conn/s

⚡ Latency Statistics (SOCKS5 handshake):
  Average:                5.23 ms
  Minimum:                2.45 ms
  Maximum:                12.67 ms
================================================================================
```

After all requested scenarios complete, the runner prints a compact summary
table that compares every captured metric with its expected target:

```
Metric                              | Actual           | Target            | Status | Note
--------------------------------------------------------------------------------------------
Minimal Pipeline Latency            | 35.82 ms         | <= 40.00 ms       | PASS   | Avg SOCKS5 handshake
Handshake-Only Throughput           | 1381.22 conn/s   | >= 1200.00 conn/s | PASS   | Connections per second
Data Transfer Bandwidth             | 952.10 MB/s      | >= 500.00 MB/s    | PASS   | Aggregate bandwidth
Session Churn Throughput            | 1133.16 conn/s   | >= 1100.00 conn/s | PASS   | Sessions per second
Concurrent 5000 Success             | 100.00 %         | >= 98.00 %        | PASS   | Successful connections (%)
```

This makes it easy to spot regressions without digging into individual log
files.

#### k6 API Results

k6 generates detailed statistics including:
- Request rate and count
- Response time percentiles (p50, p90, p95, p99)
- Error rates
- HTTP status code distribution

Results are saved in JSON format for further analysis.

## Performance Targets

### SOCKS5 Proxy

| Metric | Target | Description |
|--------|--------|-------------|
| 1000 concurrent connections | >99% success | Connection success rate |
| 5000 concurrent connections | >98% success | Stress test success rate |
| Handshake latency | <40ms avg | SOCKS5 handshake time |
| ACL evaluation | <5ms | ACL decision time |
| Session tracking | <5ms | Session update overhead |
| DB write throughput | >1100/s | Sessions written to DB |

### REST API

| Metric | Target | Description |
|--------|--------|-------------|
| p95 response time | <500ms | 95th percentile latency |
| p99 response time | <1000ms | 99th percentile latency |
| Error rate | <5% | Failed request percentage |
| Throughput | >100 req/s | Requests per second |

## Configuration

### RustSocks Configuration

Before running load tests, ensure your `config/rustsocks.toml` has appropriate settings:

```toml
[server]
max_connections = 10000  # Must be >= test connection count

[auth]
method = "none"  # IMPORTANT: Use "none" for load testing (no authentication)
                 # For authenticated tests, use "userpass" and provide --username/--password

[sessions]
enabled = true
traffic_update_packet_interval = 10  # Reduce for more frequent updates

[qos]
max_connections_global = 10000

[qos.connection_limits]
max_connections_per_user = 10000  # IMPORTANT: Must be >= test connection count
max_connections_global = 10000
```

**IMPORTANT:**
- Load tests require `auth.method = "none"` in the configuration
- Set `max_connections_per_user = 10000` (or disable QoS) to allow high concurrent connections
- If you want to test with authentication, change the config to `method = "userpass"` and use the `--username` and `--password` flags with the load test tool

### System Tuning

For high connection tests (5000+), you may need to increase system limits:

```bash
# Increase file descriptor limit (Linux/macOS)
ulimit -n 65535

# Check current limit
ulimit -n

# Permanent change (add to /etc/security/limits.conf)
* soft nofile 65535
* hard nofile 65535
```

## Troubleshooting

### "Too many open files" error

Increase file descriptor limit:
```bash
ulimit -n 65535
```

### Proxy fails to start

Check if port is already in use:
```bash
lsof -i :1080
netstat -tuln | grep 1080
```

### Low success rate

- Check system resources (CPU, memory)
- Reduce concurrent connection count
- Increase batch size in test
- Check proxy logs for errors

### k6 not found

Install k6:
```bash
# macOS
brew install k6

# Ubuntu/Debian
sudo apt-get install k6

# Or download from https://k6.io/docs/get-started/installation/
```

### API tests fail

Ensure API is enabled in `config/rustsocks.toml`:
```toml
[sessions]
stats_api_enabled = true
stats_api_port = 9090
```

## Continuous Integration

### GitHub Actions Example

```yaml
name: Load Tests
on:
  push:
    branches: [ main ]
  pull_request:
    branches: [ main ]

jobs:
  loadtest:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Build
        run: cargo build --release
      - name: Run quick load tests
        run: ./loadtests/run_loadtests.sh --all --quick
      - name: Upload results
        uses: actions/upload-artifact@v3
        with:
          name: load-test-results
          path: loadtests/results/
```

## Advanced Usage

### Custom Scenarios

Create custom test scenarios by modifying `examples/loadtest.rs`:

```rust
// Add new scenario
async fn test_custom_scenario(args: &Args) -> std::io::Result<()> {
    // Your custom test logic here
}

// Add to scenario enum
enum Scenario {
    // ...
    Custom,
}
```

### Benchmark Regression

Compare results over time:

```bash
# Run baseline test
./loadtests/run_loadtests.sh --socks --quick
cp loadtests/results/* baseline/

# After changes, run test again
./loadtests/run_loadtests.sh --socks --quick

# Compare results
diff baseline/concurrent_1000_*.log loadtests/results/concurrent_1000_*.log
```

### Performance Profiling

Run load tests with profiling:

```bash
# With flamegraph
cargo flamegraph --example loadtest -- --scenario all

# With perf
perf record -F 99 -g ./target/release/examples/loadtest --scenario all
perf report
```

## Contributing

When adding new load tests:

1. Add test scenario to `examples/loadtest.rs`
2. Update `run_loadtests.sh` to include new test
3. Document expected metrics and targets
4. Update this README

## License

Same as RustSocks main project.
