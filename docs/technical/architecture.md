# RustSocks Architecture

This document describes the module structure of RustSocks and the path a connection takes through it.

## Module Structure

### `protocol/` - SOCKS wire protocol
- `types.rs`: protocol structures (greetings, requests, UDP packets, address types)
- `parser.rs`: async parsers and serializers for SOCKS5, SOCKS4/4a, the username/password sub-negotiation, GSS-API frames and UDP datagrams
- Rejects malformed input instead of guessing: non-zero reserved fields, empty domains, oversized lengths. The parsers are covered by property tests (`tests/protocol_robustness.rs`)

### `auth/` - Authentication
- `mod.rs`: `AuthManager` with pluggable backends: none, username/password (Argon2 hashes or plain text), PAM (address or username), GSS-API
- `throttle.rs`: bounded per-address failure throttle shared by the username/password and PAM backends
- `pam/`: PAM integration (Unix); a stub on other platforms
- `gssapi.rs`: Kerberos authentication (feature `gssapi`, Unix)
- `groups.rs`: group lookup through NSS/SSSD with a timeout, a concurrency limit and a short cache

### `acl/` - Access control
- `types.rs`: configuration types for rules, groups and dynamic policies
- `matcher.rs`: compiled destination, port and protocol matchers
- `engine.rs`: evaluation of legacy rules and policies in one priority order
- `policy.rs`: policy conditions (source network, authentication method, schedule, validity window), admission limits and the in-memory usage tracker
- `usage.rs`, `usage_redis.rs`: where policy counters live (process memory, or Redis shared between instances)
- `loader.rs`, `persistence.rs`, `crud.rs`: loading, atomic saving and editing of the ACL file
- `watcher.rs`: hot reload
- `stats.rs`, `metrics.rs`: per-user allow/block counters and Prometheus metrics for decisions

See [ACL Engine](acl-engine.md) and the [policy engine guide](../config/policy-engine.md).

### `session/` - Session tracking
- `manager.rs`: active sessions in a concurrent map, bounded in-memory history, traffic accounting and policy usage
- `store/`: SQLite and MySQL/MariaDB persistence through sqlx and `mysql_async` (feature `database`)
- `batch.rs`: batch writer with a bounded queue and backpressure (feature `database`)
- `history.rs`: rolling metric snapshots for the dashboard
- `metrics.rs`: Prometheus session metrics (feature `metrics`)

See [Session Management](session-management.md).

### `server/` - Network layer
- `listener.rs`: accept loop, TLS, global and per-address connection limits, startup wiring
- `handler.rs`: the SOCKS5 and SOCKS4/4a state machines: authentication, ACL and policy decision, CONNECT
- `bind.rs`, `udp.rs`: BIND and UDP ASSOCIATE
- `resolver.rs`: DNS with a timeout, a bounded cache and a lookup concurrency limit
- `pool.rs`: reusable upstream TCP connections (disabled by default)
- `proxy.rs`: bidirectional relay with half-close semantics, traffic updates and QoS
- `net.rs`: socket tuning

### `api/` - Management API and dashboard
- `server.rs`: axum router, dashboard and Swagger page, authentication and role middleware, audit log
- `auth.rs`: dashboard login, sessions, lockout and optional Altcha challenge
- `handlers/`: sessions, statistics, telemetry, ACL and policy management, configuration, SMTP, diagnostics, system resources, pool

### `qos/`, `smtp/`, `telemetry.rs`, `utils/`
- `qos/`: hierarchical token bucket bandwidth limiting and per-user connection limits
- `smtp/`: encrypted SMTP settings and e-mail notifications (resource alerts, cooldowns)
- `telemetry.rs`: in-memory buffer of operational events (pool pressure, upstream failures)
- `utils/`: error type, system resource probes, `crypto` helpers (OS randomness, hex)

### `config/`
TOML configuration with defaults, `${ENV}` expansion for secrets, validation (`validate_effective`) and CLI overrides.

## Request Flow

### 1. Accept (`listener.rs`)
- A slot is taken from the global connection limit and, when `server.max_connections_per_ip` is set, from the per-address limit; otherwise the connection is dropped
- TLS is negotiated with its own timeout when enabled
- Client-level authentication (`pam.address`) runs before any SOCKS bytes are read

### 2. Handshake and authentication (`handler.rs`)
- The first byte selects SOCKS5 (`0x05`) or SOCKS4/4a (`0x04`), both under the handshake timeout
- SOCKS5: method negotiation, then username/password (throttled per address), PAM or GSS-API
- SOCKS4/4a only works when no authentication is configured, and its USERID is treated as untrusted text, never as an identity

