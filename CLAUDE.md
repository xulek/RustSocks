# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustSocks is a high-performance SOCKS5 proxy server written in Rust, featuring advanced ACL (Access Control List) engine, session management with SQLite persistence, and Prometheus metrics integration.

**Current Status**: Production Ready - Sprint 4.1 Complete (v0.9.0)
- ✅ Core SOCKS5 (CONNECT, BIND, UDP ASSOCIATE)
- ✅ ACL Engine + Hot Reload
- ✅ Session Management + SQLite
- ✅ PAM Authentication + LDAP Groups
- ✅ QoS & Rate Limiting
- ✅ REST API + Web Dashboard
- ✅ SOCKS over TLS (with mTLS support)
- ✅ Connection Pooling & Optimization
- ✅ Performance Verified (All targets exceeded)
- ✅ 277 Tests (263 passing, 14 ignored)

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

# Run all tests with all features
cargo test --all-features

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Check for compilation errors without building
cargo check

# Run clippy for linting
cargo clippy

# Run clippy with strict warnings (treat warnings as errors)
cargo clippy --all-features -- -D warnings
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
  - `mod.rs`: AuthManager with pluggable backend system
  - `pam.rs`: PAM (Pluggable Authentication Modules) integration
  - Supports `NoAuth` (0x00) and `Username/Password` (0x02, RFC 1929)
  - Supports PAM authentication (pam.address and pam.username)
  - Pluggable authentication methods with client-level and SOCKS-level auth

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
  - `pool.rs`: Connection pool for upstream TCP connections with timeout management
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

[server.pool]
enabled = false  # Enable connection pooling for upstream connections
max_idle_per_dest = 4  # Max idle connections per destination
max_total_idle = 100  # Max total idle connections across all destinations
idle_timeout_secs = 90  # How long to keep idle connections alive
connect_timeout_ms = 5000  # Timeout for establishing new connections
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

## Database Operations

### Running Migrations

```bash
# Migrations are automatically applied on startup when using SQLite storage
# Located in: migrations/001_create_sessions_table.sql

# To test migrations manually:
sqlx migrate run --database-url sqlite://sessions.db
```

### Querying Session Data

```bash
# Open SQLite database
sqlite3 sessions.db

# Example queries:
# Active sessions
SELECT user, dest_ip, dest_port, bytes_sent, bytes_received
FROM sessions WHERE status = 'active';

# Rejected by ACL
SELECT user, dest_ip, dest_port, acl_rule_matched
FROM sessions WHERE status = 'rejected_by_acl';

# Top users by traffic
SELECT user, SUM(bytes_sent + bytes_received) as total_bytes, COUNT(*) as sessions
FROM sessions GROUP BY user ORDER BY total_bytes DESC;

# Sessions in last hour
SELECT * FROM sessions
WHERE datetime(start_time) >= datetime('now', '-1 hour');
```

### Database Schema

The `sessions` table includes:
- `session_id` (TEXT, PRIMARY KEY) - UUID
- `user` (TEXT) - Username
- `start_time` / `end_time` (TEXT) - RFC3339 timestamps
- `duration_secs` (INTEGER) - Session duration
- `source_ip` / `source_port` - Client connection info
- `dest_ip` / `dest_port` - Destination info
- `protocol` (TEXT) - "tcp" or "udp"
- `bytes_sent` / `bytes_received` - Traffic counters
- `packets_sent` / `packets_received` - Packet counters
- `status` (TEXT) - "active", "closed", "failed", "rejected_by_acl"
- `close_reason` (TEXT) - Optional close reason
- `acl_rule_matched` (TEXT) - Matched ACL rule description
- `acl_decision` (TEXT) - "allow" or "block"

## Testing

### Integration Tests

