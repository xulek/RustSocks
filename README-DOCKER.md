# RustSocks Docker Deployment Guide

Complete guide for deploying RustSocks SOCKS5 proxy using Docker.

## Features

- ✅ Multi-stage build (Node.js + Rust + Alpine runtime)
- ✅ Web dashboard included
- ✅ PAM authentication support
- ✅ SQLite session database with persistence
- ✅ Connection pooling & optimization
- ✅ Prometheus metrics & Swagger API
- ✅ Non-root user (security)
- ✅ ~30-40MB final image size

---

## Quick Start

### 1. Build the Image

```bash
# Clone repository (if not already)
git clone https://github.com/your-org/rustsocks.git
cd rustsocks

# Build Docker image
docker build -t rustsocks:latest .
```

Build takes ~5-10 minutes (first time, with caching).

### 2. Run with Docker Compose

```bash
# Start the service
docker-compose up -d

# Check logs
docker-compose logs -f rustsocks

# Stop the service
docker-compose down
```

### 3. Access the Dashboard

Open your browser:
- **Dashboard**: http://localhost:9090/
- **API Docs (Swagger)**: http://localhost:9090/swagger-ui/
- **Health Check**: http://localhost:9090/health

### 4. Test SOCKS5 Proxy

```bash
# Test with curl
curl -x socks5://localhost:1080 http://example.com

# Test with authentication (if enabled)
curl -x socks5://alice:secret123@localhost:1080 http://example.com

# Test with SSH
ssh -o ProxyCommand='nc -X 5 -x 127.0.0.1:1080 %h %p' user@remote-host
```

---

## Configuration

### Using Custom Configuration

#### Option 1: Edit docker/configs/rustsocks.docker.toml

Edit the provided template:

```bash
# Edit the config file
nano docker/configs/rustsocks.docker.toml

# Restart container to apply changes
docker-compose restart rustsocks
```

#### Option 2: Mount Your Own Config

Create your own `rustsocks.toml` and mount it:

```yaml
# docker-compose.override.yml
services:
  rustsocks:
    volumes:
      - ./my-rustsocks.toml:/etc/rustsocks/rustsocks.toml:ro
```

Then run:
```bash
docker-compose up -d
```

### Environment Variables

Override configuration via environment variables:

```yaml
# docker-compose.yml (or .env file)
services:
  rustsocks:
    environment:
      - RUST_LOG=debug
      - RUSTSOCKS_BIND_PORT=1080
      - RUSTSOCKS_API_PORT=9090
      - RUSTSOCKS_DB_PATH=/data/sessions.db
```

Available variables:
- `RUST_LOG` - Log level (trace, debug, info, warn, error)
- `RUSTSOCKS_CONFIG` - Config file path (default: /etc/rustsocks/rustsocks.toml)
- `RUSTSOCKS_BIND_ADDRESS` - SOCKS bind address (default: 0.0.0.0)
- `RUSTSOCKS_BIND_PORT` - SOCKS port (default: 1080)
- `RUSTSOCKS_API_PORT` - API/Dashboard port (default: 9090)
- `RUSTSOCKS_DB_PATH` - Database path (default: /data/sessions.db)

---

## Authentication

### No Authentication (Default)

```toml
# docker/configs/rustsocks.docker.toml
[auth]
client_method = "none"
socks_method = "none"
```

### Username/Password Authentication

Enable in config:

```toml
[auth]
socks_method = "userpass"

[[auth.users]]
username = "alice"
password = "secret123"

[[auth.users]]
username = "bob"
password = "password456"
```

Test:
```bash
curl -x socks5://alice:secret123@localhost:1080 http://example.com
```

### PAM Authentication

#### PAM Username Authentication

Authenticate using system users:

```toml
[auth]
socks_method = "pam.username"

[auth.pam]
username_service = "rustsocks"
```

PAM configuration: `docker/configs/pam.d/rustsocks`

#### PAM Address Authentication

Authenticate based on IP address:

```toml
[auth]
client_method = "pam.address"

[auth.pam]
address_service = "rustsocks-client"
default_user = "anonymous"
```

PAM configuration: `docker/configs/pam.d/rustsocks-client`

---

## Access Control Lists (ACL)

### Enable ACL

