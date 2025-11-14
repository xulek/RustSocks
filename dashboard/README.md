# RustSocks Admin Dashboard

Modern web-based admin dashboard for RustSocks SOCKS5 proxy server.

## Features

- **Real-time Session Monitoring**: View active and historical SOCKS5 sessions with live updates
- **Advanced Session Tools**: Filter historical traffic, paginate results, export CSV, and inspect detailed session metadata
- **Live Telemetry Charts**: Interactive charts showing session counts and bandwidth with auto-refresh every few seconds
- **ACL Management**: Browse and view Access Control List rules for groups and users
- **ACL Toolbox**: Test ACL decisions and trigger live reloads without restarting the service
- **User Management**: Manage users and their group memberships
- **Statistics Dashboard**: Detailed analytics including bandwidth usage, top users, and destinations
- **System Health Overview**: Inline health status with quick navigation shortcuts
- **Configuration View**: Server health status and API endpoint documentation
- **Clean, Modern UI**: Dark theme with intuitive navigation

## Tech Stack

- **React 18**: Modern React with hooks
- **Vite**: Lightning-fast build tool and dev server
- **React Router**: Client-side routing
- **Lucide React**: Beautiful icons
- **Vanilla CSS**: No framework overhead, custom design

## Quick Start

### Prerequisites

- Node.js 18+ and npm
- RustSocks server running with API enabled

### Installation

```bash
cd dashboard
npm install
```

### Development

```bash
npm run dev
```

Dashboard will be available at `http://localhost:3000` with API proxy to `http://127.0.0.1:9090`.

### Production Build

```bash
npm run build
```

Built files will be in `dashboard/dist/` directory.

### Testing

```bash
npm test
```

Runs the Vitest-powered unit test suite (uses jsdom + Testing Library).

> **Note:** When deploying with a URL base path (e.g., `/rustsocks`), see [Building with Base Path Guide](../docs/guides/building-with-base-path.md) for complete instructions.

## Configuration

### Enable Dashboard in RustSocks

Edit `rustsocks.toml`:

```toml
[sessions]
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
base_path = "/"  # Options: "/" or "/rustsocks" or any custom path
```

### Dashboard Authentication

You can require HTTP Basic Authentication before the UI loads:

```toml
[sessions.dashboard_auth]
enabled = true
[[sessions.dashboard_auth.users]]
username = "admin"
password = "strong-secret"
```

The dashboard will prompt for the configured credentials when auth is enabled.

### Serve Dashboard from RustSocks

The dashboard is served automatically when `dashboard_enabled = true` and the `dashboard/dist/` directory exists.

**URL Base Path Support:**
- `base_path = "/"` - Dashboard at `http://host:9090/`
- `base_path = "/rustsocks"` - Dashboard at `http://host:9090/rustsocks`

For detailed instructions on building and deploying with custom base paths, see [Building with Base Path Guide](../docs/guides/building-with-base-path.md).

## Dashboard Pages

### 1. Dashboard (Home)
- Active/total session counters with real-time updates
- Inline health status (status, version, uptime)
- Quick actions in top tables to jump into filtered session history
- Live charts for session counts and bandwidth growth
- Total bandwidth tracker
- Refreshes automatically every 5 seconds

### 2. Sessions
- Real-time session monitoring
- Toggle between active sessions and history
- Flexible history filters (user, destination, status, time window)
- Pagination controls and CSV export for current view
- Detailed session drawer with ACL and transfer metadata
- Auto-refresh every 3 seconds for active view (manual + filtered history loads)

### 3. ACL Rules
- Browse ACL groups and their rules
- View user ACL configurations
- Rule details: action, destinations, ports, protocols, priority
- Group membership visualization
- ACL toolbox to test decisions and reload configuration live

### 4. Users
- List all users with ACL rules
- Group memberships
- Rule count per user

### 5. Statistics
- Aggregated session statistics
- Bandwidth by user
- Top destinations
- Session success/failure rates

### 6. Configuration
- Server health status and uptime
- API endpoint documentation
- Configuration instructions

## API Integration

The dashboard consumes the following RustSocks API endpoints:

- `GET /health` - Server health check
- `GET /api/sessions/active` - Active sessions
- `GET /api/sessions/history` - Session history
- `GET /api/sessions/stats` - Aggregated statistics
- `GET /api/acl/groups` - List ACL groups
- `GET /api/acl/groups/{name}` - Group details with rules
- `GET /api/acl/users` - List users with ACL

All API calls use relative paths and are proxied during development.

## Customization

### Colors

Edit `src/index.css` CSS variables:

```css
:root {
  --primary: #2563eb;
  --success: #10b981;
  --danger: #ef4444;
  --bg-dark: #0f172a;
  --bg-light: #1e293b;
  /* ... */
}
```

### Refresh Intervals

Edit refresh intervals in component files:
- Dashboard: 5000ms
- Sessions: 3000ms

## Security Notes

- The dashboard is for **administrative use only**
- Deploy behind authentication/VPN in production
- API endpoints should be secured with tokens (future feature)
- Do not expose dashboard to public internet

## Future Enhancements

- [ ] Real-time WebSocket updates
- [ ] Advanced ACL rule editor (add/edit/delete)
- [ ] User creation and management
- [ ] Session termination controls
- [ ] Traffic graphs with Recharts
- [ ] Export statistics (CSV/JSON)
- [ ] Authentication/authorization
- [ ] Dark/light theme toggle

## Troubleshooting

### Dashboard shows "Failed to fetch"

- Ensure RustSocks server is running with `stats_api_enabled = true`
- Check API is accessible at `http://127.0.0.1:9090`
- Verify proxy configuration in `vite.config.js`

### Empty data on dashboard

- Create some SOCKS5 connections to generate session data
- Enable ACL to see rules
- Check server logs for errors

## License

Part of the RustSocks project.