Located in `tests/`:
- `acl_integration.rs` - ACL enforcement end-to-end (handshake, block/allow scenarios)
- `ipv6_domain.rs` - IPv6 and domain resolution
- `session_tracking.rs` - Session lifecycle and traffic tracking
- `connection_pool.rs` - Connection pool integration (3 tests)
- `pool_edge_cases.rs` - Pool edge cases and limits (14 tests)
- `pool_socks_integration.rs` - Pool with SOCKS5 flows (4 tests)
- `pool_concurrency.rs` - Pool stress tests (3 tests, ignored)
- `e2e_tests.rs` - Comprehensive E2E tests (10 tests)

### E2E Tests

The `tests/e2e_tests.rs` file contains comprehensive end-to-end tests covering all critical scenarios:

**Test Coverage (10 tests):**
1. `e2e_basic_connect` - Basic SOCKS5 CONNECT with echo server
2. `e2e_auth_noauth` - NoAuth authentication flow
3. `e2e_auth_userpass` - Username/password authentication (valid credentials)
4. `e2e_auth_userpass_invalid` - Authentication rejection (invalid credentials)
5. `e2e_acl_allow` - ACL allows connection
6. `e2e_acl_block` - ACL blocks connection and tracks rejected session
7. `e2e_session_tracking` - Full session lifecycle (create, update, close)
8. `e2e_udp_associate` - UDP ASSOCIATE command
9. `e2e_bind_command` - BIND command handshake
10. `e2e_complete_flow` - Complete flow combining auth + ACL + session + data transfer

**Helper Functions:**
- `create_basic_server_context()` - Creates SOCKS5 server with custom config
- `spawn_echo_server()` - Spawns echo server for data transfer tests
- `spawn_socks_server()` - Spawns SOCKS5 server with handler
- `socks5_handshake_noauth()` - Performs SOCKS5 handshake without auth
- `socks5_handshake_userpass()` - Performs SOCKS5 handshake with username/password
- `socks5_connect()` - Sends SOCKS5 CONNECT request

**Running E2E Tests:**
```bash
# Run all E2E tests
cargo test --all-features --test e2e_tests

# Run with output
cargo test --all-features --test e2e_tests -- --nocapture

# Run specific E2E test
cargo test --all-features e2e_basic_connect
```

### Running Specific Tests

```bash
# ACL tests only
cargo test acl

# Session tests with database
cargo test --features database session

# All tests with all features
cargo test --all-features

# Ignored performance tests (release mode recommended)
cargo test --release -- --ignored

# Specific performance test
cargo test --release acl_performance_under_seven_ms -- --ignored --nocapture

# Run all integration tests
cargo test --test '*'
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

### Code Quality Standards

**Clippy Rules:**
- All code must pass `cargo clippy --all-features -- -D warnings` (warnings as errors)
- Function parameter limits: max 7 parameters (use context structs if exceeded)
- Use modern Rust idioms:
  - `io::Error::other(msg)` instead of `io::Error::new(io::ErrorKind::Other, msg)`
  - Implement standard traits (`Display`, `FromStr`) instead of custom methods
  - Derive `Default` instead of manual implementations where possible
- No unused imports or variables

**Code Patterns:**
- Context structs for grouping related parameters (e.g., `ClientHandlerContext`, `SessionContext`)
- Use `Arc<T>` for shared ownership, `RwLock` for read-heavy workloads, `DashMap` for concurrent access
- All database-related code must be feature-gated with `#[cfg(feature = "database")]`
- Metrics code feature-gated with `#[cfg(feature = "metrics")]`

## UDP ASSOCIATE Command

**Implementation Status**: ✅ Complete

The UDP ASSOCIATE command enables UDP traffic relaying through the SOCKS5 proxy.

### How It Works

1. **TCP Control Connection**: Client sends UDP ASSOCIATE request over TCP
2. **UDP Relay Binding**: Server binds a UDP socket and returns the address/port to client
3. **UDP Packet Format**: All UDP packets use SOCKS5 UDP packet format:
   ```
   +----+------+------+----------+----------+----------+
   |RSV | FRAG | ATYP | DST.ADDR | DST.PORT |   DATA   |
   +----+------+------+----------+----------+----------+
   | 2  |  1   |  1   | Variable |    2     | Variable |
   +----+------+------+----------+----------+----------+
   ```
