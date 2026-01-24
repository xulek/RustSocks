# Testing Guide

This document covers the testing strategy, test organization, and guidelines for RustSocks.

## Overview

RustSocks has comprehensive test coverage across:
- Unit tests (in module files)
- Integration tests (`tests/` directory)
- E2E tests (complete flows)
- Performance tests (stress tests, ignored by default)

## Test Organization

### Unit Tests

Location: Inline in source files (`src/**/*.rs`)

Run with:
```bash
cargo test --lib
cargo test --lib --all-features
```

Coverage:
- ACL matchers and engine logic
- QoS rate limiting algorithms
- Connection pool operations
- Protocol parsing
- Configuration validation

### Integration Tests

Location: `tests/` directory

Run with:
```bash
cargo test --test '*'
cargo test --test acl_integration
```

Tests include:
- `acl_integration.rs` - ACL enforcement
- `acl_api.rs` - ACL API endpoints
- `acl_unit.rs` - ACL matchers/rules
- `bind_command.rs` - BIND command
- `connection_pool.rs` - Pool basics
- `pool_edge_cases.rs` - Pool edge cases
- `pool_socks_integration.rs` - Pool with SOCKS5
- `pool_concurrency.rs` - Stress tests
- `pool_system_verification.rs` - Pool platform checks
- `e2e_tests.rs` - Complete flows
- `ipv6_domain.rs` - IPv6/domain support
- `ldap_groups.rs` - LDAP groups
- `pam_integration.rs` - PAM auth
- `session_tracking.rs` - Session lifecycle
- `session_manager_edge_cases.rs` - Session manager robustness
- `resolver_edge_cases.rs` - DNS resolver coverage
- `protocol_edge_cases.rs` - Protocol parsing
- `qos_unit.rs` - QoS algorithms
- `qos_integration.rs` - QoS integration
- `udp_associate.rs` - UDP relay coverage
- `tls_support.rs` - TLS/mTLS

### E2E Tests

Location: `tests/e2e_tests.rs`

Complete end-to-end flows (refer to `tests/e2e_tests.rs` for the current list):
1. `e2e_basic_connect` - Basic SOCKS5 CONNECT
2. `e2e_auth_noauth` - NoAuth flow
3. `e2e_auth_userpass` - Username/password auth
4. `e2e_auth_userpass_invalid` - Auth rejection
5. `e2e_acl_allow` - ACL allows connection
6. `e2e_acl_block` - ACL blocks connection
7. `e2e_session_tracking` - Session lifecycle
8. `e2e_udp_associate` - UDP ASSOCIATE
9. `e2e_bind_command` - BIND command
10. `e2e_complete_flow` - Auth + ACL + Session + Data

Run with:
```bash
cargo test --all-features --test e2e_tests
cargo test --all-features e2e_basic_connect
```

## Running Tests

### Quick Commands

```bash
# All tests (default features)
cargo test

# All tests with all features
cargo test --all-features

# Specific component
cargo test acl
cargo test pool
cargo test pam

# With output
cargo test -- --nocapture

# Specific test
cargo test test_name

# Integration tests only
cargo test --test '*'

# Ignored tests (performance)
cargo test --release -- --ignored
```

### Feature-Specific Tests

```bash
# Database tests
cargo test --features database

# Metrics tests
cargo test --features metrics

# All features
cargo test --all-features
```

### Performance Tests

Performance tests are ignored by default (too slow for CI):

```bash
# Run all performance tests
cargo test --release -- --ignored --nocapture

# Specific performance test
cargo test --release acl_performance_under_seven_ms -- --ignored --nocapture

# Pool stress test
cargo test --release --test pool_concurrency -- --ignored --nocapture
```

**Note**: Always use `--release` for performance tests (10x faster).

## Test Categories

### By Component

**Note**: Counts vary over time. Run `cargo test -- --list` to see current totals.

