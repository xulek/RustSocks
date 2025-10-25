# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustSocks is a high-performance SOCKS5 proxy server written in Rust, featuring advanced ACL (Access Control List) engine, session management with SQLite persistence, and Prometheus metrics integration.

**Current Status**: MVP + ACL Engine + Session Manager complete (Sprint 2.1-2.3)

## Common Commands

### Build & Test

```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Run all tests (default feature set)
cargo test

# Run tests with database support
cargo test --features database

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Check for compilation errors without building
cargo check

# Run clippy for linting
cargo clippy
```

### Running the Server

```bash
# Run with defaults (127.0.0.1:1080, no auth)
./target/release/rustsocks

# Run with config file
./target/release/rustsocks --config config/rustsocks.toml

# Generate example config
./target/release/rustsocks --generate-config config/rustsocks.toml

# Override bind address/port
./target/release/rustsocks --bind 0.0.0.0 --port 1080

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

## Architecture

### Module Structure

The codebase follows a modular architecture organized by functionality:

- **`protocol/`** - SOCKS5 protocol parsing and types
  - `types.rs`: Core protocol structures (ClientGreeting, Socks5Request, etc.)
  - `parser.rs`: Async parsing logic for SOCKS5 messages
  - Supports IPv4, IPv6, and domain name addressing

- **`auth/`** - Authentication manager
  - Supports `NoAuth` (0x00) and `Username/Password` (0x02, RFC 1929)
  - Pluggable authentication methods

- **`acl/`** - Access Control List engine
  - `types.rs`: ACL data structures (Action, Protocol, matchers)
  - `matcher.rs`: Pattern matching logic (IP, CIDR, domain wildcards, ports)
  - `engine.rs`: Rule evaluation with priority ordering (BLOCK rules first)
  - `loader.rs`: TOML config parsing and validation
  - `watcher.rs`: Hot-reload support with zero-downtime updates
  - `stats.rs`: Per-user allow/block statistics

- **`session/`** - Session tracking and metrics
  - `types.rs`: Session data structures and filters
  - `manager.rs`: In-memory session tracking using DashMap
  - `store.rs`: SQLite persistence with sqlx (feature-gated: `database`)
  - `batch.rs`: Batch writer for efficient database writes (feature-gated: `database`)
  - `metrics.rs`: Prometheus metrics integration (feature-gated: `metrics`)

- **`server/`** - Server implementation
  - `listener.rs`: TCP listener setup
  - `handler.rs`: Connection handler orchestrating auth → ACL → connect → proxy
  - `proxy.rs`: Bidirectional data transfer with traffic tracking
  - `resolver.rs`: DNS resolution supporting IPv4/IPv6/domains
  - `stats.rs`: Statistics API (HTTP endpoint)

- **`config/`** - Configuration management
  - TOML-based configuration with validation
  - CLI argument overrides
  - Nested configs for server, auth, ACL, sessions

### Request Flow

1. **TCP Accept** (`listener.rs`)
   - Accept incoming connection
   - Spawn handler task

2. **SOCKS5 Handshake** (`handler.rs`)
   - Parse client greeting
   - Negotiate authentication method
   - Authenticate user (if required)

3. **ACL Evaluation** (`handler.rs` + `acl/engine.rs`)
   - Extract destination and protocol from SOCKS5 request
   - Evaluate per-user and per-group rules
   - BLOCK rules take priority over ALLOW rules
   - Log decision and update ACL statistics
   - If blocked: send `ConnectionNotAllowed`, track rejected session, close connection

4. **Connection Establishment** (`handler.rs` + `resolver.rs`)
   - Resolve destination (IPv4/IPv6/domain)
   - Connect to target server
   - Create session in SessionManager

5. **Data Proxying** (`proxy.rs`)
   - Bidirectional copy between client and target
   - Track traffic (bytes/packets sent/received)
   - Update session metrics periodically (configurable interval)
   - Final flush on connection close

6. **Session Lifecycle** (`session/manager.rs`)
   - `new_session()`: Create active session
   - `update_traffic()`: Increment traffic counters
   - `close_session()`: Mark completed, record duration
   - `track_rejected_session()`: Record ACL rejections

7. **Persistence** (`session/store.rs`, `session/batch.rs`)
   - Batch writer queues sessions
   - Auto-flush on batch_size or batch_interval_ms
   - Background cleanup task removes old records (retention_days)

### ACL Engine

**Key Design Principles:**
- **Priority-based evaluation**: BLOCK rules are checked before ALLOW rules
- **Group inheritance**: Users inherit rules from their groups
- **Thread-safe**: Uses `Arc<RwLock>` for concurrent access
- **Hot-reload capable**: `AclWatcher` atomically swaps config on file changes
- **Default policy**: Configurable allow/block for unmatched connections

**Rule Matching:**
- IP exact match (IPv4/IPv6)
- CIDR ranges (`10.0.0.0/8`, `2001:db8::/32`)
- Domain exact match (case-insensitive)
- Wildcard domains (`*.example.com`, `api.*.com`)
- Port ranges (`8000-9000`), multiple (`80,443,8080`), or any (`*`)
- Protocol filtering (TCP, UDP, Both)

**Evaluation Algorithm** (in `engine.rs`):
1. Collect all applicable rules (user rules + group rules)
2. Sort by priority (higher priority first, BLOCK action first)
3. Iterate rules until first match
4. Return decision (Allow/Block) and matched rule description
5. Fall back to `default_policy` if no rules match

### Session Manager

**In-Memory Storage:**
- Active sessions stored in `DashMap<String, Session>` (concurrent hashmap)
- Session snapshots (closed/rejected) in `RwLock<Vec<Session>>`
- Efficient lookups and updates without blocking

**Traffic Tracking:**
- Proxy loop calls `update_traffic()` every N packets (configurable)
- Reduces write amplification while maintaining accuracy
- Final flush ensures no data loss on connection close

**Statistics API:**
- `get_stats(window)` aggregates rolling window metrics
- Returns active count, total sessions/bytes, top users/destinations
- HTTP endpoint: `GET /stats?window_hours=48`

**Database Integration (feature: `database`):**
- SQLite backend via sqlx
- Async migrations in `migrations/` directory
- Batch writer for performance (configurable batch size and interval)
- Automatic cleanup of old records

### Metrics (feature: `metrics`)

Prometheus metrics exported via `prometheus` crate:
- `rustsocks_active_sessions` - Gauge of active sessions
- `rustsocks_sessions_total` - Counter of accepted sessions
- `rustsocks_sessions_rejected_total` - Counter of rejected sessions
- `rustsocks_session_duration_seconds` - Histogram of session durations
- `rustsocks_bytes_sent_total` / `rustsocks_bytes_received_total` - Traffic counters
- `rustsocks_user_sessions_total{user}` - Per-user session counter
- `rustsocks_user_bandwidth_bytes_total{user,direction}` - Per-user bandwidth

## Configuration

### Feature Flags

- `default = ["metrics"]` - Prometheus metrics enabled by default
- `metrics` - Enables Prometheus metrics and lazy_static
- `database` - Enables SQLite session persistence via sqlx

### Main Config (`rustsocks.toml`)

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 1000

[auth]
method = "none"  # or "userpass"
# [[auth.users]] for userpass mode

[acl]
enabled = false
config_file = "config/acl.toml"
watch = false  # Enable hot-reload
anonymous_user = "anonymous"

[sessions]
enabled = false
storage = "memory"  # or "sqlite"
database_url = "sqlite://path/to/sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24
traffic_update_packet_interval = 10
stats_window_hours = 24
stats_api_enabled = false
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
```