4. **Bidirectional Relay**: Server forwards packets between client and destination
5. **Session Lifetime**: UDP session remains active while TCP control connection is open
6. **Timeout**: 120-second idle timeout (no packets in either direction)

### Key Components

- **`protocol/types.rs`**: `UdpHeader`, `UdpPacket` structures
- **`protocol/parser.rs`**: `parse_udp_packet()`, `serialize_udp_packet()` functions
- **`server/udp.rs`**: UDP relay implementation
  - `UdpSessionMap`: Tracks client-to-destination mappings
  - `handle_udp_associate()`: Main UDP relay handler
  - `run_udp_relay()`: Relay loop with timeout
  - `handle_client_packet()`: Forward client → destination
  - `handle_destination_packet()`: Forward destination → client
- **`server/handler.rs`**: Integration with main handler flow

### Features

- ✅ Full SOCKS5 UDP packet encapsulation
- ✅ Bidirectional UDP forwarding
- ✅ ACL enforcement (TCP/UDP protocol filtering)
- ✅ Session tracking and traffic metrics
- ✅ IPv4/IPv6/domain name support
- ✅ Automatic cleanup on TCP disconnect
- ✅ 120-second idle timeout
- ❌ UDP fragmentation not supported (FRAG must be 0)

### Testing

```bash
# Run UDP tests
cargo test --all-features udp

# Integration tests include:
# - Basic UDP ASSOCIATE flow
# - ACL allow/block for UDP
# - Session tracking
```

## BIND Command

**Implementation Status**: ✅ Complete

The BIND command enables reverse connections through the SOCKS5 proxy, allowing incoming connections to reach the client.

### How It Works

1. **BIND Request**: Client sends BIND command specifying destination address and port
2. **Listener Binding**: Server binds a TCP listener on an ephemeral port (0)
3. **First Response**: Server sends first SOCKS5 response with the bind address/port
4. **Wait for Connection**: Server waits up to 300 seconds (RFC 1928) for incoming connection
5. **Second Response**: Server sends second response with the connecting peer's address/port
6. **Data Proxying**: Server proxies data bidirectionally between client and incoming connection
7. **Session Cleanup**: Session closes when connection ends

### Key Components

- **`server/bind.rs`**: BIND command implementation
  - `handle_bind()`: Main BIND handler
  - `send_bind_response()`: Send SOCKS5 BIND responses
  - `BIND_ACCEPT_TIMEOUT`: 300-second timeout per RFC 1928
- **`server/handler.rs`**: Integration with main handler flow (Command::Bind match)

### Features

- ✅ RFC 1928 compliant (300-second timeout)
- ✅ Two-response protocol (bind address, then peer address)
- ✅ ACL enforcement for incoming connections
- ✅ Session tracking and traffic metrics
- ✅ IPv4/IPv6 address support
- ✅ Proper timeout handling with error responses
- ✅ Bidirectional data proxying

### BIND Response Format

```
+----+-----+-------+------+----------+----------+
|VER | REP |  RSV  | ATYP | BND.ADDR | BND.PORT |
+----+-----+-------+------+----------+----------+
| 1  |  1  |   1   |  1   | Variable |    2     |
+----+-----+-------+------+----------+----------+
```

### Testing

```bash
# Run BIND tests
cargo test --all-features bind

# Integration tests include:
# - Basic BIND handshake
# - BIND with incoming connection acceptance
# - ACL allow/block for BIND
# - Session tracking
```

## PAM Authentication

**Implementation Status**: ✅ Complete

PAM (Pluggable Authentication Modules) provides flexible system-level authentication for RustSocks.

### Authentication Methods

RustSocks supports **two-tier authentication**:

1. **Client-level auth** (before SOCKS handshake) - `client_method`
2. **SOCKS-level auth** (after SOCKS handshake) - `socks_method`

#### pam.address - IP-based Authentication

Authenticates clients based on IP address only (no username/password required).

- **Use case**: Trusted networks, IP-based ACLs
- **Configuration**: `client_method = "pam.address"` or `socks_method = "pam.address"`
- **PAM service**: Configured via `auth.pam.address_service` (default: "rustsocks-client")
- **Default user**: `auth.pam.default_user` (default: "rhostusr")

#### pam.username - Username/Password Authentication

Traditional SOCKS5 username/password authentication via PAM.

- **Use case**: User-based access control with system accounts
- **Configuration**: `socks_method = "pam.username"`
- **PAM service**: Configured via `auth.pam.username_service` (default: "rustsocks")
- **Protocol**: SOCKS5 username/password (RFC 1929)
- ⚠️ **Security**: Password transmitted in clear-text (use in trusted networks only)

### Key Components

- **`src/auth/pam.rs`**: PAM integration
  - `PamAuthenticator`: Async PAM authentication wrapper
  - `PamMethod`: Address vs Username method enum
  - `authenticate_address()`: IP-only authentication
  - `authenticate_username()`: Username/password authentication
  - Cross-platform support (Unix + non-Unix fallback)
- **`src/auth/mod.rs`**: AuthManager integration
  - `AuthBackend::PamAddress` and `PamUsername` variants
  - `authenticate_client()`: Pre-SOCKS authentication (pam.address)
  - `authenticate()`: SOCKS-level authentication (pam.username)
- **`src/config/mod.rs`**: PamSettings configuration and validation

### Configuration

```toml
[auth]
# Client-level authentication (before SOCKS handshake)
client_method = "none"           # Options: "none", "pam.address"

# SOCKS-level authentication (after SOCKS handshake)
socks_method = "pam.username"    # Options: "none", "userpass", "pam.address", "pam.username"

[auth.pam]
# PAM service names (corresponds to /etc/pam.d/<service>)
username_service = "rustsocks"
address_service = "rustsocks-client"

# Default identity for pam.address authentication
default_user = "rhostusr"
default_ruser = "rhostusr"

# Enable verbose PAM logging
verbose = false

# Verify PAM service files exist at startup
verify_service = false
```

### PAM Service Files

Example PAM configurations are provided in `config/pam.d/`:

- **`rustsocks`** - Username/password authentication (production)
- **`rustsocks-client`** - IP-based authentication (production)
- **`rustsocks-test`** - Permissive config for testing
- **`rustsocks-client-test`** - Permissive config for testing

**Installation**:
```bash
# Copy to system PAM directory
sudo cp config/pam.d/rustsocks /etc/pam.d/rustsocks
sudo cp config/pam.d/rustsocks-client /etc/pam.d/rustsocks-client

# Set permissions
sudo chmod 644 /etc/pam.d/rustsocks*
```

See `config/pam.d/README.md` for detailed setup instructions.

### Features

- ✅ Two-tier authentication (client + SOCKS levels)
- ✅ pam.address - IP-based authentication
- ✅ pam.username - Username/password authentication
- ✅ Async PAM operations via `spawn_blocking`
- ✅ Cross-platform support (Unix + fallback)
- ✅ Configurable PAM service names
- ✅ Integration with ACL engine
- ✅ Session tracking with PAM decisions
- ✅ Comprehensive error handling

### Testing

```bash
# Run PAM tests (requires PAM setup)
cargo test --all-features pam -- --ignored

# Integration tests include:
# - PAM configuration validation
# - pam.address authentication
# - pam.username authentication
# - Cross-platform compatibility
# - Error handling
```

**Note**: PAM integration tests require:
- PAM installed on the system
- Test PAM service files in `/etc/pam.d/`
- Running as root (for actual PAM authentication)

### Security Considerations