```toml
# docker/configs/rustsocks.docker.toml
[acl]
enabled = true
config_file = "/etc/rustsocks/acl.toml"
watch = true  # Enable hot-reload
```

### Example ACL Rules

See `docker/configs/acl.docker.toml` for a complete example.

**Per-user rules:**
```toml
[[users]]
username = "alice"
groups = ["admins"]

  [[users.rules]]
  action = "allow"
  destinations = ["*"]
  ports = ["*"]
  priority = 100
```

**Block specific destinations:**
```toml
[[users.rules]]
action = "block"
description = "Block social media"
destinations = ["*.facebook.com", "*.twitter.com", "*.tiktok.com"]
ports = ["*"]
priority = 500
```

**Allow only specific networks:**
```toml
[[users.rules]]
action = "allow"
destinations = ["10.0.0.0/8", "*.company.com"]
ports = ["80", "443"]
priority = 100
```

### Hot Reload

When `watch = true`, ACL changes are applied automatically:

```bash
# Edit ACL config
nano docker/configs/acl.docker.toml

# Changes applied within 1-2 seconds (no restart needed)
```

---

## Volumes and Persistence

### Volume Mounts

The docker-compose.yml defines these volumes:

| Volume | Purpose | Important Data |
|--------|---------|----------------|
| `/etc/rustsocks/` | Configuration files | rustsocks.toml, acl.toml |
| `/etc/pam.d/` | PAM configs | PAM service files |
| `/data/` | Database storage | sessions.db (SQLite) |
| `/var/log/rustsocks/` | Logs (optional) | Application logs |

### Backup Database

```bash
# Create backup
docker-compose exec rustsocks sqlite3 /data/sessions.db ".backup /data/sessions-backup.db"

# Copy to host
docker cp rustsocks-proxy:/data/sessions.db ./sessions-backup.db

# Restore backup
docker cp ./sessions-backup.db rustsocks-proxy:/data/sessions.db
docker-compose restart rustsocks
```

### Inspect Database

```bash
# Open SQLite shell
docker-compose exec rustsocks sqlite3 /data/sessions.db

# Example queries:
sqlite> SELECT COUNT(*) FROM sessions;
sqlite> SELECT user, COUNT(*) FROM sessions GROUP BY user;
sqlite> SELECT * FROM sessions WHERE status = 'active';
sqlite> .exit
```

---

## Monitoring & Metrics

### Dashboard

Access the web dashboard at: http://localhost:9090/

Features:
- Real-time session monitoring
- ACL rules viewer
- User management
- Statistics & analytics
- API documentation

### API Endpoints

```bash
# Health check
curl http://localhost:9090/health

# Session statistics
curl http://localhost:9090/rustsocks/api/sessions/stats

# Connection pool stats
curl http://localhost:9090/rustsocks/api/pool/stats

# Active sessions
curl http://localhost:9090/rustsocks/api/sessions/active

# ACL statistics
curl http://localhost:9090/rustsocks/api/acl/stats
```

### Swagger API Documentation

Interactive API docs: http://localhost:9090/swagger-ui/

### Prometheus Metrics

Prometheus-compatible metrics endpoint:

```bash
curl http://localhost:9090/metrics
```

Example metrics:
- `rustsocks_active_sessions` - Current active sessions
- `rustsocks_sessions_total` - Total sessions accepted
- `rustsocks_bytes_sent_total` - Total bytes sent
- `rustsocks_session_duration_seconds` - Session duration histogram

---

## TLS/SSL Configuration

### Enable SOCKS over TLS

Generate certificates:

```bash
# Generate self-signed certificate (testing only)
openssl req -x509 -newkey rsa:4096 -keyout server.key -out server.crt -days 365 -nodes

# For production: Use certificates from Let's Encrypt or your CA
```

Configure RustSocks:

```toml
# docker/configs/rustsocks.docker.toml
[server.tls]
enabled = true
certificate_path = "/etc/rustsocks/tls/server.crt"
private_key_path = "/etc/rustsocks/tls/server.key"
min_protocol_version = "TLS13"
```

Mount certificates:

```yaml
# docker-compose.yml
services:
  rustsocks:
    volumes:
      - ./certs:/etc/rustsocks/tls:ro
```

### Mutual TLS (mTLS)

For client certificate authentication:

```toml
[server.tls]
enabled = true
require_client_auth = true
client_ca_path = "/etc/rustsocks/tls/clients-ca.crt"
```