### ACL Config (`acl.toml`)

```toml
[global]
default_policy = "block"

[[users]]
username = "alice"
groups = ["developers"]

  [[users.rules]]
  action = "block"  # or "allow"
  description = "Block admin panel"
  destinations = ["admin.company.com", "10.0.0.0/8"]
  ports = ["*"]  # or ["80", "443", "8000-9000"]
  protocols = ["tcp"]  # or ["udp", "both"]
  priority = 1000

[[groups]]
name = "developers"
  [[groups.rules]]
  action = "allow"
  destinations = ["*.dev.company.com"]
  ports = ["*"]
  priority = 50
```

## Testing

### Integration Tests

Located in `tests/`:
- `acl_integration.rs` - ACL enforcement end-to-end (handshake, block/allow scenarios)
- `ipv6_domain.rs` - IPv6 and domain resolution
- `session_tracking.rs` - Session lifecycle and traffic tracking

### Running Specific Tests

```bash
# ACL tests only
cargo test acl

# Session tests with database
cargo test --features database session

# Ignored performance tests
cargo test -- --ignored
```

### Test Guidelines

- Use `tokio::test` for async tests
- Session store tests require `#[cfg(feature = "database")]`
- Use `sqlite::memory:` for test databases
- Integration tests in `tests/` directory, unit tests in module files

## Development Notes

### Error Handling

- Custom error type: `RustSocksError` in `utils/error.rs`
- Uses `thiserror` for derive macros
- `Result<T>` is aliased to `std::result::Result<T, RustSocksError>`
- Errors are logged via `tracing` framework

### Async Runtime

- Uses Tokio runtime with "full" feature set
- All I/O operations are async
- Connection handling spawns tasks per connection
- Graceful shutdown via `tokio::signal::ctrl_c()`

### Logging

- `tracing` crate for structured logging
- `tracing-subscriber` for output formatting
- Log levels: trace, debug, info, warn, error
- Pretty and JSON formats supported

### Thread Safety

- `Arc<T>` for shared ownership across tasks
- `RwLock` for ACL config (rare writes, frequent reads)
- `DashMap` for concurrent session access without locking
- `Mutex` for batch writer queue

### Hot Reload Mechanism (`acl/watcher.rs`)

1. Watch ACL config file using `notify` crate
2. On file change, load and validate new config
3. Compile new ACL rules
4. Atomically swap `Arc<RwLock<CompiledAclConfig>>`
5. Rollback on validation errors
6. Typical reload time: <100ms

## Roadmap Context

- **Sprint 1 (Complete)**: MVP with SOCKS5 protocol, auth, basic proxy
- **Sprint 2.1 (Complete)**: ACL engine with hot reload
- **Sprint 2.2-2.4 (Complete)**: Session manager, persistence, metrics, IPv6/domain resolution
- **Sprint 3 (Planned)**: REST API, production packaging, PAM auth, BIND/UDP ASSOCIATE commands