1. **Clear-text passwords**: SOCKS5 username/password transmits credentials unencrypted
   - Only use in trusted networks
   - Consider TLS wrapper, VPN, or SSH tunnel for production
2. **PAM service configuration**:
   - ⚠️ On some systems, missing PAM service file may allow all connections!
   - Always verify `/etc/pam.d/<service>` exists
   - Test both successful and failed authentication
3. **Privilege requirements**:
   - PAM typically requires root for password verification
   - Server should drop privileges after binding socket
4. **Defense in depth**:
   - Combine PAM with ACL engine for layered security
   - Use `client_method = "pam.address"` + `socks_method = "pam.username"` for dual authentication

### Platform Support

- **Unix/Linux**: Full PAM support via `pam` crate
- **Windows/macOS**: Stub implementation (returns NotSupported error)
- **Build-time**: Requires `libpam-dev` on Unix systems (na Red Hat / CentOS dodatkowo `gcc`, `nodejs`, `rust`, `cargo`, `pam-devel`)

**Dependencies**:
```toml
[target.'cfg(unix)'.dependencies]
pam = "0.7"
```

### Examples

#### Example 1: Username/password authentication only
```toml
[auth]
client_method = "none"
socks_method = "pam.username"
```

#### Example 2: IP filtering + username/password (defense in depth)
```toml
[auth]
client_method = "pam.address"      # IP check before SOCKS
socks_method = "pam.username"      # Username/password after SOCKS
```

#### Example 3: IP-based authentication only (trusted network)
```toml
[auth]
client_method = "none"
socks_method = "pam.address"
```

## SOCKS over TLS

**Implementation Status**: ✅ Complete

RustSocks supports full TLS encryption for SOCKS5 connections, including mutual TLS (mTLS) with client certificate authentication.

### Features

- ✅ Full TLS 1.2 and TLS 1.3 support
- ✅ Server certificate configuration
- ✅ Mutual TLS (mTLS) with client authentication
- ✅ Configurable protocol versions
- ✅ Integration with all authentication methods
- ✅ Session tracking with encrypted connections

### Key Components

- **`src/server/listener.rs`**: `create_tls_acceptor()` - TLS initialization
  - Certificate and key loading
  - Protocol version configuration
  - Client CA path (for mTLS)
- **`src/config/mod.rs`**: `TlsSettings` - Configuration struct
- **Integration tests**: `tests/tls_support.rs`
  - Basic SOCKS5 over TLS
  - Mutual TLS with client certificates

### Configuration

```toml
[server.tls]
enabled = true
certificate_path = "/etc/rustsocks/server.crt"
private_key_path = "/etc/rustsocks/server.key"
min_protocol_version = "TLS13"  # or "TLS12"

# For mutual TLS (client authentication):
require_client_auth = true
client_ca_path = "/etc/rustsocks/clients-ca.crt"
```

### Testing

```bash
# Run TLS integration tests
cargo test --all-features tls_support

# Test with mTLS (requires client cert)
cargo test --all-features socks5_connect_with_mutual_tls
```

### Security Benefits

- **Encryption**: All SOCKS5 handshake and data traffic encrypted
- **No plaintext credentials**: Even with username/password auth, credentials are transmitted over TLS
- **mTLS support**: Client certificate validation for additional security
- **Protocol enforcement**: Can require TLS 1.3 minimum for maximum security

### Typical Deployment

```bash
# Generate self-signed certificate (for testing)
openssl req -x509 -newkey rsa:4096 -keyout server.key -out server.crt -days 365 -nodes

# Production: Use certificates from trusted CA
# Place in /etc/rustsocks/ and set permissions
sudo chmod 600 /etc/rustsocks/server.key
```

## Connection Pool & Optimization

**Implementation Status**: ✅ Complete (Sprint 4.1)

RustSocks includes an efficient connection pool for upstream TCP connections, reducing connection establishment overhead and improving performance.

### How It Works

