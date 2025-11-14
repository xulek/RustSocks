# Web Dashboard Guide

**Implementation Status**: ✅ Complete

RustSocks includes a modern web-based admin dashboard built with React for real-time monitoring and management.

## Overview

The web dashboard provides:
- Real-time session monitoring
- ACL management and viewing
- User and group management
- Statistics and analytics
- Server health monitoring
- Operational telemetry feed for pool pressure and upstream errors
- Inline editing of `config/rustsocks.toml` with automatic restart
- API documentation (Swagger UI)

## Quick Start

### 1. Configuration

Enable dashboard in `rustsocks.toml`:

```toml
[sessions]
stats_api_enabled = true    # Enable API server
dashboard_enabled = true    # Enable web dashboard
swagger_enabled = true      # Enable Swagger UI
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
base_path = "/"            # URL base path (e.g., "/rustsocks")
```

### Optional Dashboard Authentication

Enable Basic Authentication for the dashboard to gate access:

```toml
[sessions.dashboard_auth]
enabled = true
[[sessions.dashboard_auth.users]]
username = "admin"
password = "strong-secret"
```

When enabled, the browser prompts for the configured credentials before the dashboard loads.

### 2. Build Dashboard

```bash
cd dashboard
npm install
npm run build
```

This creates the production build in `dashboard/dist/`.

### 3. Start Server

```bash
cargo build --release
./target/release/rustsocks --config config/rustsocks.toml
```

### 4. Access Dashboard

Open browser to: `http://127.0.0.1:9090/`

## Features

### Dashboard Page

**Real-time overview**:
- Active session count
- Total sessions (24h window)
- Total bandwidth (sent/received)
- Top users by bandwidth
- Top destinations by session count
- Auto-refresh every 5 seconds

### Sessions Page

**Live session monitoring**:
- Toggle between Active and History views
- Real-time updates for active sessions
- Session details: user, source, destination, protocol
- Traffic metrics: bytes/packets sent/received
- Duration and status
- Refresh button for manual updates

### ACL Rules Page

**Browse access control rules**:
- View groups and their rules
- View per-user rules
- Rule details: action, destinations, ports, protocols, priority
- Search/filter capabilities

### Users Page

**User management**:
- List all users
- View group memberships
- User statistics
- Search functionality

### Statistics Page

**Detailed analytics**:
- Customizable time windows (1h, 6h, 24h, 7d, 30d)
- Session count over time
- Bandwidth charts
- Top users ranking
- Top destinations ranking
- Export capabilities

### Telemetry Page

**Operational alerts**:
- Live stream of connection pool warnings and upstream connectivity failures
- Filters for lookback window, severity (info/warning/error), limit, and optional category
- Backed by the `/api/telemetry/events` endpoint for quick diagnostics when the pool hits limits

### Configuration Page

**Server information**:
- Server health status
- Uptime
- Version information
- Configuration summary
- Link to Swagger API documentation
- Runtime config editor: Forms are split into modules (Server, Sessions, Pool, Metrics, Telemetry). Every change is validated exactly like process startup, writes `config/rustsocks.toml` atomically, and can optionally trigger a controlled restart without logging into the host. When the server runs without a configuration file, this section becomes read-only.

## Development

### Setup

```bash
cd dashboard
npm install
```

### Development Server

```bash
npm run dev
```

This starts:
- Vite dev server on `http://localhost:3000`
- Proxy to API on `http://127.0.0.1:9090`

**Benefits**:
- Hot module replacement (HMR)
- Fast refresh
- Instant feedback

### Project Structure

```
dashboard/
├── src/
│   ├── components/         # Reusable UI components
│   │   ├── SessionDetailDrawer.jsx
│   │   ├── SystemResources.jsx
│   │   └── UserDetailModal.jsx
│   ├── pages/              # Route-level views
│   │   ├── Dashboard.jsx
│   │   ├── Sessions.jsx
│   │   ├── AclRules.jsx
│   │   ├── UserManagement.jsx
│   │   ├── Statistics.jsx
│   │   ├── Configuration.jsx
│   │   ├── Telemetry.jsx
│   │   ├── Diagnostics.jsx
│   │   └── Login.jsx
│   ├── App.jsx             # Main app component
│   ├── App.css             # Global styles
│   └── main.jsx            # Entry point
├── public/                 # Static assets
├── vite.config.js          # Vite configuration
├── package.json            # Dependencies
└── README.md               # Dashboard docs
```

### Building for Production

```bash
npm run build
```

Output in `dashboard/dist/` is served automatically by RustSocks.

## URL Base Path Support

Deploy dashboard under custom URL prefix:

```toml
[sessions]
base_path = "/rustsocks"
```

### How It Works

1. **Backend**: Nests all routes under prefix
   - API: `http://host:port/rustsocks/api/`
   - Dashboard: `http://host:port/rustsocks/`
   - Swagger: `http://host:port/rustsocks/swagger-ui/`

2. **Frontend**: Auto-detects base path
   - Reads `window.__RUSTSOCKS_BASE_PATH__` injected by backend
   - React Router uses `basename` for routing
   - API calls use `getApiUrl()` helper

### Example Configurations

**Root path** (default):
```toml
base_path = "/"
```
- Dashboard: `http://127.0.0.1:9090/`
- API: `http://127.0.0.1:9090/api/`

**Custom path**:
```toml
base_path = "/rustsocks"
```
- Dashboard: `http://127.0.0.1:9090/rustsocks`
- API: `http://127.0.0.1:9090/rustsocks/api/`

