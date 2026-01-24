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

### Users

User and group management:

- List users and group membership
- View per-user statistics
- Manage group assignments and per-user rules

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

System and server health at a glance:

- Health check status
- Process and system resource metrics
- Quick validation of service state

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

### Login

If dashboard authentication is enabled, a login screen prompts for credentials configured in `sessions.dashboard_auth`.

## Security Notes

- The dashboard can be protected with Basic Auth and optional Altcha.
- For production, bind the API to localhost or protect it behind a reverse proxy.

See [Dashboard Authentication](../guides/dashboard-authentication.md) for details.