### 3. ACL and policy decision
- The user's groups (static `[[users]].groups` plus dynamic NSS/LDAP groups) are resolved
- `AclEngine::evaluate_policy_for_traffic` evaluates legacy rules and dynamic policies together and returns allow or block, the matched rule or policy and any admission limits
- A blocked request gets `ConnectionNotAllowed` and is recorded as a rejected session
- An allowed request reserves capacity against the policy limits (active connections, rate, quotas); if a limit is reached it is rejected the same way
- The decision is counted in `rustsocks_acl_decisions_total`

### 4. Connection establishment
- The destination is resolved (timeout, bounded cache)
- Every resolved address is checked again against explicit block rules, so a hostname cannot reach a blocked CIDR
- A pooled connection is reused when pooling is enabled, otherwise a new one is opened; the session is created

### 5. Data proxying (`proxy.rs`)
- Bidirectional copy; EOF in one direction does not cancel the other
- Traffic counters are updated every N packets and flushed on close; they also feed policy transfer quotas
- QoS limits bandwidth when enabled

### 6. Close
- The session is marked closed, the final traffic snapshot is persisted, the policy admission slot is released

BIND validates the accepted peer against the ACL, and UDP ASSOCIATE checks each datagram (and its resolved address) against the ACL, with the same decision path.

## ACL Engine Design

- **One priority order:** legacy rules and policies are evaluated by `priority`, highest first. At equal priority `block` wins over `allow`, and then a policy is evaluated before a legacy rule. This is *not* "all blocks first"; a higher-priority `allow` beats a lower-priority `block`
- **No per-request sorting:** each source (a user's rules, each group's rules, the policy list) is sorted once when the configuration is compiled, and evaluation merges those sorted lists lazily and stops at the first match
- **Prepared destination:** the destination is lowercased and parsed once per request, not once per rule
- **Concurrency:** the compiled configuration sits behind `Arc<RwLock<...>>`; evaluation takes a read lock and reloads swap it atomically
- **Two evaluation paths:** the per-connection path builds no explanation trace; the Explain API builds a full trace

See [ACL Engine](acl-engine.md) for details.

## Session Manager Design

- Active sessions: concurrent map keyed by session id, each behind its own lock
- Closed and rejected sessions are kept in memory in bounded lists (`sessions.history_max_entries` and `retention_days`)
- Traffic updates go through a queue to a worker; when the queue is full, updates are coalesced per session instead of spawning tasks
- With the `database` feature, finished sessions are written in batches (`batch_size`, `batch_interval_ms`) through a bounded queue; on shutdown the queue is drained
- `GET /api/sessions/stats?window_hours=N` aggregates a rolling window

## Metrics (feature `metrics`)

Prometheus metrics are exported at `/metrics`: sessions, traffic, QoS, ACL and policy decisions, admission denials, authentication failures, evaluation latency and shared-state errors. See [Metrics & Audit Log](../guides/metrics-and-audit.md) for the full list.

## Operational Telemetry

- `telemetry.rs` keeps recent events in memory (`TelemetryHistory`) for the dashboard and API
- Events cover connection pool drops/evictions and upstream connection failures, each with a timestamp, severity, category and message
- The buffer is configured by `telemetry.max_events` and `telemetry.retention_hours` and served at `GET /api/telemetry/events`

## Thread Safety

- `Arc<T>` for shared ownership across tasks
- `RwLock` for the compiled ACL (frequent reads, rare writes)
- `DashMap` for concurrent session and per-address state
- Bounded queues and semaphores wherever input can arrive faster than it is processed (traffic updates, batch writer, DNS lookups, group lookups, connections)

## Hot Reload (`acl/watcher.rs`)

1. The ACL file is watched with `notify`, with a polling fallback and a content fingerprint to avoid duplicate reloads
2. A change is loaded and validated
3. The new rules are compiled
4. The compiled configuration is swapped in atomically
5. On any error the previous configuration stays active
6. A reload can also be triggered with `POST /api/admin/reload-acl`

Dynamic policies are only evaluated when a new session is opened; a reload does not terminate sessions that are already established.

## Related Documentation

- [ACL Engine](acl-engine.md)
- [PAM Authentication](pam-authentication.md)
- [Session Management](session-management.md)
- [Connection Pool](connection-pool.md)
- [Protocol Implementation](protocol.md) - UDP, BIND, TLS
- [Active Directory Integration](../guides/active-directory.md)
- [Testing Guide](../guides/testing.md)