1. **Pool Management**: Idle upstream connections are stored per-destination
2. **Connection Reuse**: When connecting to the same destination, pooled connections are reused
3. **Timeout Handling**: Connections expire after idle_timeout_secs of inactivity
4. **Background Cleanup**: Periodic cleanup task removes expired connections
5. **Capacity Limits**: Both per-destination and global limits prevent resource exhaustion

### Key Features

- ✅ LRU-style connection pooling with timeout management
- ✅ Per-destination and global connection limits
- ✅ Configurable idle timeout and connect timeout
- ✅ Background cleanup of expired connections
- ✅ Thread-safe implementation using Arc<Mutex>
- ✅ Optional (disabled by default for backward compatibility)
- ✅ Zero-copy connection reuse
- ✅ Automatic eviction when limits are reached

### Configuration

```toml
[server.pool]
enabled = true  # Enable connection pooling
max_idle_per_dest = 4  # Max idle connections per destination
max_total_idle = 100  # Max total idle connections
idle_timeout_secs = 90  # Keep-alive duration
connect_timeout_ms = 5000  # Connection timeout
```

### Benefits

- **Reduced Latency**: Reusing connections eliminates TCP handshake overhead
- **Lower CPU Usage**: Fewer connection establishments
- **Better Resource Utilization**: Controlled connection limits
- **Improved Throughput**: Faster connection reuse for frequent destinations

### Implementation Details

- **Location**: `src/server/pool.rs` (445 lines)
- **Key Structures**:
  - `ConnectionPool`: Main pool manager
  - `PooledConnection`: Wrapper with metadata (created_at, last_used)
  - `PoolConfig`: Configuration parameters
  - `PoolStats`: Pool statistics API
- **Integration**: `handler.rs` uses pool via `ConnectHandlerContext`
- **Testing**: 7 unit tests + 21 integration tests (comprehensive coverage)

### Testing

```bash
# Run pool unit tests
cargo test --all-features pool

# Run pool integration tests (3 basic tests)
cargo test --all-features --test connection_pool

# Run pool edge case tests (14 comprehensive tests)
cargo test --all-features --test pool_edge_cases

# Run pool SOCKS integration tests (4 tests)
cargo test --all-features --test pool_socks_integration

# Run concurrency stress tests (3 tests, ignored by default)
cargo test --all-features --test pool_concurrency -- --ignored --nocapture

# Run all pool tests at once
cargo test --all-features pool
```

**Test Coverage**:
- Basic integration (connection_pool.rs): Connection reuse, timeout handling, disabled mode
- Edge cases (pool_edge_cases.rs): Closed servers, expired connections, per-dest limits, global limits, stats accuracy, concurrent operations, LIFO behavior, cleanup tasks
- SOCKS5 integration (pool_socks_integration.rs): Full SOCKS5 flows with pooling, error handling, stats reflection
- Stress tests (pool_concurrency.rs): 200-500 concurrent operations, mutex contention benchmarks

### Performance Under Load

**Stress Test Results** (200-500 concurrent operations):
- ✅ **100% success rate** - Zero failures under load
- ✅ **Throughput scales** - 3,000 ops/sec (1 thread) → 7,000 ops/sec (200 threads)
- ✅ **Sub-millisecond latency** - Average 742µs per operation
- ✅ **No mutex contention** - Performance improves with concurrency
- ✅ **Production ready** - Handles hundreds of concurrent connections efficiently

The `Arc<Mutex<HashMap>>` implementation provides excellent performance because:
- Critical sections are very short (HashMap lookup/insert only)
- Most time spent in I/O (connect), not holding locks
- Lock-free fast paths (disabled pool, empty pool)
- Tokio async yields during I/O operations

## Web Dashboard

**Implementation Status**: ✅ Complete

RustSocks includes a modern web-based admin dashboard built with React.

### Features

- **Real-time Session Monitoring**: View active and historical sessions with live updates
- **ACL Management**: Browse groups, users, and their access rules
- **User Management**: View users and group memberships
- **Statistics Dashboard**: Analytics including bandwidth, top users, and destinations
- **Configuration View**: Server health and API documentation
- **Modern UI**: Dark theme with clean, intuitive design

