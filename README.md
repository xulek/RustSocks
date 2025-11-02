# RustSocks - High-Performance SOCKS5 Proxy Server

![Version](https://img.shields.io/badge/version-0.9.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)
![License](https://img.shields.io/badge/license-MIT-green.svg)
![Status](https://img.shields.io/badge/status-Production%20Ready-brightgreen.svg)

<div align="center">
  <img src="docs/assets/rustsocks.png" alt="RustSocks Logo" width="300">
</div>

A modern, high-performance SOCKS5 proxy server written in Rust, featuring advanced Access Control Lists (ACL), real-time session tracking, Prometheus metrics, and an intuitive web dashboard. Built for administrators who need fine-grained control, security, and comprehensive monitoring.

---

## Key Features

- **🔐 Multi-Layer Authentication**
  - NoAuth, Username/Password (RFC 1929)
  - PAM integration (IP-based & username/password authentication)
  - Two-tier authentication (client-level + SOCKS-level)
  - Cross-platform support (Unix/Linux + Windows)

- **🔒 Transport Security (SOCKS over TLS)**
  - Full TLS 1.2 & TLS 1.3 support
  - Mutual TLS (mTLS) with client certificate validation
  - Configurable minimum protocol versions
  - Self-signed certificate support

- **🛡️ Advanced Access Control**
  - Per-user and per-group rules
  - CIDR ranges, wildcard domains, custom port ranges
  - LDAP groups integration
  - Hot-reload without downtime
  - Priority-based rule evaluation

- **📊 Comprehensive Session Management**
  - Real-time active session tracking
  - SQLite persistence with automatic cleanup
  - Traffic statistics (bytes sent/received, duration)
  - Batch writer for high-performance database operations

- **⚡ QoS & Rate Limiting**
  - Hierarchical Token Bucket (HTB) algorithm
  - Per-user bandwidth limits
  - Fair bandwidth sharing
  - Connection limits per user/destination

- **🚀 Complete SOCKS5 Support**
  - CONNECT command (TCP connections)
  - BIND command (reverse connections)
  - UDP ASSOCIATE command (UDP relay)
  - IPv4, IPv6, and domain name resolution

- **📈 Monitoring & Metrics**
  - Prometheus metrics export
  - Real-time API endpoints
  - System resource monitoring (CPU, RAM)
  - Connection pool statistics
  - Performance insights

- **🎨 Modern Web Dashboard**
  - Real-time session monitoring
  - ACL rule management UI
  - User management
  - Statistics & analytics
  - System resources overview
  - Built with React + Vite

- **🔌 REST API & Swagger**
  - Full REST API with JSON
  - Swagger UI documentation
  - Session history queries
  - Statistics aggregation
  - Connectivity diagnostics

---

## Installation

### Quick Start (Build from Source)

**Requirements:**
- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Node.js 18+ (for dashboard only)
- Linux/Unix/Windows

**Build & Run:**

```bash
# Clone repository
git clone https://github.com/yourusername/rustsocks.git
cd rustsocks

# Build release version
cargo build --release

# Generate example config
./target/release/rustsocks --generate-config config/rustsocks.toml

# Run server
./target/release/rustsocks --config config/rustsocks.toml
```

**Dashboard Setup (Optional):**

To build and enable the web dashboard, you'll need Node.js 18+:

```bash
# Navigate to dashboard directory
cd dashboard

# Install dependencies
npm install

# Build for production
npm run build

# This creates optimized static files in dashboard/dist/
# which are served automatically by the backend
```

Then enable the dashboard in `config/rustsocks.toml`:

```toml
[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite://sessions.db"
stats_api_enabled = true       # Enable REST API server
dashboard_enabled = true       # Enable web dashboard
swagger_enabled = true         # Enable Swagger UI documentation
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
```

Once running, access:
- **Dashboard**: http://127.0.0.1:9090/
- **Swagger UI**: http://127.0.0.1:9090/swagger-ui/
- **API**: http://127.0.0.1:9090/api/

### Configuration

Create `config/rustsocks.toml`:

```toml
[server]
bind_address = "0.0.0.0"
bind_port = 1080
max_connections = 1000

[auth]
socks_method = "none"  # Options: "none", "userpass", "pam.address", "pam.username"

[acl]
enabled = true
config_file = "config/acl.toml"
watch = true  # Hot reload

[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite://sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24
traffic_update_packet_interval = 10
stats_window_hours = 24

# REST API & Dashboard
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
base_path = "/"                # Change to "/rustsocks" for subdirectory deployment

# Connection Pooling (optional, disabled by default)
[server.pool]
enabled = true                 # Enable connection pooling for upstream connections
max_idle_per_dest = 4          # Max idle connections per destination
max_total_idle = 100           # Max total idle connections across all destinations
idle_timeout_secs = 90         # How long to keep idle connections alive
connect_timeout_ms = 5000      # Timeout for establishing new connections

# QoS & Rate Limiting (optional, disabled by default)
[qos]
enabled = true                 # Enable QoS and bandwidth limiting
default_rate_limit_mbps = 100  # Default bandwidth limit per user (Mbps)
default_conn_limit = 10        # Default simultaneous connections per user
enable_traffic_shaping = true  # Enable HTB algorithm for fair bandwidth sharing
```

### Testing Connection

```bash
# Test with curl
curl -x socks5://127.0.0.1:1080 http://example.com

# Test with authentication
curl -x socks5://user:password@127.0.0.1:1080 http://example.com
```

---

## Advanced Features Configuration

### Web Dashboard & Administration

The RustSocks web dashboard provides real-time monitoring and management of your SOCKS5 proxy.

**Prerequisites:**
- Node.js 18+ (for building dashboard only; not required at runtime)
- Already built dashboard files (`dashboard/dist/`)

**Building the Dashboard:**

```bash
# Install Node.js dependencies
cd dashboard
npm install

# Build optimized production bundle
npm run build

# Verify dashboard/dist/ directory was created
ls -la dashboard/dist/
```

The build process creates a `dist/` directory with static files served by the backend.

**Enabling the Dashboard:**

Update `config/rustsocks.toml`:

```toml
[sessions]
stats_api_enabled = true       # Must be enabled for dashboard to work
dashboard_enabled = true       # Enable dashboard
swagger_enabled = true         # Enable API documentation
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090          # API and dashboard port
```

**Accessing the Dashboard:**

Once the server is running:
- **Admin Dashboard**: http://127.0.0.1:9090/
- **Swagger API Docs**: http://127.0.0.1:9090/swagger-ui/
- **REST API**: http://127.0.0.1:9090/api/

The dashboard includes:
- Real-time session monitoring
- User and ACL rule management
- System resource usage (CPU, RAM)
- Bandwidth statistics and analytics
- Connection pool statistics

**Deployment with Custom Base URL:**

If deploying behind a reverse proxy or at a subdirectory URL:

```bash
# 1. Set base_path in config
[sessions]
base_path = "/rustsocks"  # URLs will be /rustsocks, /rustsocks/api/, etc.
```

```bash
# 2. Rebuild dashboard (it auto-detects base_path)
cd dashboard
npm run build
```

```bash
# 3. Rebuild and run server with config
cargo build --release
./target/release/rustsocks --config config/rustsocks.toml
```

Now access dashboard at: http://127.0.0.1:9090/rustsocks

For nginx reverse proxy setup, see [Building with Base Path Guide](docs/guides/building-with-base-path.md).

### Connection Pooling

Connection pooling reuses upstream TCP connections, dramatically improving performance for frequent destinations.

**Why Use Connection Pooling?**
- Reduces latency (no repeated TCP handshakes)
- Decreases CPU usage
- Improves throughput for repeated connections
- Lowers network overhead

**Enabling Connection Pooling:**

Update `config/rustsocks.toml`:

```toml
[server.pool]
enabled = true                 # Enable connection pooling
max_idle_per_dest = 4          # Keep up to 4 idle connections per destination
max_total_idle = 100           # Max 100 idle connections total
idle_timeout_secs = 90         # Close idle connections after 90 seconds
connect_timeout_ms = 5000      # 5 second timeout for new connections
```

**Configuration Options:**

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | false | Enable/disable connection pooling |
| `max_idle_per_dest` | 4 | Maximum idle connections per destination |
| `max_total_idle` | 100 | Maximum total idle connections across all destinations |
| `idle_timeout_secs` | 90 | How long to keep idle connections alive |
| `connect_timeout_ms` | 5000 | Timeout for establishing new connections (ms) |

**How It Works:**

1. After completing a SOCKS5 connection, the upstream TCP connection is returned to the pool
2. Next connection to the same destination reuses a pooled connection
3. Expired or excess connections are closed automatically
4. Pool statistics available via API: `GET /api/pool/stats`

**Performance Impact:**

- **With pooling disabled**: 3,000 ops/sec
- **With pooling enabled**: 7,000 ops/sec (2.3x improvement)
- **Memory overhead**: ~50KB per pooled connection

### QoS & Rate Limiting

QoS (Quality of Service) limits bandwidth and connections per user to prevent resource exhaustion.

**Why Use QoS?**
- Prevent single user from consuming all bandwidth
- Fair bandwidth distribution among users
- Connection limits per user
- Protect server from abuse

**Enabling QoS:**

Update `config/rustsocks.toml`:

```toml
[qos]
enabled = true                         # Enable QoS and rate limiting
default_rate_limit_mbps = 100          # Default 100 Mbps per user
default_conn_limit = 10                # Default 10 simultaneous connections per user
enable_traffic_shaping = true          # Use HTB algorithm for fair sharing
```

**Per-User Configuration in `config/acl.toml`:**

```toml
[[users]]
username = "alice"
rate_limit_mbps = 50                   # Override: 50 Mbps for alice
connection_limit = 5                   # Override: 5 simultaneous connections

[[users]]
username = "bob"
rate_limit_mbps = 200                  # Override: 200 Mbps for bob
connection_limit = 20                  # Override: 20 simultaneous connections

[[groups]]
name = "developers"
rate_limit_mbps = 150
connection_limit = 15
```

**Configuration Options:**

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | false | Enable/disable QoS |
| `default_rate_limit_mbps` | 100 | Bandwidth limit (Mbps) |
| `default_conn_limit` | 10 | Maximum simultaneous connections |
| `enable_traffic_shaping` | true | Use HTB for fair bandwidth distribution |

**How It Works:**

1. **Token Bucket Algorithm**: Each user has a "bucket" of bandwidth tokens
2. **Rate Limiting**: Users can only send/receive data at configured Mbps
3. **Connection Limits**: Rejects new connections if user exceeds limit
4. **Fair Sharing**: HTB algorithm ensures no user starves others
5. **Per-Group Limits**: Groups inherit limits applied to all members

**Monitoring QoS:**

View current QoS status via API:
```bash
curl http://127.0.0.1:9090/api/qos/status
```

QoS metrics in dashboard under "Statistics" tab.

---

## How It Works (Architecture)

RustSocks implements a layered architecture combining security, performance, and observability:

### Request Flow

1. **TCP Accept** - Listener accepts incoming connection
2. **SOCKS5 Handshake** - Negotiate authentication method
3. **Authentication** - Validate user (if configured)
4. **ACL Evaluation** - Check access rules (if enabled)
5. **Connection Establishment** - Resolve and connect to destination
6. **Data Proxying** - Bidirectional async copy with metrics
7. **Session Lifecycle** - Track, persist, and cleanup

### Key Components

- **Protocol Module** (`src/protocol/`) - SOCKS5 parsing and serialization
- **ACL Engine** (`src/acl/`) - Rule evaluation with hot-reload
- **Session Manager** (`src/session/`) - Active tracking + SQLite persistence
- **Connection Pool** (`src/server/pool.rs`) - Upstream connection reuse
- **REST API** (`src/api/`) - Management endpoints and metrics
- **QoS** (`src/qos/`) - Rate limiting and bandwidth management

### Data Flow

```
Client → TLS/TCP → Auth → ACL → Destination
                 ↓
            Session Manager ↔ SQLite
                 ↓
            Metrics (Prometheus)
                 ↓
            Dashboard/API
```

---

## Dashboard Features

Access the admin dashboard at **http://127.0.0.1:9090** (when enabled):

- **Dashboard** - Real-time overview with active sessions, top users, top destinations
- **Sessions** - Live session monitoring with filtering, sorting, and history
- **ACL Rules** - Browse and manage access control rules
- **User Management** - View users and group memberships
- **Statistics** - Detailed analytics and bandwidth metrics
- **System Resources** - CPU, RAM usage (system-wide and process-specific)

---

## Supported Authentication Methods

| Method | Description | Security |
|--------|-------------|----------|
| **None** | No authentication required | Low - use in trusted networks |
| **Username/Password** | SOCKS5 RFC 1929 | Medium - credentials in plaintext (use TLS) |
| **PAM Address** | IP-based authentication | Medium - IP spoofing possible |
| **PAM Username** | System PAM module | High - leverages system auth |

**Recommended:** Combine TLS + PAM for maximum security.

---

## REST API Examples

```bash
# Active sessions
curl http://127.0.0.1:9090/api/sessions/active

# Session statistics (past 24h)
curl http://127.0.0.1:9090/api/sessions/stats?window_hours=24

# Health check
curl http://127.0.0.1:9090/health

# Prometheus metrics
curl http://127.0.0.1:9090/metrics

# System resources
curl http://127.0.0.1:9090/api/system/resources

# Connection pool stats
curl http://127.0.0.1:9090/api/pool/stats
```

Full API documentation: **http://127.0.0.1:9090/swagger-ui/**

---

## Performance & Testing

### Benchmarks (v0.9.0)

- **Throughput:** 7,000+ ops/sec (concurrent connections)
- **Latency:** <5ms average (p99 <50ms)
- **Memory:** 231 MB @ 200k+ connections
- **CPU:** Scales linearly with concurrency
- **ACL Evaluation:** 1.92ms average
- **Database Writes:** 12,279 sessions/second

### Running Tests

```bash
# All tests with all features
cargo test --all-features

# Specific module
cargo test --lib acl

# Integration tests
cargo test --test '*'

# With output
cargo test -- --nocapture

# Release mode (performance tests)
cargo test --release -- --ignored --nocapture
```

**Test Coverage:** 273 passing tests, 14 ignored (performance) ✅

---

## Development

### Requirements

- Rust 1.70+
- Node.js 18+ (dashboard)
- libpam0g-dev (Linux, for PAM)
- SQLite (for persistence)

### Build Commands

```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Check without building
cargo check --all-features

# Code quality
cargo clippy --all-features -- -D warnings
cargo fmt --check

# Security audit
cargo audit
```

### Project Structure

```
rustsocks/
├── src/
│   ├── protocol/          # SOCKS5 protocol implementation (types, parsing)
│   ├── auth/              # Authentication backends (PAM, username/password)
│   ├── acl/               # Access Control List engine (rules, matching, hot-reload)
│   ├── session/           # Session tracking & persistence (manager, store, batch writer)
│   ├── server/            # Server logic & connection pool (listener, handler, proxy, pool)
│   ├── api/               # REST API handlers (endpoints, types, middleware)
│   ├── config/            # Configuration management (parsing, validation)
│   ├── metrics/           # Prometheus metrics collection
│   ├── qos/               # QoS & rate limiting (Token Bucket algorithm)
│   ├── utils/             # Utility functions (error handling, helpers)
│   ├── lib.rs             # Library exports
│   └── main.rs            # Server entry point & CLI handling
├── dashboard/             # React + Vite web dashboard
│   ├── src/
│   │   ├── components/    # React components (modals, drawers, cards)
│   │   ├── pages/         # Dashboard pages (Dashboard, Sessions, ACL, Users, Stats)
│   │   ├── lib/           # Utility functions (API calls, helpers)
│   │   ├── tests/         # Component tests
│   │   ├── index.css      # Global styling
│   │   └── main.jsx       # App entry point
│   ├── public/            # Static assets (favicon, images)
│   ├── dist/              # Built dashboard (generated)
│   └── package.json       # Node.js dependencies
├── tests/                 # Integration tests (ACL, Pool, E2E, UDP, BIND, TLS)
├── migrations/            # SQLite migrations (schema, indexes)
├── config/                # Example configuration files
│   └── pam.d/             # PAM service configurations
├── docs/                  # Documentation & guides
│   ├── assets/            # Logo & images
│   ├── guides/            # User guides (LDAP, base path setup)
│   ├── technical/         # Technical docs (ACL engine, PAM, architecture)
│   └── examples/          # Configuration examples
├── docker/                # Docker configuration
│   ├── entrypoint.sh      # Container startup script
│   └── configs/           # Docker-specific configs
├── examples/              # Example binaries (echo server, load test)
├── loadtests/             # Performance testing (k6, scripts, results)
├── scripts/               # Build & utility scripts
├── Cargo.toml             # Rust project manifest
├── Cargo.lock             # Dependency lock file
├── Dockerfile             # Multi-stage Docker build
├── .dockerignore           # Docker build exclusions
├── CLAUDE.md              # Developer guide for Claude Code
├── README.md              # Project documentation
└── LICENSE                # MIT License
```

---

## Feature Flags

Control compilation with Cargo features:

```toml
default = ["metrics", "fast-allocator"]

# Optional features
metrics = ["prometheus"]          # Prometheus metrics export
database = ["sqlx"]               # SQLite persistence
fast-allocator = ["mimalloc"]     # Faster memory allocator
```

**Build with all features:**

```bash
cargo build --release --all-features
```

---

## Documentation

- **[User Guides](docs/guides/)** - Setup & deployment
  - [LDAP Groups Integration](docs/guides/ldap-groups.md)
  - [Building with Base Path](docs/guides/building-with-base-path.md)

- **[Technical Documentation](docs/technical/)** - Implementation details
  - [ACL Engine](docs/technical/acl-engine.md)
  - [PAM Authentication](docs/technical/pam-authentication.md)

- **[CLAUDE.md](CLAUDE.md)** - Complete developer guide

---


## Reporting Issues

Found a bug? Please report it in the [**Issues**](https://github.com/xulek/rustsocks/issues) section:

1. Include RustSocks version (`./target/release/rustsocks --version`)
2. Provide relevant config (with sensitive data redacted)
3. Attach server logs (enable `log_level = "debug"`)
4. Steps to reproduce the issue

---

## Support & Contribution

**Questions?** Check the documentation or open a discussion.

**Want to contribute?** We welcome:
- Bug reports and fixes
- Feature requests
- Documentation improvements
- Performance optimizations
- Test coverage expansion

---

## License

MIT License - see [LICENSE](LICENSE) file for details.

---

## Acknowledgments

- Built with [Tokio](https://tokio.rs/) async runtime
- Powered by Rust 🦀

---

**Status:** 🟢 Production Ready
**Version:** 0.9.0
**Tests:** 273/287 (14 ignored - performance) passing ✅
**Code Quality:** Zero clippy warnings ✅
**Performance:** All targets exceeded ✅
**Last Updated:** 2025-11-02