| Component | Representative tests |
|-----------|----------------------|
| ACL (engine + API) | `acl_unit.rs`, `acl_integration.rs`, `acl_api.rs` |
| Authentication / LDAP | `pam_integration.rs`, `ldap_groups.rs` (several ignored without PAM/SSSD) |
| QoS / Rate Limiting | `qos_unit.rs`, `qos_integration.rs` |
| Connection Pool | `connection_pool.rs`, `pool_edge_cases.rs`, `pool_socks_integration.rs`, `pool_system_verification.rs`, `pool_concurrency.rs` |
| Protocol & Transport | `protocol_edge_cases.rs`, `bind_command.rs`, `udp_associate.rs`, `tls_support.rs`, `ipv6_domain.rs` |
| Resolver & Session Mgmt | `resolver_edge_cases.rs`, `session_manager_edge_cases.rs`, `session_tracking.rs` |
| API / Monitoring | `api_endpoints.rs`, `e2e_tests.rs` (API-focused flows) |
| Misc / System | Remaining integration helpers, QoS metrics hooks, etc. |
| Inline unit tests (`src/**`) | Spread across modules for config, ACL, session, QoS, etc. |

### By Type

- **Unit tests (inline)**: `src/**`
- **Integration + system tests**: `tests/**`

### Coverage

Coverage varies by feature set and environment. Use your preferred coverage tooling in CI to track current numbers.

## Test Guidelines

### General Principles

1. **Use async tests**: `#[tokio::test]` for async code
2. **Feature gates**: Use `#[cfg(feature = "...")]` for optional features
3. **In-memory databases**: Use `sqlite::memory:` for database tests
4. **Cleanup**: Always clean up resources (files, connections)
5. **Isolation**: Tests should not depend on each other
6. **Deterministic**: Avoid flaky tests (timeouts, race conditions)

### Writing Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // Arrange
        let input = create_test_input();

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_async_function() {
        let result = async_function().await;
        assert!(result.is_ok());
    }
}
```

### Writing Integration Tests

```rust
// tests/my_integration_test.rs

use rustsocks::*;

#[tokio::test]
async fn test_integration() {
    // Setup server
    let server = spawn_test_server().await;

    // Create client
    let client = create_test_client().await;

    // Test interaction
    let result = client.connect("example.com", 80).await;
    assert!(result.is_ok());

    // Cleanup
    server.shutdown().await;
}
```

### E2E Test Helpers

Use helper functions from `tests/e2e_tests.rs`:

```rust
// Create test server
let context = create_basic_server_context(config);
let server = spawn_socks_server(context).await;

// Echo server for data tests
let echo = spawn_echo_server(9999).await;

// SOCKS5 handshake
let stream = TcpStream::connect("127.0.0.1:1080").await?;
let mut stream = socks5_handshake_noauth(stream).await?;

// SOCKS5 CONNECT
socks5_connect(&mut stream, "127.0.0.1", 9999).await?;
```

## Testing Specific Features

### Testing ACL

```bash
# All ACL tests
cargo test acl

# ACL matchers only
cargo test --lib acl::matcher

# ACL integration
cargo test --test acl_integration

# ACL API
cargo test --test acl_api
```

### Testing Authentication

```bash
# PAM tests (requires PAM setup)
cargo test pam

# PAM integration (ignored, requires root)
cargo test --all-features pam -- --ignored

# LDAP groups
cargo test --all-features ldap_groups
```

### Testing Connection Pool

```bash
# All pool tests
cargo test --all-features pool

# Basic tests
cargo test --all-features --test connection_pool

# Edge cases
cargo test --all-features --test pool_edge_cases

# SOCKS integration
cargo test --all-features --test pool_socks_integration

# Stress tests (ignored)
cargo test --release --test pool_concurrency -- --ignored --nocapture
```

### Testing Session Management

```bash
# Session tests (memory only)
cargo test session

# With database
cargo test --features database session

# Session tracking integration
cargo test --all-features --test session_tracking
```

### Testing Protocol Commands

```bash
# UDP ASSOCIATE
cargo test --all-features udp

# BIND command
cargo test --all-features bind

# TLS support
cargo test --all-features tls_support
```

## Load Testing

See [Load Testing Manual](../references/loadtests-manual.md) for comprehensive load testing.

### Quick Load Tests

```bash
# Run all SOCKS5 tests
bash loadtests/run_loadtests.sh --socks