### Configuration

Enable dashboard and Swagger UI in `rustsocks.toml`:

```toml
[sessions]
stats_api_enabled = true    # Enable API server
dashboard_enabled = true    # Enable web dashboard
swagger_enabled = true      # Enable Swagger UI
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
```

### Development

```bash
# Install dependencies
cd dashboard
npm install

# Run development server (with proxy to API)
npm run dev

# Build for production
npm run build
```

Development server runs at `http://localhost:3000` with API proxy.

### Production Deployment

Dashboard is served automatically from `dashboard/dist/` when:
1. `dashboard_enabled = true` in config
2. Dashboard has been built (`npm run build`)
3. API server is enabled

Access dashboard at: `http://127.0.0.1:9090/`

### URL Base Path Support

RustSocks supports deploying under a custom URL prefix:

```toml
[sessions]
base_path = "/rustsocks"  # Options: "/", "/rustsocks", "/proxy", etc.
```

**How it works:**
- Backend nests all routes under the prefix
- Frontend auto-detects base path from injected `window.__RUSTSOCKS_BASE_PATH__`
- React Router uses `basename` for client-side routing
- All API calls automatically include the prefix via `getApiUrl()`

**Build process:**
```bash
# 1. Set base_path in config
# 2. Build frontend
cd dashboard && npm run build

# 3. Build backend
cargo build --release

# 4. Run server
./target/release/rustsocks --config config/rustsocks.toml
```

**URLs with base_path = "/rustsocks":**
- Dashboard: `http://127.0.0.1:9090/rustsocks`
- API: `http://127.0.0.1:9090/rustsocks/api/`
- Swagger: `http://127.0.0.1:9090/rustsocks/swagger-ui/`

For detailed instructions including nginx reverse proxy setup, see [Building with Base Path Guide](docs/guides/building-with-base-path.md).

### Dashboard Pages

- **Dashboard**: Real-time overview with session stats, top users, top destinations
- **Sessions**: Live session monitoring with active/history toggle
- **ACL Rules**: Browse and view ACL groups, users, and rules
- **Users**: User management and group memberships
- **Statistics**: Detailed analytics and bandwidth metrics
- **Configuration**: Server health, uptime, API documentation

### Security Notes

- Dashboard is for **administrative use only**
- Deploy behind authentication/VPN in production
- Do not expose to public internet
- API endpoints require security tokens (future enhancement)

### Tech Stack

- React 18 with hooks
- Vite build system
- React Router for navigation
- Lucide React icons
- Vanilla CSS (no framework)

See `dashboard/README.md` for detailed documentation.

## Roadmap Context

- **Sprint 1 (Complete)**: MVP with SOCKS5 protocol, auth, basic proxy ✅
- **Sprint 2.1 (Complete)**: ACL engine with hot reload ✅
- **Sprint 2.2-2.4 (Complete)**: Session manager, persistence, metrics, IPv6/domain resolution ✅
- **Sprint 3.1 (Complete)**: UDP ASSOCIATE command ✅
- **Sprint 3.2 (Complete)**: BIND command ✅
- **Sprint 3.3 (Complete)**: REST API + endpoints ✅
- **Sprint 3.4 (Complete)**: LDAP Groups integration ✅
- **Sprint 3.5 (Complete)**: Web Dashboard ✅
- **Sprint 3.6 (Complete)**: QoS & Rate Limiting ✅
- **Sprint 3.7 (Complete)**: PAM authentication ✅
- **Sprint 3.8 (Complete)**: LDAP Groups integration ✅
- **Sprint 3.9 (Complete)**: Web Dashboard enhancements ✅
- **Sprint 3.10 (Complete)**: Load Testing + Performance Verification ✅
- **Sprint 4.1 (Complete)**: Connection Pooling & Optimization ✅
- **Sprint 4+ (Planned)**: Production packaging, systemd integration, Grafana dashboards

