# rustsocks.toml Reference

This document describes every supported option in the main configuration file.

RustSocks loads configuration from a TOML file passed via `--config`. When no file is provided, defaults are used.

## File Structure

The file is grouped into blocks:

- `[server]` core listener settings
- `[server.resolver]` DNS timeouts, cache and concurrency
- `[server.tls]` TLS configuration
- `[server.pool]` connection pooling
- `[auth]` authentication strategy
- `[logging]` log format and level
- `[acl]` ACL system and hot reload
- `[sessions]` session tracking + API/dashboard
- `[sessions.dashboard_auth]` dashboard authentication
- `[policy_state]` where dynamic-policy counters live (memory or Redis)
- `[metrics]` metrics retention and storage
- `[telemetry]` operational event feed
- `[qos]` rate limiting and connection limits

## Minimal Example

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080

[auth]
client_method = "none"
socks_method = "none"
```

For a full example, see `docs/examples/rustsocks.example.toml`.

## [server]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `bind_address` | string | `127.0.0.1` | Address to bind the SOCKS5 listener. |
| `bind_port` | integer | `1080` | Port to bind the SOCKS5 listener. |
| `max_connections` | integer | `1000` | Maximum concurrent connections. |
| `max_connections_per_ip` | integer | `0` | Maximum concurrent connections from one client address; `0` disables the limit. IPv6 clients are grouped by /64. Must not exceed `max_connections`. Set it low enough to stop one source exhausting `max_connections`, but account for clients behind NAT sharing one address. |
| `handshake_timeout_ms` | integer | `10000` | Timeout for SOCKS5 handshake. |
| `allow_unsafe_public_proxy` | bool | `false` | Allow a no-auth SOCKS listener on a non-loopback address. Startup fails closed unless this is set explicitly. |

### [server.resolver]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `timeout_ms` | integer | `5000` | DNS lookup timeout (1-60000). |
| `cache_ttl_secs` | integer | `30` | Positive-result cache TTL (1-86400). |
| `cache_max_entries` | integer | `4096` | Maximum cached entries (1-1000000). |
| `max_concurrent_lookups` | integer | `128` | Concurrent lookup limit (1-4096). |

### [server.tls]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Enable TLS wrapping for incoming connections. |
| `certificate_path` | string | none | Server certificate path. |
| `private_key_path` | string | none | Server private key path. |
| `key_password` | string | none | Password for encrypted private key. |
| `require_client_auth` | bool | `false` | Require client certificate authentication (mTLS). |
| `client_ca_path` | string | none | CA certificate to validate client certs. |
| `alpn_protocols` | array | `[]` | ALPN protocols (optional). |
| `min_protocol_version` | string | none | Minimum TLS version, e.g. `TLS13`. |

### [server.pool]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Enable outbound connection pooling. |
| `max_idle_per_dest` | integer | `4` | Max idle connections per destination. |
| `max_total_idle` | integer | `100` | Max idle connections across all destinations. |
| `idle_timeout_secs` | integer | `90` | Time before idle pooled connection expires. |
| `connect_timeout_ms` | integer | `5000` | Connect timeout for pool creates. |

## [auth]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `client_method` | string | `none` | Client authentication method: `none`, `pam.address`. |
| `socks_method` | string | `none` | SOCKS authentication method: `none`, `userpass`, `pam.address`, `pam.username`, `gssapi`. |

**Brute-force protection.** For `userpass` and `pam.username`, a source address that fails authentication 10 times within 60 seconds is locked out for the rest of that window: further attempts are rejected without checking credentials (and, for PAM, without calling PAM). A successful login clears the counter. Only genuine credential failures count; PAM or system faults do not. The tracking table is bounded (65,536 addresses). Failures are exported as `rustsocks_socks_auth_failures_total{method}`; see [Metrics & Audit Log](../guides/metrics-and-audit.md). Combine with `server.max_connections_per_ip` to limit parallel guessing from one address.

### [[auth.users]]

Used only when `socks_method = "userpass"`.

| Key | Type | Description |
| --- | --- | --- |
| `username` | string | Username for SOCKS auth. |
| `password` | string | Password for SOCKS auth. |

### [auth.pam]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `username_service` | string | `rustsocks` | PAM service for username authentication. |
| `address_service` | string | `rustsocks-client` | PAM service for address authentication. |
| `default_user` | string | `rhostusr` | Default user for PAM address-based auth. |
| `default_ruser` | string | `rhostusr` | Default remote user for PAM address-based auth. |
| `verbose` | bool | `false` | Enable PAM verbose logging. |
| `verify_service` | bool | `false` | Verify PAM service exists on startup. |

### [auth.gssapi]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `service_name` | string | `socks` | GSSAPI service name. |
| `keytab_path` | string | none | Optional keytab path. |
| `protection_level` | string | `integrity` | `integrity`, `confidentiality`, or `selective`. |
| `verbose` | bool | `false` | Enable GSSAPI verbose logging. |

**Note**: GSSAPI authentication requires building with the `gssapi` feature (e.g., `cargo build --release --features gssapi` or `--all-features`) and is supported on Unix systems.

## [logging]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `level` | string | `info` | Log level: `trace`, `debug`, `info`, `warn`, `error`. |
| `format` | string | `pretty` | `pretty` (human-readable lines) or `json` (one JSON object per line, for log pipelines). |

The level can also be given as a filter directive (for example `warn,rustsocks::server=info`). The `--log-level` command-line option overrides `logging.level`. Messages written while the configuration file is being read (before logging starts) are not logged. At `info`, every accepted connection is logged; use `warn` in production if that is too verbose.

## [acl]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Enable ACL enforcement. |
| `config_file` | string | none | Path to `acl.toml`. |
| `watch` | bool | `false` | Hot-reload ACL config on changes. |
| `anonymous_user` | string | `anonymous` | Fallback username for unauthenticated clients. |

## [sessions]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Enable session tracking. |
| `storage` | string | `memory` | `memory`, `sqlite`, `mariadb`, `mysql`. |
| `database_url` | string | none | DB URL required for `sqlite`/`mariadb`/`mysql` when sessions are enabled. |
| `batch_size` | integer | `100` | Batch size for session writes. |
| `batch_interval_ms` | integer | `1000` | Max wait before flush. |
| `retention_days` | integer | `90` | Retention for session history. |
| `cleanup_interval_hours` | integer | `24` | Cleanup cadence for session history. |
| `traffic_update_packet_interval` | integer | `10` | Update frequency based on packets. |
| `traffic_queue_capacity` | integer | `10000` | Buffer size for traffic updates. |
| `stats_window_hours` | integer | `24` | Window for dashboard stats. |
| `stats_api_enabled` | bool | `false` | Enable API + dashboard server. |
| `stats_api_bind_address` | string | `127.0.0.1` | API bind address. |
| `stats_api_port` | integer | `9090` | API port. |
| `api_token` | string | none | Optional token for `/api/*`. |
| `smtp_encryption_key` | string | none | Dedicated key used to encrypt SMTP passwords stored in the database; independent of `api_token`. |
| `batch_queue_capacity` | integer | `10000` | Hard bound on queued session batches; writes apply backpressure instead of growing during database outages. |
| `history_max_entries` | integer | `100000` | Maximum closed/rejected sessions kept in memory, in addition to time-based retention. |
| `swagger_enabled` | bool | `true` | Enable Swagger UI at `/swagger-ui/`. |
| `dashboard_enabled` | bool | `false` | Serve dashboard UI. |
| `base_path` | string | `/` | URL prefix for all routes. |

**Note**: Database-backed session storage (`sqlite`, `mariadb`, `mysql`) requires the `database` feature at build time.

### [sessions.dashboard_auth]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Enable dashboard Basic Auth. |
| `users` | array | `[]` | Users list (same fields as `auth.users`). |
| `altcha_enabled` | bool | `true` | Enable Altcha proof-of-work challenge. |
| `altcha_challenge_url` | string | none | External Altcha endpoint (optional). |
| `cookie_secure` | bool | `false` | Set cookies as Secure (HTTPS). |
| `session_secret` | string | auto | Random secret generated at startup if not set. |
| `session_duration_hours` | integer | `24` | Session lifetime for dashboard auth. |

## [policy_state]

Where dynamic-policy counters live: active connections, the 60-second connection rate and
daily/monthly transfer quotas used by [policies](policy-engine.md). Legacy ACL rules do not
use them.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `backend` | string | `memory` | `memory` keeps counters in the process (quotas reset on restart, each instance enforces its own limits). `redis` shares them between instances; requires a build with the `redis` feature. |
| `redis_url` | string | none | Required for `redis`, e.g. `redis://:${REDIS_PASSWORD}@redis:6379/0`. Supports `${ENV}` expansion. |
| `key_prefix` | string | `rustsocks` | Prefix for every Redis key, so several deployments can share one Redis. No whitespace or braces. |
| `failure_mode` | string | `fail_closed` | When Redis is unreachable for a policy that has limits: `fail_closed` denies the connection, `fail_open` falls back to this instance's local counters. Connections under policies without limits are always admitted. |
| `lease_secs` | integer | `60` | Lifetime of an active-connection lease (5-3600). A crashed instance's connections stop counting when their lease expires. |
| `operation_timeout_ms` | integer | `250` | Upper bound for one Redis operation on the connection path (10-10000). |

## [metrics]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Enable metrics collection. |
| `storage` | string | `memory` | `memory` or `sqlite` (uses the sessions database when available). |
| `retention_hours` | integer | `24` | How long to keep metrics history. |
| `cleanup_interval_hours` | integer | `6` | Cleanup cadence for metrics store. |
| `collection_interval_secs` | integer | `5` | Metrics sampling interval. |

**Notes**:
- The `/metrics` endpoint is served by the stats API server, so `sessions.stats_api_enabled` must be `true`.
- When `storage = "sqlite"`, metrics persistence requires sessions storage configured with a database URL.

## [telemetry]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Enable operational telemetry feed. |
| `max_events` | integer | `256` | Max events kept in memory. |
| `retention_hours` | integer | `6` | Age limit for telemetry events. |

## [qos]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Enable QoS / rate limiting. |
| `algorithm` | string | `htb` | QoS algorithm (`htb`). |

### [qos.htb]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `global_bandwidth_bytes_per_sec` | integer | `125000000` | Global bandwidth cap (1 Gbps). |
| `guaranteed_bandwidth_bytes_per_sec` | integer | `131072` | Guaranteed per-user minimum (1 Mbps). |
| `max_bandwidth_bytes_per_sec` | integer | `12500000` | Max per-user bandwidth (100 Mbps). |
| `burst_size_bytes` | integer | `1048576` | Burst size in bytes (1 MB). |
| `refill_interval_ms` | integer | `50` | Token refill interval. |
| `fair_sharing_enabled` | bool | `true` | Enable fair sharing between users. |
| `rebalance_interval_ms` | integer | `100` | Rebalance cadence. |
| `idle_timeout_secs` | integer | `5` | Idle threshold to mark user inactive. |

### [qos.connection_limits]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `max_connections_per_user` | integer | `20` | Per-user connection limit. |
| `max_connections_global` | integer | `10000` | Global connection limit. |

## Restart behavior

The dashboard "restart" action saves the configuration and exits with code `75`. Run RustSocks under a supervisor (systemd, Docker Compose, Kubernetes) configured to restart the process.

## CLI Overrides

- `--bind` overrides `[server].bind_address`
- `--port` overrides `[server].bind_port`
- `--log-level` overrides `[logging].level`

See `rustsocks --help` for the full CLI usage.