# Quick tests (3-5 minutes)
bash loadtests/run_loadtests.sh --socks --quick

# API tests (requires k6)
bash loadtests/run_loadtests.sh --api
```

### Manual Load Testing

```bash
# Build binaries
cargo build --release --example loadtest --example echo_server

# Start echo server
./target/release/examples/echo_server --port 9999 &

# Start proxy
./target/release/rustsocks --config config/rustsocks.toml &

# Run load test
./target/release/examples/loadtest \
    --scenario full-pipeline \
    --proxy 127.0.0.1:1080 \
    --upstream 127.0.0.1:9999 \
    --duration 30
```

## Continuous Integration

### Pre-commit Checks

```bash
# Format check
cargo fmt --check

# Linting (warnings as errors)
cargo clippy --all-features -- -D warnings

# Tests
cargo test --all-features

# Security audit
cargo audit
```

### CI Pipeline

Typical CI pipeline:
1. **Format**: `cargo fmt --check`
2. **Lint**: `cargo clippy --all-features -- -D warnings`
3. **Build**: `cargo build --all-features`
4. **Test**: `cargo test --all-features`
5. **Audit**: `cargo audit` (allow-list known issues)

## Debugging Tests

### Enable Logging

```bash
# Set log level
RUST_LOG=debug cargo test -- --nocapture

# Specific module
RUST_LOG=rustsocks::acl=trace cargo test acl -- --nocapture
```

### Run Single Test

```bash
# With output
cargo test test_name -- --nocapture

# With logging
RUST_LOG=debug cargo test test_name -- --nocapture
```

### Debug Test Binary

```bash
# Build test binary
cargo test --no-run

# Find binary in target/debug/deps/
# Run with debugger
gdb target/debug/deps/rustsocks-<hash>
```

## Test Data

### Configuration Files

Test configs in `config/`:
- `rustsocks.toml` - Main config
- `acl.toml` - ACL rules
- `examples/` - Example configs

### Mock Data

Use builders for test data:

```rust
// Create test ACL config
let config = AclConfigBuilder::default()
    .default_policy("block")
    .user("alice", vec!["developers"])
    .allow_rule("*.example.com", vec!["80", "443"])
    .build();

// Create test session
let session = SessionBuilder::default()
    .user("alice")
    .source("192.168.1.100", 12345)
    .destination("example.com", 443)
    .protocol(Protocol::Tcp)
    .build();
```

## Performance Testing

### Benchmarks

Use Criterion for micro-benchmarks (not implemented yet):

```bash
cargo bench
```

### Profiling

Use flamegraph for profiling:

```bash
cargo install flamegraph
cargo flamegraph --test test_name
```

### Memory Profiling

Use valgrind or heaptrack:

```bash
# Valgrind
valgrind --tool=massif target/debug/rustsocks

# Heaptrack
heaptrack target/debug/rustsocks
heaptrack_gui heaptrack.rustsocks.<pid>.gz
```

## Troubleshooting

### Tests Hanging

**Causes**:
- Deadlock in async code
- Timeout not set
- Server not responding

**Solutions**:
- Add timeout: `tokio::time::timeout(Duration::from_secs(5), test).await`
- Check for deadlocks: `RUST_LOG=trace`
- Verify server is started before client

### Flaky Tests

**Causes**:
- Race conditions
- Timing-dependent assertions
- Port conflicts

**Solutions**:
- Use synchronization primitives
- Add retries for timing-dependent checks
- Use random ports or port selection

### Database Tests Failing

**Causes**:
- Database not initialized
- Migration not applied
- Connection not closed

**Solutions**:
- Use `sqlite::memory:` for isolation
- Apply migrations in test setup
- Close connections explicitly

## Related Documentation

- [Load Testing Manual](../references/loadtests-manual.md)
- [Architecture Overview](../technical/architecture.md)
- [Connection Pool](../technical/connection-pool.md)
- [Session Management](../technical/session-management.md)