## Quality Metrics (Latest - 2025-11-01)

- **Tests**: 253/253 passing (242 pass, 11 ignored) ✅
  - Unit: 98 tests (ACL, QoS, Pool, Auth)
  - Integration: 155 tests (E2E flows + stress tests)
  - Stress: 3 concurrency tests (200-500 concurrent ops)
- **Code Quality**: cargo clippy --all-features -- -D warnings ✅ (zero warnings)
- **Security**: cargo audit (2 unfixable issues in transitive deps, not affecting SQLite-only usage)
- **Performance**: All targets exceeded
  - Latency: <5ms avg (target: <50ms p99) ✅
  - Connection Pool: 7,000 ops/sec @ 200 concurrent threads ✅
  - ACL: 1.92ms avg (target: <5ms) ✅
  - Session: 1.01ms overhead (target: <2ms) ✅
  - DB writes: 12,279/s (target: >1000/s) ✅
  - Memory: 231 MB @ 200k+ conn (target: <800MB @ 5k) ✅
  - API: 96ms avg (target: <100ms) ✅
- **Dependencies**: Updated to latest (sqlx 0.8, prometheus 0.14, protobuf 3.7)

## Test Coverage Summary

**Total Tests: 287 (273 passing, 14 ignored)**

### Test Breakdown by Category

| Category | Tests | Status |
|----------|-------|--------|
| ACL (unit + integration + API + matchers) | 134 | 132 ✅ + 2 ignored |
| Authentication (PAM + groups) | 31 | 24 ✅ + 7 ignored |
| QoS (unit + integration) | 36 | 36 ✅ |
| Connection Pool (unit + integration + stress) | 31 | 28 ✅ + 3 ignored |
| E2E Tests | 10 | 10 ✅ |
| Protocol & Session | 2 | 2 ✅ |
| API Endpoints | 11 | 11 ✅ |
| Integration (BIND, UDP, IPv6, TLS) | 10 | 10 ✅ |
| Configuration & Utils | 9 | 9 ✅ |
| Documentation | 1 | 1 ✅ |
| **TOTAL** | **287** | **273 ✅ + 14 ⊘** |

### Coverage by Component
- **ACL Engine**: >90% coverage
- **Authentication (PAM)**: >85% coverage (19 new tests added)
- **Session Manager**: >85% coverage
- **API Endpoints**: >85% coverage
- **QoS/Rate Limiting**: >90% coverage
- **Protocol Implementation**: >85% coverage
- **Connection Pool**: 100% coverage (comprehensive edge case testing)
- **E2E Scenarios**: 100% coverage (all critical flows)

### Key Test Categories
- ✅ Unit tests: 97 tests (95 passing, 2 ignored)
- ✅ Integration tests: 180 tests
  - ACL: 14 tests (comprehensive matchers + API)
  - Pool: 28 tests (21 integration + 7 unit + 3 stress ignored)
  - QoS: 36 tests (complex scenarios)
  - API endpoints: 11 tests
  - UDP ASSOCIATE: 3 tests
  - BIND command: 4 tests
  - LDAP groups: 7 tests
  - PAM integration: 16 tests (9 passing, 7 ignored)
  - TLS support: 2 tests (mTLS validation)
  - IPv6/Domain: 1 test
  - Session tracking: 1 test
  - Documentation: 1 test
- ✅ E2E tests: 10 comprehensive tests
  - basic_connect, auth (NoAuth, UserPass, invalid)
  - ACL (allow, block), session_tracking
  - UDP, BIND, complete_flow

### Continuous Integration
- ✅ `cargo fmt` - zero style issues
- ✅ `cargo clippy --all-features -- -D warnings` - zero warnings
- ✅ `cargo test --all-features` - 273 passing, 14 ignored
- ✅ `cargo audit` - 2 known transitive vulnerabilities (no fix available, low risk)