## Reverse Proxy Setup

### Nginx Configuration

```nginx
server {
    listen 80;
    server_name proxy.example.com;

    location /rustsocks {
        proxy_pass http://127.0.0.1:9090;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;

        # For WebSocket support (future)
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

**RustSocks config**:
```toml
[sessions]
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
base_path = "/rustsocks"
```

Access: `http://proxy.example.com/rustsocks`

### Apache Configuration

```apache
<VirtualHost *:80>
    ServerName proxy.example.com

    ProxyPreserveHost On
    ProxyPass /rustsocks http://127.0.0.1:9090/rustsocks
    ProxyPassReverse /rustsocks http://127.0.0.1:9090/rustsocks

    <Location /rustsocks>
        Require all granted
    </Location>
</VirtualHost>
```

## Security

### Important Considerations

⚠️ **The dashboard is for administrative use only**

1. **Authentication**: Not built-in (future enhancement)
   - Deploy behind VPN
   - Use reverse proxy with authentication
   - Restrict to trusted networks

2. **Network Exposure**:
   - Bind to `127.0.0.1` by default (localhost only)
   - Use `0.0.0.0` only behind firewall/VPN
   - Never expose directly to internet

3. **API Security**:
   - No authentication on API endpoints (yet)
   - Consider API tokens (future enhancement)
   - Rate limiting recommended

### Production Deployment

**Recommended setup**:
```toml
[sessions]
stats_api_bind_address = "127.0.0.1"  # Localhost only
stats_api_port = 9090
dashboard_enabled = true
swagger_enabled = false  # Disable in production
```

**Access via SSH tunnel**:
```bash
ssh -L 9090:127.0.0.1:9090 user@server
# Access locally: http://127.0.0.1:9090/
```

**Or use reverse proxy with authentication** (nginx basic auth, OAuth, etc.)

## Tech Stack

- **React 18**: Modern React with hooks
- **Vite**: Fast build tool and dev server
- **React Router**: Client-side routing
- **Lucide React**: Icon library
- **Vanilla CSS**: No framework dependencies

### Why These Choices?

- **React**: Industry standard, excellent ecosystem
- **Vite**: Fast builds, excellent DX
- **No CSS framework**: Reduces bundle size, full control
- **Minimal dependencies**: Faster builds, smaller bundle

## API Integration

Dashboard uses REST API endpoints:

### Session Endpoints

```
GET /api/sessions/active          # List active sessions
GET /api/sessions/history         # List completed sessions
GET /api/sessions/stats           # Aggregated statistics
GET /api/sessions/:id             # Get session details
```

### ACL Endpoints

```
GET /api/acl/groups               # List ACL groups
GET /api/acl/users                # List users with rules
GET /api/acl/stats                # ACL statistics
```

### Health Endpoint

```
GET /api/health                   # Server health check
```

### Metrics Endpoint

```
GET /metrics                      # Prometheus metrics
```

See Swagger UI for complete API documentation.

## Troubleshooting

### Dashboard not loading

**Problem**: 404 error when accessing dashboard

**Causes**:
- Dashboard not built (`npm run build` not executed)
- `dashboard_enabled = false` in config
- `dashboard/dist/` directory missing

**Solutions**:
```bash
cd dashboard
npm run build
# Verify dist/ directory exists
ls -la dist/
```

### API requests failing

**Problem**: CORS errors or connection refused

**Causes**:
- API server not running
- Wrong port/address in config
- CORS issues in development

**Solutions**:
- Verify API server is running: `curl http://127.0.0.1:9090/api/health`
- Check config: `stats_api_enabled = true`
- Development: Vite proxy should handle CORS

### Base path not working

**Problem**: 404 on custom base path

**Causes**:
- Mismatched base_path in config and nginx
- Trailing slash issues
- Dashboard not rebuilt after config change

**Solutions**:
- Ensure `base_path` in config matches nginx location
- Rebuild dashboard: `cd dashboard && npm run build`
- Check both config and nginx trailing slashes

### Styles not loading

**Problem**: Unstyled dashboard (HTML only)

**Causes**:
- CSS files not found
- Incorrect base path
- Build artifacts missing

**Solutions**:
- Rebuild dashboard: `npm run build`
- Check browser console for 404 errors
- Verify `base_path` configuration

## Performance

### Bundle Size

Production build:
- JavaScript: ~200KB (gzipped: ~70KB)
- CSS: ~50KB (gzipped: ~10KB)
- Total: ~250KB (~80KB transferred)

### Load Time

Typical load times on localhost:
- Initial load: <500ms
- Subsequent visits: <100ms (cached)
- API calls: 10-50ms

### Optimization Tips

1. **Enable gzip compression** in nginx/Apache
2. **Set cache headers** for static assets
3. **Use CDN** for production deployments
4. **Enable HTTP/2** for multiplexing

## Future Enhancements

Planned features:
- [ ] Built-in authentication (JWT, OAuth)
- [ ] WebSocket for real-time updates
- [ ] Advanced filtering and search
- [ ] Export to CSV/JSON
- [ ] Dark/light theme toggle
- [ ] Mobile-responsive design improvements
- [ ] ACL rule editing interface
- [ ] User management (add/remove users)

## Related Documentation

- [Session Management](../technical/session-management.md)
- [API Documentation](building-with-base-path.md)
- [Testing Guide](testing.md)
- Dashboard README: `dashboard/README.md`
