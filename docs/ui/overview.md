# Dashboard Overview

RustSocks ships with a modern, single-page admin dashboard served directly by the Rust backend. It is designed for operators who need fast insight and immediate control.

## Enable the UI

```toml
[sessions]
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
base_path = "/"
api_token = "change-me"   # or enable [sessions.dashboard_auth]; /api/* is rejected without one of them
```

Build the frontend once:

```bash
cd dashboard
npm install
npm run build
```

Open: `http://127.0.0.1:9090/`

## Page-by-Page Guide

### Dashboard

A live operational overview with:

- Active sessions and total session counts
- Bandwidth totals (sent/received)
- Top users by bandwidth
- Top destinations by session count
- System resources (CPU and memory, system-wide and for the RustSocks process) and connection pool statistics
- Auto-refresh every few seconds

### Sessions

Live and historical session visibility:

- Toggle Active vs History
- Filter by user, destination, protocol, status
- View session details (source, destination, bytes, packets, duration)
- Export visible rows to CSV
- Terminate active sessions

### ACL Rules

Manage access control lists visually:

- Browse groups and per-user rules
- See destinations, ports, protocols, priorities
- Create, edit, delete ACL rules
- Immediate effect when ACL watching is enabled

### Access Policies

Manage dynamic policies (see the [policy engine guide](../config/policy-engine.md)):

- Create, edit and delete policies: subjects (users and groups), destinations, ports, protocols and priority
- Choose `enforce` or `monitor` mode and enable or disable each policy
- Add conditions: source networks, authentication method, weekday and time schedule, validity window, connection limits and daily/monthly transfer quotas
- **Explain simulator:** enter a user, destination, source IP, authentication method and time, and see the decision with a step-by-step trace of every rule and policy considered

### Users

User and group management:

- List users and group membership
- View per-user statistics
- Create and delete users, manage group assignments and per-user rules

### Statistics

Historical analytics and trends:

- Time windows (1h, 6h, 24h, 7d, 30d)
- Session count charts
- Bandwidth charts
- Top users and top destinations

### Telemetry

Operational telemetry stream:

- Pool pressure and upstream failures
- Filter by lookback window, severity, category
- Useful for quick troubleshooting

### Diagnostics

Ad-hoc connectivity checks from the server (administrators only):

- Test whether the server can open a TCP connection to an IP address and port
- Choose the timeout and see the result and latency
- Useful for telling a firewall or routing problem from an ACL decision

### Configuration

Runtime configuration editor for key modules:

- Server: bind address, port, limits
- Sessions & API: storage, dashboard, API settings
- Connection pool: pooling strategy and timeouts
- Metrics & telemetry: retention and collection

Runtime fields exposed in the UI:

- **Global ACL**: default policy (allow/block)
- **Server**: bind address, port, dashboard enabled, Swagger enabled, stats API enabled, stats API bind address, stats API port
- **Sessions**: storage, database URL, retention days, cleanup interval, traffic update packet interval, stats window, base path
- **Pool**: enabled, max idle per destination, max total idle, idle timeout, connect timeout
- **Metrics**: enabled, storage, retention hours, cleanup interval, collection interval
- **Telemetry**: enabled, max events, retention hours

Changes are validated and written atomically to `config/rustsocks.toml` when a config file is in use. When no config file is active, the editor becomes read-only.

### SMTP

Configure e-mail notifications: connection mode and server, sender, credentials, recipients, the alert types to send, cooldowns, and a test e-mail. See the [SMTP guide](../guides/smtp-configuration.md).

### Login

If dashboard authentication is enabled, a login screen prompts for credentials configured in `sessions.dashboard_auth`. What each user can do depends on their role:

| Role | Can do |
| --- | --- |
| `viewer` | Read sessions, statistics, telemetry, metrics and ACL rules and policies (default for users without an assigned role) |
| `operator` | Everything a viewer can, plus terminate sessions and acknowledge alerts |
| `admin` | Everything, including ACL and policy changes, and the only role with access to the configuration (`/api/admin/*`), SMTP and diagnostics pages |

## API documentation

Swagger UI is served at `/swagger-ui/` and the OpenAPI document at `/openapi.json` when `swagger_enabled = true`.

## Security Notes

- `/api/*` fails closed: requests are rejected unless dashboard authentication or `sessions.api_token` is configured.
- Dashboard logins are protected by a per-user and per-address lockout and optional Altcha proof-of-work.
- Every state-changing request is written to the audit log; see [Metrics & Audit Log](../guides/metrics-and-audit.md).
- For production, bind the API to localhost or protect it behind a reverse proxy with TLS.

See [Dashboard Authentication](../guides/dashboard-authentication.md) for details.