---

## Performance Tuning

### Connection Pooling

Enable connection pooling for better performance:

```toml
[server.pool]
enabled = true
max_idle_per_dest = 4
max_total_idle = 100
idle_timeout_secs = 90
```

Verify pooling works:

```bash
# Check pool stats
curl http://localhost:9090/rustsocks/api/pool/stats | jq

# Look for:
# - pool_hits > 0 (connections reused)
# - hit_rate > 0% (percentage of reuses)
```

### QoS & Rate Limiting

Limit bandwidth and connections per user:

```toml
[qos]
enabled = true

[[qos.users]]
username = "alice"
max_bandwidth_mbps = 10
max_connections = 5
max_new_connections_per_minute = 30
```

### Database Performance

Tune batch writer for your workload:

```toml
[sessions]
batch_size = 100  # Increase for high-volume
batch_interval_ms = 1000  # Decrease for real-time updates
```

### Resource Limits

Set Docker resource limits:

```yaml
# docker-compose.yml
services:
  rustsocks:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 512M
        reservations:
          cpus: '0.5'
          memory: 128M
```

---

## Troubleshooting

### Check Logs

```bash
# Follow logs in real-time
docker-compose logs -f rustsocks

# Last 100 lines
docker-compose logs --tail=100 rustsocks

# With timestamps
docker-compose logs -f -t rustsocks
```

### Enable Debug Logging

```yaml
# docker-compose.yml
services:
  rustsocks:
    environment:
      - RUST_LOG=debug
```

Or start manually:
```bash
docker run -e RUST_LOG=debug rustsocks:latest
```

### Verify Configuration

```bash
# Check config syntax
docker-compose exec rustsocks cat /etc/rustsocks/rustsocks.toml

# Validate TOML
docker-compose exec rustsocks rustsocks --config /etc/rustsocks/rustsocks.toml --validate
```

### Connection Issues

**Problem: Cannot connect to SOCKS proxy**

Check if service is running:
```bash
docker-compose ps
curl http://localhost:9090/health
```

Check firewall:
```bash
# Test from host
curl -x socks5://localhost:1080 http://example.com

# Test from container
docker-compose exec rustsocks nc -zv localhost 1080
```

**Problem: Authentication fails**

Check PAM configuration:
```bash
# View PAM config
docker-compose exec rustsocks cat /etc/pam.d/rustsocks

# Check logs for auth errors
docker-compose logs rustsocks | grep -i auth
```

**Problem: Dashboard not accessible**

Verify dashboard is enabled:
```bash
# Check config
docker-compose exec rustsocks grep dashboard_enabled /etc/rustsocks/rustsocks.toml

# Should return: dashboard_enabled = true
```

Verify files exist:
```bash
docker-compose exec rustsocks ls -la /app/dashboard/dist/
```

### Database Issues

**Problem: Database locked**

```bash
# Stop service
docker-compose stop rustsocks

# Remove lock files
docker-compose exec rustsocks rm -f /data/sessions.db-shm /data/sessions.db-wal

# Restart
docker-compose start rustsocks
```

**Problem: Database corruption**

```bash
# Backup current database
docker cp rustsocks-proxy:/data/sessions.db ./sessions-corrupted.db

# Try to recover
sqlite3 sessions-corrupted.db ".recover" > recovered.sql

# Remove old database and let RustSocks create new one
docker-compose exec rustsocks rm /data/sessions.db
docker-compose restart rustsocks
```

---

## Security Best Practices

### 1. Use Non-Root User

✅ Already configured in Dockerfile (user: rustsocks, UID 1000)

### 2. Enable Authentication

Don't run without authentication in production:

```toml
[auth]
socks_method = "userpass"  # or "pam.username"
```

### 3. Configure ACL

Use ACL to restrict access:

```toml
[acl]
enabled = true
```

### 4. Use TLS/SSL

Encrypt SOCKS traffic:

```toml
[server.tls]
enabled = true
min_protocol_version = "TLS13"
```

### 5. Network Isolation

Use Docker networks:

```yaml
networks:
  frontend:
    external: true
  backend:
    internal: true  # No internet access
```

### 6. Secure PAM Configuration

For production PAM, remove `nullok`:

