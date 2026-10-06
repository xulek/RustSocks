# Metrics and Audit Log

## Prometheus metrics (`GET /metrics`)

Available when built with the `metrics` feature (default). All series are exported with zero
values from the first scrape. No metric uses a per-user label, because usernames can be
attacker controlled and would create unbounded cardinality.

| Metric | Type | Labels | Description |
| --- | --- | --- | --- |
| `rustsocks_active_sessions` | gauge | none | Currently active sessions. |
| `rustsocks_sessions_total` | counter | none | Accepted sessions since start. |
| `rustsocks_sessions_rejected_total` | counter | none | Sessions rejected (for example by ACL). |
| `rustsocks_session_duration_seconds` | histogram | none | Session duration. |
| `rustsocks_bytes_sent_total` / `rustsocks_bytes_received_total` | counter | none | Proxied traffic. |
| `rustsocks_qos_active_users` | gauge | none | Users with at least one QoS-managed connection. |
| `rustsocks_qos_bandwidth_allocated_bytes_total` | counter | `direction` (`upload`, `download`) | Bytes allocated by the QoS engine. |
| `rustsocks_qos_allocation_wait_seconds` | histogram | none | Time spent throttling traffic for QoS allocations. |
| `rustsocks_acl_decisions_total` | counter | `decision` (`allow`, `block`), `source` | Primary decision per request. `source` is `legacy_acl`, `policy`, `default` or `post_dns` (block after DNS resolution hit an explicit block rule). |
| `rustsocks_policy_monitor_matches_total` | counter | none | Monitor-mode policies that would have applied to a request. Use it to judge a policy before switching it to `enforce`. |
| `rustsocks_policy_admission_denied_total` | counter | `reason` (`active_connections`, `connection_rate`, `daily_quota`, `monthly_quota`) | Sessions denied by policy admission limits. |
| `rustsocks_socks_auth_failures_total` | counter | `method` (`none`, `userpass`, `pam.address`, `pam.username`, `gssapi`) | Failed SOCKS client authentication attempts. |
| `rustsocks_policy_usage_store_errors_total` | counter | `op` (`reserve`, `snapshot`, `release`, `heartbeat`, `flush`) | Failed operations against the shared (Redis) policy usage store. Non-zero means limits are degraded; see `policy_state.failure_mode`. |
| `rustsocks_policy_evaluation_seconds` | histogram | none | Time to evaluate ACL rules and policies for one request. |

Useful queries:

```promql
# Block ratio per deciding source
sum by (source) (rate(rustsocks_acl_decisions_total{decision="block"}[5m]))

# Brute-force indicator
sum(rate(rustsocks_socks_auth_failures_total[5m]))

# p99 evaluation latency
histogram_quantile(0.99, sum by (le) (rate(rustsocks_policy_evaluation_seconds_bucket[5m])))
```

## Audit log

Every state-changing API request (`POST`, `PUT`, `PATCH`, `DELETE`) is logged with the tracing
target `audit`, so it can be routed or filtered separately:

```bash
RUST_LOG=info,audit=info rustsocks --config config/rustsocks.toml
```

| Field | Meaning |
| --- | --- |
| `outcome` | `completed` (request was executed), `forbidden` (RBAC denied) or `unauthenticated`. |
| `actor` | Dashboard username, or `api-token` for the static API token. |
| `role` / `required_role` | Granted role, and the role the route requires (on `forbidden`). |
| `method`, `path` | Request method and path. |
| `status` | HTTP status returned (on `completed`). |
| `remote` | Client IP address. |

Request bodies and tokens are never written to the audit log. Read-only requests (`GET`,
`HEAD`, `OPTIONS`) are not audited. Policy and ACL changes made through the API are therefore
attributable to a user, while the change content itself is available from the saved ACL file
and its backups.
