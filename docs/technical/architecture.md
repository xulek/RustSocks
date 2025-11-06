# RustSocks Architecture

This document provides a detailed overview of the RustSocks architecture, module structure, and request flow.

## Module Structure

The codebase follows a modular architecture organized by functionality:

### `protocol/` - SOCKS5 Protocol Implementation
- `types.rs`: Core protocol structures (ClientGreeting, Socks5Request, etc.)
- `parser.rs`: Async parsing logic for SOCKS5 messages
- Supports IPv4, IPv6, and domain name addressing
- UDP packet format structures and serialization

### `auth/` - Authentication Manager
- `mod.rs`: AuthManager with pluggable backend system
- `pam.rs`: PAM (Pluggable Authentication Modules) integration
- `groups.rs`: Dynamic group resolution via getgrouplist()
- Supports:
  - NoAuth (0x00)
  - Username/Password (0x02, RFC 1929)
  - PAM authentication (pam.address and pam.username)
  - Two-tier authentication (client-level and SOCKS-level)

### `acl/` - Access Control List Engine
- `types.rs`: ACL data structures (Action, Protocol, matchers)
- `matcher.rs`: Pattern matching logic (IP, CIDR, domain wildcards, ports)
- `engine.rs`: Rule evaluation with priority ordering (BLOCK rules first)
- `loader.rs`: TOML config parsing and validation
- `watcher.rs`: Hot-reload support with zero-downtime updates
- `stats.rs`: Per-user allow/block statistics

See [ACL Engine Documentation](acl-engine.md) for detailed implementation.

### `session/` - Session Tracking and Metrics
- `types.rs`: Session data structures and filters
- `manager.rs`: In-memory session tracking using DashMap
- `store.rs`: SQLite persistence with sqlx (feature-gated: `database`)
- `batch.rs`: Batch writer for efficient database writes (feature-gated: `database`)
- `metrics.rs`: Prometheus metrics integration (feature-gated: `metrics`)

See [Session Management Documentation](session-management.md) for detailed implementation.

### `server/` - Server Implementation
- `listener.rs`: TCP listener setup and TLS acceptor
- `handler.rs`: Connection handler orchestrating auth → ACL → connect → proxy
- `proxy.rs`: Bidirectional data transfer with traffic tracking
- `resolver.rs`: DNS resolution supporting IPv4/IPv6/domains
- `pool.rs`: Connection pool for upstream TCP connections
- `stats.rs`: Statistics API (HTTP endpoint)
- `udp.rs`: UDP ASSOCIATE implementation
- `bind.rs`: BIND command implementation

### `config/` - Configuration Management
- TOML-based configuration with validation
- CLI argument overrides
- Nested configs for server, auth, ACL, sessions
- Feature flags and platform-specific settings

## Request Flow

### 1. TCP Accept (`listener.rs`)
- Accept incoming connection
- Apply TLS if enabled
- Spawn handler task

### 2. SOCKS5 Handshake (`handler.rs`)
- Parse client greeting
- Negotiate authentication method
- Authenticate user (if required)
  - Client-level authentication (before handshake)
  - SOCKS-level authentication (after handshake)

### 3. ACL Evaluation (`handler.rs` + `acl/engine.rs`)
- Extract destination and protocol from SOCKS5 request
- Resolve user groups (if using PAM/LDAP)
- Evaluate per-user and per-group rules
- BLOCK rules take priority over ALLOW rules
- Log decision and update ACL statistics
- If blocked: send `ConnectionNotAllowed`, track rejected session, close connection

### 4. Connection Establishment (`handler.rs` + `resolver.rs`)
- Resolve destination (IPv4/IPv6/domain)
- Check connection pool for reusable connection
- Connect to target server (or reuse pooled connection)
- Create session in SessionManager

### 5. Data Proxying (`proxy.rs`)
- Bidirectional copy between client and target
- Track traffic (bytes/packets sent/received)
- Update session metrics periodically (configurable interval)
- Apply QoS/rate limiting if enabled
- Final flush on connection close

### 6. Session Lifecycle (`session/manager.rs`)
- `new_session()`: Create active session
- `update_traffic()`: Increment traffic counters
- `close_session()`: Mark completed, record duration
- `track_rejected_session()`: Record ACL rejections

### 7. Persistence (`session/store.rs`, `session/batch.rs`)
- Batch writer queues sessions
- Auto-flush on batch_size or batch_interval_ms
- Background cleanup task removes old records (retention_days)

## ACL Engine Design

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

See [ACL Engine Documentation](acl-engine.md) for comprehensive details.

## Session Manager Design

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
- HTTP endpoint: `GET /api/sessions/stats?window_hours=48`

**Database Integration (feature: `database`):**
- SQLite backend via sqlx
- Async migrations in `migrations/` directory
- Batch writer for performance (configurable batch size and interval)
- Automatic cleanup of old records

See [Session Management Documentation](session-management.md) for implementation details.

## Metrics (feature: `metrics`)

Prometheus metrics exported via `prometheus` crate:
- `rustsocks_active_sessions` - Gauge of active sessions
- `rustsocks_sessions_total` - Counter of accepted sessions
- `rustsocks_sessions_rejected_total` - Counter of rejected sessions
- `rustsocks_session_duration_seconds` - Histogram of session durations
- `rustsocks_bytes_sent_total` / `rustsocks_bytes_received_total` - Traffic counters
- `rustsocks_user_sessions_total{user}` - Per-user session counter
- `rustsocks_user_bandwidth_bytes_total{user,direction}` - Per-user bandwidth

## Thread Safety

- `Arc<T>` for shared ownership across tasks
- `RwLock` for ACL config (rare writes, frequent reads)
- `DashMap` for concurrent session access without locking
- `Mutex` for batch writer queue

## Hot Reload Mechanism (`acl/watcher.rs`)

1. Watch ACL config file using `notify` crate
2. On file change, load and validate new config
3. Compile new ACL rules
4. Atomically swap `Arc<RwLock<CompiledAclConfig>>`
5. Rollback on validation errors
6. Typical reload time: <100ms

## Related Documentation

- [ACL Engine Details](acl-engine.md)
- [PAM Authentication](pam-authentication.md)
- [Session Management](session-management.md)
- [Connection Pool](connection-pool.md)
- [Protocol Implementation](protocol.md) - UDP, BIND, TLS
- [Active Directory Integration](../guides/active-directory.md)
- [Testing Guide](../guides/testing.md)
