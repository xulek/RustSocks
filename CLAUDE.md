# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustSocks is a high-performance SOCKS5 proxy server written in Rust, featuring advanced ACL (Access Control List) engine, session management with SQLite persistence, and Prometheus metrics integration.

**Current Status**: Production Ready - Sprint 4.1 Complete (v0.9.0)
- ✅ Core SOCKS5 (CONNECT, BIND, UDP ASSOCIATE)
- ✅ ACL Engine + Hot Reload
- ✅ Session Management + SQLite
- ✅ PAM Authentication + LDAP Groups + Active Directory
- ✅ QoS & Rate Limiting
- ✅ REST API + Web Dashboard
- ✅ SOCKS over TLS (with mTLS support)
- ✅ Connection Pooling & Optimization
- ✅ 287 Tests (273 passing, 14 ignored)

## Common Commands

### Build & Test

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run all tests
cargo test --all-features

# Run specific component tests
cargo test acl
cargo test pool
cargo test pam

# Linting (warnings as errors)
cargo clippy --all-features -- -D warnings

# Format check
cargo fmt --check
```

### Running the Server

```bash
# Run with defaults (127.0.0.1:1080, no auth)
./target/release/rustsocks

# Run with config file
./target/release/rustsocks --config config/rustsocks.toml

# Generate example config
./target/release/rustsocks --generate-config config/rustsocks.toml

# Set log level
./target/release/rustsocks --log-level debug
```

### Testing with Clients

```bash
# Test with curl
curl -x socks5://127.0.0.1:1080 http://example.com

# Test with authentication
curl -x socks5://alice:secret123@127.0.0.1:1080 http://example.com
```

### Load Testing

```bash
# Run all SOCKS5 load tests
bash loadtests/run_loadtests.sh --socks

# Quick tests (3-5 minutes)
bash loadtests/run_loadtests.sh --socks --quick
```

See [Load Testing Manual](loadtests/MANUAL.md) for details.

## Module Structure

Brief overview of key modules:

- **`protocol/`** - SOCKS5 protocol parsing and types
- **`auth/`** - Authentication (NoAuth, UserPass, PAM, LDAP groups)
- **`acl/`** - Access Control List engine with hot-reload
- **`session/`** - Session tracking and SQLite persistence
- **`server/`** - Server implementation (listener, handler, proxy, pool)
- **`config/`** - TOML-based configuration management

📖 **See [Architecture Documentation](docs/technical/architecture.md) for detailed module structure.**

## Configuration

### Feature Flags

```toml
[features]
default = ["metrics"]
metrics = []     # Prometheus metrics
database = []    # SQLite persistence
```

### Main Config

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 1000

[auth]
client_method = "none"       # or "pam.address"
socks_method = "none"        # or "userpass", "pam.username"

[acl]
enabled = false
config_file = "config/acl.toml"
watch = false  # Enable hot-reload

[sessions]
enabled = false
storage = "memory"  # or "sqlite"
stats_api_enabled = false
stats_api_port = 9090

[server.pool]
enabled = false  # Connection pooling
max_idle_per_dest = 4
idle_timeout_secs = 90
```

📖 **See example configs in `config/` and `config/examples/` for complete options.**

## Development Standards

### Code Quality

- **All code must pass**: `cargo clippy --all-features -- -D warnings`
- **Function parameters**: Max 7 parameters (use context structs if exceeded)
- **Modern Rust idioms**:
  - `io::Error::other(msg)` instead of `io::Error::new(io::ErrorKind::Other, msg)`
  - Implement standard traits (`Display`, `FromStr`) instead of custom methods
- **Feature gates**: Database code uses `#[cfg(feature = "database")]`

### Error Handling

- Custom error type: `RustSocksError` in `utils/error.rs`
- Uses `thiserror` for derive macros
- Errors logged via `tracing` framework

### Async & Concurrency

- Tokio runtime with "full" feature set
- `Arc<T>` for shared ownership
- `RwLock` for read-heavy workloads
- `DashMap` for concurrent access
- `Mutex` for batch writer queue

### Testing

- Use `#[tokio::test]` for async tests
- Feature-gate database tests: `#[cfg(feature = "database")]`
- Use `sqlite::memory:` for test databases
- Integration tests in `tests/`, unit tests in modules

📖 **See [Testing Guide](docs/guides/testing.md) for comprehensive testing documentation.**

## Documentation

### Technical Documentation

- **[Architecture Overview](docs/technical/architecture.md)** - Detailed module structure and request flow
- **[ACL Engine](docs/technical/acl-engine.md)** - Access control implementation
- **[PAM Authentication](docs/technical/pam-authentication.md)** - PAM integration details
- **[Protocol Implementation](docs/technical/protocol.md)** - UDP ASSOCIATE, BIND, TLS
- **[Connection Pool](docs/technical/connection-pool.md)** - Connection pooling details
- **[Session Management](docs/technical/session-management.md)** - Session tracking and persistence

### User Guides

