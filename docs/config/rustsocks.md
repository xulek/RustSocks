# rustsocks.toml Reference

This document describes every supported option in the main configuration file.

RustSocks loads configuration from a TOML file passed via `--config`. When no file is provided, defaults are used.

## File Structure

The file is grouped into blocks:

- `[server]` core listener settings
- `[server.tls]` TLS configuration
- `[server.pool]` connection pooling
- `[auth]` authentication strategy
- `[logging]` log format and level
- `[acl]` ACL system and hot reload
- `[sessions]` session tracking + API/dashboard
- `[sessions.dashboard_auth]` dashboard authentication
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
| `handshake_timeout_ms` | integer | `10000` | Timeout for SOCKS5 handshake. |

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
| `format` | string | `pretty` | `pretty` or `json`. |

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
| `api_token` | string | none | Optional token for `/api/*`; also required to encrypt SMTP passwords stored in the database. |
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

## CLI Overrides

- `--bind` overrides `[server].bind_address`
- `--port` overrides `[server].bind_port`
- `--log-level` overrides `[logging].level`

See `rustsocks --help` for the full CLI usage.