```pam
# docker/configs/pam.d/rustsocks
auth required pam_unix.so  # Remove nullok in production
```

### 7. Keep Images Updated

```bash
# Rebuild with latest base images
docker-compose build --pull --no-cache rustsocks

# Update and restart
docker-compose up -d
```

### 8. Rotate Secrets

Regularly update:
- User passwords
- TLS certificates
- Database encryption (if enabled)

### 9. Monitor Access

Enable session tracking and review logs:

```bash
# Check recent sessions
docker-compose exec rustsocks sqlite3 /data/sessions.db \
  "SELECT user, dest_ip, dest_port, start_time FROM sessions ORDER BY start_time DESC LIMIT 20;"
```

### 10. Limit Exposure

Only expose necessary ports:

```yaml
# docker-compose.yml
ports:
  - "127.0.0.1:1080:1080"  # Bind to localhost only
  - "127.0.0.1:9090:9090"  # Dashboard only on localhost
```

Use reverse proxy (nginx/Caddy) for public access.

---

## Advanced Deployments

### Docker Swarm

```bash
# Initialize swarm
docker swarm init

# Deploy stack
docker stack deploy -c docker-compose.yml rustsocks

# Scale service
docker service scale rustsocks_rustsocks=3

# Check status
docker service ps rustsocks_rustsocks
```

### Kubernetes

Coming soon. See future sprints for Kubernetes Helm charts.

### Reverse Proxy (nginx)

Example nginx configuration:

```nginx
# Dashboard
server {
    listen 443 ssl;
    server_name rustsocks.company.com;

    ssl_certificate /etc/nginx/ssl/server.crt;
    ssl_certificate_key /etc/nginx/ssl/server.key;

    location / {
        proxy_pass http://localhost:9090;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }
}

# SOCKS5 proxy (TCP proxy)
stream {
    server {
        listen 1080;
        proxy_pass localhost:1080;
    }
}
```

---

## Maintenance

### Update to New Version

```bash
# Pull latest code
git pull origin master

# Rebuild image
docker-compose build --no-cache rustsocks

# Stop old container
docker-compose down

# Start new version
docker-compose up -d

# Verify
docker-compose logs -f rustsocks
curl http://localhost:9090/health
```

### Database Cleanup

Automatic cleanup is configured in rustsocks.toml:

```toml
[sessions]
retention_days = 90
cleanup_interval_hours = 24
```

Manual cleanup:

```bash
# Delete sessions older than 30 days
docker-compose exec rustsocks sqlite3 /data/sessions.db \
  "DELETE FROM sessions WHERE datetime(start_time) < datetime('now', '-30 days');"

# Vacuum to reclaim space
docker-compose exec rustsocks sqlite3 /data/sessions.db "VACUUM;"
```

### Log Rotation

Logs in `/var/log/rustsocks/` can grow large. Use logrotate:

```bash
# Create logrotate config
cat > /etc/logrotate.d/rustsocks <<EOF
/var/log/rustsocks/*.log {
    daily
    rotate 7
    compress
    delaycompress
    notifempty
    missingok
    copytruncate
}
EOF
```

---

## FAQ

**Q: How do I change the SOCKS port?**

A: Edit docker-compose.yml ports section or use environment variable:
```yaml
ports:
  - "2080:1080"  # Map host port 2080 to container 1080
```

**Q: Can I use this with multiple containers?**

A: Yes, but database must be on shared storage (NFS, EFS, etc.) or use separate databases per instance.

**Q: Is the dashboard secure?**

A: Dashboard has no built-in authentication. Deploy behind VPN, use reverse proxy with auth, or restrict to localhost.

**Q: How do I backup everything?**

A: Backup these directories:
```bash
docker cp rustsocks-proxy:/data ./backup/data
docker cp rustsocks-proxy:/etc/rustsocks ./backup/config
```

**Q: Can I disable the dashboard?**

A: Yes, set `dashboard_enabled = false` in rustsocks.toml.

**Q: What's the performance impact of session tracking?**

A: Very minimal (~1ms overhead). Database writes are batched for efficiency.

---

## Support

- **Documentation**: See main README.md and CLAUDE.md
- **Issues**: https://github.com/your-org/rustsocks/issues
- **Logs**: Use `docker-compose logs -f rustsocks` for troubleshooting

---

## License

MIT License. See LICENSE file for details.