- **[Active Directory Integration](docs/guides/active-directory.md)** - Complete AD/LDAP setup (1300+ lines)
- **[LDAP Groups](docs/guides/ldap-groups.md)** - Group-based ACL rules
- **[Web Dashboard](docs/guides/web-dashboard.md)** - Dashboard setup and usage
- **[Building with Base Path](docs/guides/building-with-base-path.md)** - Custom URL paths
- **[Testing Guide](docs/guides/testing.md)** - Comprehensive testing documentation
- **[Load Testing Manual](loadtests/MANUAL.md)** - Performance testing (1200+ lines)

### Configuration Examples

All in `config/examples/`:
- `sssd-ad.conf` - SSSD configuration for AD
- `krb5-ad.conf` - Kerberos configuration
- `acl-ad-example.toml` - ACL rules with AD groups
- `rustsocks-ad.toml` - Complete RustSocks config for AD

## Key Features

### Authentication

- **NoAuth** (0x00) - No authentication
- **Username/Password** (0x02, RFC 1929) - Simple auth
- **PAM Authentication**:
  - `pam.address` - IP-based authentication
  - `pam.username` - Username/password via PAM
- **Active Directory** - Via SSSD/PAM integration
- **LDAP Groups** - Group-based ACL rules

📖 **See [PAM Authentication](docs/technical/pam-authentication.md) and [Active Directory](docs/guides/active-directory.md)**

### ACL Engine

- Priority-based rule evaluation (BLOCK before ALLOW)
- Pattern matching: IP, CIDR, domains, wildcards, ports
- Group inheritance (users inherit group rules)
- Hot-reload support (zero-downtime updates)
- Per-user statistics

📖 **See [ACL Engine Documentation](docs/technical/acl-engine.md)**

### Protocol Support

- **CONNECT** - Standard TCP connection (RFC 1928)
- **BIND** - Reverse connections (RFC 1928)
- **UDP ASSOCIATE** - UDP relay (RFC 1928)
- **SOCKS over TLS** - Encryption with mTLS support

📖 **See [Protocol Documentation](docs/technical/protocol.md)**

### Session Management

- In-memory tracking with DashMap
- SQLite persistence (optional)
- Batch writing for performance
- Prometheus metrics
- REST API for queries
- Web dashboard

📖 **See [Session Management](docs/technical/session-management.md)**

### Connection Pool

- Reuse upstream connections
- Per-destination and global limits
- Configurable timeouts
- Background cleanup
- 100% test coverage

📖 **See [Connection Pool](docs/technical/connection-pool.md)**

### Web Dashboard

- Real-time session monitoring
- ACL rule viewing
- User management
- Statistics and analytics
- Swagger API documentation

📖 **See [Web Dashboard Guide](docs/guides/web-dashboard.md)**

## Database Operations

```bash
# Migrations run automatically on startup
# Manual migration test:
sqlx migrate run --database-url sqlite://sessions.db

# Query sessions
sqlite3 sessions.db "SELECT user, dest_ip, dest_port, bytes_sent, bytes_received FROM sessions WHERE status = 'active'"

# Top users by traffic
sqlite3 sessions.db "SELECT user, SUM(bytes_sent + bytes_received) as total_bytes, COUNT(*) as sessions FROM sessions GROUP BY user ORDER BY total_bytes DESC LIMIT 10"
```

📖 **See [Session Management](docs/technical/session-management.md) for schema and queries.**

## Quality Metrics

**Latest** (2025-11-01):
- **Tests**: 287 total (273 passing, 14 ignored) ✅
  - Unit: 97 tests
  - Integration: 180 tests
  - E2E: 10 tests
- **Code Quality**: Zero clippy warnings ✅
- **Performance**: All targets exceeded ✅
  - Latency: <5ms avg
  - Pool: 7,000 ops/sec @ 200 threads
  - ACL: 1.92ms avg
  - Session: 1.01ms overhead
  - DB writes: 12,279/s

### Test Coverage by Component

| Component | Tests | Coverage |
|-----------|-------|----------|
| ACL Engine | 134 | >90% |
| Authentication | 31 | >85% |
| QoS | 36 | >90% |
| Connection Pool | 31 | 100% |
| Session Manager | - | >85% |
| Protocol | 12 | >85% |
| E2E Scenarios | 10 | 100% |

📖 **See [Testing Guide](docs/guides/testing.md) for detailed test documentation.**

## Quick Reference

### Essential Files

- `CLAUDE.md` - This file (you're here!)
- `README.md` - User-facing documentation
- `Cargo.toml` - Dependencies and features
- `config/rustsocks.toml` - Main configuration
- `config/acl.toml` - ACL rules

### Essential Commands

```bash
# Build and test
cargo build --release
cargo test --all-features
cargo clippy --all-features -- -D warnings

# Run server
./target/release/rustsocks --config config/rustsocks.toml

# Load test
bash loadtests/run_loadtests.sh --socks --quick

# Security audit
cargo audit
```

### Getting Help

- Check documentation in `docs/`
- Review tests in `tests/` for examples
- See configuration examples in `config/examples/`
- Load testing: `loadtests/MANUAL.md`
- Active Directory setup: `docs/guides/active-directory.md`

## Roadmap

- ✅ **Sprint 1-4.1**: Core features complete (v0.9.0)
- 🔄 **Sprint 4+**: Production packaging, systemd, Grafana dashboards

---

**Remember**: Detailed documentation has been moved to `docs/` directory. Always check there for comprehensive information before making changes.
