# RustSocks - High-Performance SOCKS5 Proxy Server

![Version](https://img.shields.io/badge/version-0.9.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.89%2B-orange.svg)
![License](https://img.shields.io/badge/license-MIT-green.svg)
![Status](https://img.shields.io/badge/status-Hardened%20Candidate-blue.svg)
![Coverage](https://img.shields.io/github/actions/workflow/status/xulek/RustSocks/coverage.yml?branch=master&label=coverage)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/xulek/RustSocks)

<div align="center">
  <img src="docs/assets/rustsocks.png" alt="RustSocks Logo" width="300">
</div>

A modern, high-performance SOCKS5 proxy server written in Rust, featuring advanced Access Control Lists (ACL), real-time session tracking, Prometheus metrics, and an intuitive web dashboard. Built for administrators who need fine-grained control, security, and comprehensive monitoring.

**Documentation:** https://xulek.github.io/RustSocks/

> **Security default:** public no-auth listeners are rejected unless explicitly opted in. The bundled Docker Compose publishes ports on loopback and requires secrets through environment variables.

---

## Key Features

- **🔐 Multi-Layer Authentication**
  - NoAuth, Username/Password (RFC 1929), GSS-API/Kerberos *(Unix, `gssapi` feature)*
  - PAM integration (IP-based & username/password authentication) *(Unix/SSSD only)*
  - **Active Directory / LDAP integration** (via SSSD/NSS on Unix systems)
  - Two-tier authentication (client-level + SOCKS-level)
  - Brute-force protection: failed logins are throttled per source address (username/password and PAM)
  - Cross-platform builds (core SOCKS server runs on Unix/Linux and Windows; advanced PAM/LDAP features require Unix)

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
  - **Dynamic access policies**: schedules, source networks, authentication method, validity windows, connection limits and daily/monthly transfer quotas, with a monitor mode and an Explain simulator ([details](#dynamic-access-policy-engine))
  - Resolved addresses are re-checked after DNS, so a hostname cannot bypass a CIDR block

- **📊 Comprehensive Session Management**
  - Real-time active session tracking
  - SQLite, MySQL and MariaDB persistence with automatic cleanup (requires `database` feature)
  - Traffic statistics (bytes sent/received, duration)
  - Batch writer for high-performance database operations
  - Policy limits and quotas can be shared between several instances through Redis (`redis` feature)

- **⚡ QoS & Rate Limiting**
  - Hierarchical Token Bucket (HTB) algorithm
  - Per-user bandwidth limits
  - Fair bandwidth sharing
  - Connection limits per user/destination

- **🚀 SOCKS5 and SOCKS4/4a**
  - CONNECT command (TCP connections), including SOCKS4/4a
  - BIND command (reverse connections, SOCKS5)
  - UDP ASSOCIATE command (UDP relay, SOCKS5; fragmentation is not supported)
  - IPv4, IPv6, and domain name resolution
  - Connection limits globally, per user and per client address

- **📈 Monitoring & Metrics**
  - Prometheus metrics export, including ACL/policy decisions, admission denials and authentication failures
  - Audit log of every state-changing API request
  - Real-time API endpoints
  - System resource monitoring (CPU, RAM)
  - Connection pool statistics
  - Performance insights

- **🎨 Modern Web Dashboard**
  - Login with roles (`viewer`, `operator`, `admin`) and optional Altcha CAPTCHA
  - Real-time session monitoring, session termination and CSV export
  - ACL rule and access policy management UI
  - User and group management
  - Statistics & analytics, telemetry and diagnostics
  - System resources overview
  - SMTP configuration with notification switches, cooldowns, and test emails
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
- Rust 1.89+ ([Install Rust](https://rustup.rs/))
- Node.js 20.19+ or 22.12+ (for dashboard only)
- Linux/Unix/Windows

**Prebuilt options:** tagged releases (`v*`) publish Linux and Windows archives with SHA-256 checksums on the [Releases](https://github.com/xulek/RustSocks/releases) page, and a container image on GitHub Container Registry. See [README-DOCKER.md](README-DOCKER.md) for Docker and Docker Compose.

**Build & Run:**

```bash
# Clone repository
git clone https://github.com/xulek/RustSocks.git
cd RustSocks

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

**Note**: Database-backed session storage (`sqlite`, `mariadb`, `mysql`) requires building with the `database` feature (or `--all-features`).

**The API and dashboard fail closed:** every `/api/*` request is rejected unless dashboard authentication or an API token (`sessions.api_token`) is configured. Enable the dashboard login with a session secret, at least one user and a role:

```toml
[sessions.dashboard_auth]
enabled = true
session_secret = "<long random string, e.g. openssl rand -base64 32>"
[[sessions.dashboard_auth.users]]
username = "admin"
password = "strong-secret"
[[sessions.dashboard_auth.roles]]
username = "admin"
role = "admin"                # viewer | operator | admin
```

See the [Dashboard Authentication guide](docs/guides/dashboard-authentication.md) for roles, CAPTCHA and cookie settings.

Once running, access:
- **Dashboard**: http://127.0.0.1:9090/
- **Swagger UI**: http://127.0.0.1:9090/swagger-ui/
- **API**: http://127.0.0.1:9090/api/

### Configuration

Create `config/rustsocks.toml`:

```toml
[server]
bind_address = "127.0.0.1"     # A public no-auth listener is refused unless server.allow_unsafe_public_proxy = true
bind_port = 1080
max_connections = 1000
max_connections_per_ip = 0     # 0 = unlimited; limits concurrent connections per client address

[auth]
socks_method = "none"  # Options: "none", "userpass", "pam.address", "pam.username", "gssapi"

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
api_token = "change-me"        # Token for /api/* (alternative to dashboard login)
smtp_encryption_key = "change-me-too"  # Encrypts SMTP passwords stored in the database

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
algorithm = "htb"

[qos.htb]
global_bandwidth_bytes_per_sec = 125000000
guaranteed_bandwidth_bytes_per_sec = 131072
max_bandwidth_bytes_per_sec = 12500000
burst_size_bytes = 1048576
refill_interval_ms = 50
fair_sharing_enabled = true
rebalance_interval_ms = 100
idle_timeout_secs = 5

[qos.connection_limits]
max_connections_per_user = 20
max_connections_global = 10000
```

**Note**: Database-backed session storage (`sqlite`, `mariadb`, `mysql`) requires building with the `database` feature (or `--all-features`). GSSAPI requires the `gssapi` feature and is supported on Unix systems. Every option is described in the [configuration reference](docs/config/rustsocks.md).

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
- Node.js 20.19+ or 22.12+ (for building the dashboard only; not required at runtime)
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
- User, group, ACL rule and access policy management
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
# 2. Build dashboard (only if not built yet)
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

Performance impact depends on destination reuse and latency. Measure with your workload and monitor `/api/pool/stats`.

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
algorithm = "htb"

[qos.htb]
global_bandwidth_bytes_per_sec = 125000000
guaranteed_bandwidth_bytes_per_sec = 131072
max_bandwidth_bytes_per_sec = 12500000
burst_size_bytes = 1048576
refill_interval_ms = 50
fair_sharing_enabled = true
rebalance_interval_ms = 100
idle_timeout_secs = 5

[qos.connection_limits]
max_connections_per_user = 20
max_connections_global = 10000
```

**Configuration Options:**

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | false | Enable/disable QoS |
| `algorithm` | htb | QoS algorithm |
| `qos.htb.*` | see config | HTB bandwidth and fairness tuning |
| `qos.connection_limits.*` | see config | Per-user and global connection limits |

**How It Works:**

1. **Token Bucket Algorithm**: Each user has a "bucket" of bandwidth tokens
2. **Rate Limiting**: Users can only send/receive data at configured Mbps
3. **Connection Limits**: Rejects new connections if user exceeds limit
4. **Fair Sharing**: HTB algorithm ensures no user starves others
5. **Connection Limits**: Enforces per-user and global caps

**Monitoring QoS:**

QoS metrics are exported via Prometheus when metrics are enabled (see `/metrics`).

---

## Dynamic Access Policy Engine

Policies (`[[policies]]`) extend the legacy ACL with context a destination rule cannot express. They share one priority order with the legacy rules, so existing configurations keep working unchanged.

```toml
[[policies]]
id = "developers-github"
groups = ["developers"]
action = "allow"
destinations = ["github.com", "*.github.com"]
ports = ["443"]
protocols = ["tcp"]
priority = 1500
enforce_conditions = true      # failing a condition blocks instead of falling through

[policies.conditions]
source_ips = ["10.0.0.0/8"]
auth_methods = ["userpass", "gssapi"]
max_active_connections = 20
daily_transfer_limit_bytes = 10737418240

[policies.conditions.schedule]
days = ["mon", "tue", "wed", "thu", "fri"]
start = "07:00"
end = "20:00"
utc_offset_minutes = 120
```

- `mode = "monitor"` records what a policy would do (`rustsocks_policy_monitor_matches_total`) without affecting traffic, so a rule can be observed before it is enforced.
- Manage policies in the dashboard (**Access Policies**) or through `/api/acl/policies`; `POST /api/acl/test` explains a decision step by step.
- To enforce limits across several instances behind a load balancer, set `[policy_state] backend = "redis"` (build with the `redis` feature).

See the [policy engine guide](docs/config/policy-engine.md) and `config/examples/acl-policies.toml`.

---

## How It Works (Architecture)

RustSocks implements a layered architecture combining security, performance, and observability:

### Request Flow

1. **TCP Accept** - Listener accepts the connection (global and per-address limits apply)
2. **Handshake** - SOCKS5 (or SOCKS4/4a) negotiation
3. **Authentication** - Validate user (if configured), with failure throttling
4. **ACL and Policy Evaluation** - Check legacy rules and dynamic policies in one priority order (if enabled)
5. **Connection Establishment** - Resolve the destination, re-check the resolved address, then connect
6. **Data Proxying** - Bidirectional async copy with metrics
7. **Session Lifecycle** - Track, persist, and cleanup

### Key Components

- **Protocol Module** (`src/protocol/`) - SOCKS4/5 parsing and serialization
- **ACL Engine** (`src/acl/`) - Rule and policy evaluation with hot-reload, usage tracking and decision metrics
- **Session Manager** (`src/session/`) - Active tracking + optional database persistence
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
- **Sessions** - Live session monitoring with filtering, sorting, history, termination and CSV export
- **ACL Rules** - Browse and manage access control rules
- **Access Policies** - Manage dynamic policies and try requests in the Explain simulator
- **User Management** - Manage users, groups and memberships
- **Statistics / Telemetry** - Detailed analytics, bandwidth metrics and error trends
- **Diagnostics** - Connectivity checks (administrators only)
- **SMTP** - Notification settings and test emails
- **System Resources** - CPU, RAM usage (system-wide and process-specific)

---

## Supported Authentication Methods

| Method | Description | Security |
|--------|-------------|----------|
| **None** | No authentication required | Low - use in trusted networks |
| **Username/Password** | SOCKS5 RFC 1929 | Medium - credentials in plaintext (use TLS) |
| **PAM Address** | IP-based authentication | Medium - IP spoofing possible |
| **PAM Username** | System PAM module | High - leverages system auth |
| **GSS-API** | Kerberos (`gssapi` feature, Unix) | High - no password on the wire |

**Recommended:** Combine TLS + PAM for maximum security.

---

## Active Directory Integration

RustSocks provides **native Active Directory integration** for enterprise environments, enabling authentication and group-based access control using your existing Windows AD infrastructure.

### Features

- ✅ **Seamless AD Authentication** - Users authenticate with their AD credentials
- ✅ **Automatic Group Resolution** - AD security groups retrieved automatically via SSSD
- ✅ **Group-Based ACL Rules** - Control access using AD groups (e.g., Developers, Temps, Admins)
- ✅ **Kerberos Support** - Secure, encrypted authentication
- ✅ **Works with Azure AD DS** - Compatible with Azure Active Directory Domain Services
- ✅ **No Code Changes Required** - Uses standard Unix authentication stack (PAM/SSSD/NSS)

### Quick Setup

1. **Join Linux server to AD domain:**
   ```bash
   sudo realm join --user=administrator ad.company.com
   ```

2. **Configure PAM service** (`/etc/pam.d/rustsocks`):
   ```
   auth required pam_sss.so
   account required pam_sss.so
   ```

3. **Configure RustSocks** (`config/rustsocks.toml`):
   ```toml
   [auth]
   socks_method = "pam.username"

   [acl]
   enabled = true
   config_file = "config/acl.toml"
   ```

4. **Define ACL rules with AD groups** (`config/acl.toml`):
   ```toml
   [[groups]]
   name = "Developers@ad.company.com"
     [[groups.rules]]
     action = "allow"
     destinations = ["*.dev.company.com"]
     ports = ["*"]
     priority = 100

   [[groups]]
   name = "Temps@ad.company.com"
     [[groups.rules]]
     action = "allow"
     destinations = ["*.company.com"]  # Only company sites
     ports = ["80", "443"]
     priority = 50
   ```

5. **Test connection:**
   ```bash
   # Authenticate with AD credentials
   curl -x socks5://alice:PASSWORD@localhost:1080 http://example.com
   ```

### Use Cases

**Scenario:** Temporary employees need access to work sites only, while full employees have broader access.

**Solution:** Create AD groups ("Temps", "Employees") and define ACL rules:
- **Temps group:** Allow `*.company.com` and essential services only
- **Employees group:** Allow all destinations except social media
- **Admins group:** Unrestricted access

### Documentation

📖 **Complete Guide:** [Active Directory Integration Guide](docs/guides/active-directory.md)

The guide covers:
- Step-by-step AD domain join instructions
- SSSD and Kerberos configuration
- ACL rules with AD groups examples
- Troubleshooting and best practices
- Multi-domain and Azure AD DS support

### Example Configuration Files

All example configurations are available in `config/examples/`:
- `sssd-ad.conf` - SSSD configuration for AD
- `krb5-ad.conf` - Kerberos configuration
- `acl-ad-example.toml` - ACL rules with AD groups
- `rustsocks-ad.toml` - Complete RustSocks config for AD

---

## REST API Examples

`/api/*` requires a dashboard session or the API token (`sessions.api_token`); `/health` is public.

```bash
TOKEN="change-me"   # sessions.api_token

# Active sessions
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9090/api/sessions/active

# Session statistics (past 24h)
curl -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:9090/api/sessions/stats?window_hours=24"

# Health check
curl http://127.0.0.1:9090/health

# Prometheus metrics
curl http://127.0.0.1:9090/metrics

# System resources
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9090/api/system/resources

# Connection pool stats
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9090/api/pool/stats
```

Full API documentation: **http://127.0.0.1:9090/swagger-ui/**

---

## Performance & Testing

### Benchmarks

Benchmark results vary by hardware and configuration. For current numbers, run the Criterion benchmarks in `benches/` (for example `cargo bench --bench policy_evaluation`) and the end-to-end load tests in `loadtests/` (also available as a manual GitHub Actions workflow). See the [Testing Guide](docs/guides/testing.md).

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

**Test Coverage:** See the Testing Guide for current counts and instructions. ✅

---

## Development

### Requirements

- Rust 1.89+
- Node.js 20.19+ or 22.12+ (dashboard)
- libpam0g-dev and libkrb5-dev (Linux, for PAM and GSS-API when building with `--all-features`)
- SQLite/MySQL/MariaDB (for persistence; requires the `database` feature)

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

# Everything CI runs (fmt, check, clippy, tests in three feature sets)
./scripts/ci-local.sh        # Windows: .\scripts\ci-local.ps1

# Dependency advisories, licences and sources
cargo deny --all-features check
```

Tests for the Redis backend run when `REDIS_URL` is set (for example `REDIS_URL=redis://127.0.0.1:6379/0 cargo test --all-features`) and are skipped otherwise.

### Project Structure

```
rustsocks/
├── src/
│   ├── protocol/          # SOCKS5 protocol implementation (types, parsing)
│   ├── auth/              # Authentication backends (PAM, GSS-API, username/password, throttling)
│   ├── acl/               # ACL and policy engine (rules, policies, matching, usage state, hot-reload)
│   ├── session/           # Session tracking & persistence (manager, store, batch writer)
│   ├── server/            # Server logic & connection pool (listener, handler, proxy, pool)
│   ├── api/               # REST API handlers (endpoints, types, middleware)
│   ├── config/            # Configuration management (parsing, validation)
│   ├── metrics/           # Prometheus metrics collection
│   ├── qos/               # QoS & rate limiting (Token Bucket algorithm)
│   ├── smtp/              # SMTP notifications (encrypted settings, alerts)
│   ├── telemetry.rs       # Error and latency history for the dashboard
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
├── docker-compose.yml     # Hardened single-container deployment
├── deny.toml              # cargo-deny policy (advisories, licences, sources)
├── .github/workflows/     # CI, coverage, supply-chain checks, release, docs, load test
├── .dockerignore          # Docker build exclusions
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
metrics = ["prometheus", "lazy_static"]    # Prometheus metrics export (default)
fast-allocator = ["mimalloc"]              # mimalloc allocator (default)
database = ["database-sqlite", "database-mysql"]  # SQLite + MySQL/MariaDB session storage
database-sqlite = ["sqlx-core", "sqlx-sqlite"]    # SQLite only
database-mysql = ["mysql_async"]           # MySQL/MariaDB only
gssapi = ["libgssapi"]                     # GSS-API / Kerberos authentication (Unix)
redis = ["dep:redis"]                      # Share policy usage state between instances
```

**Build with all features:**

```bash
cargo build --release --all-features
```

---

## Documentation

- **[Configuration Reference](docs/config/rustsocks.md)** - Every `rustsocks.toml` option
  - [ACL configuration](docs/config/acl.md)
  - [Dynamic Policy Engine](docs/config/policy-engine.md), including running several instances

- **[User Guides](docs/guides/)** - Setup & deployment
  - [Dashboard Authentication](docs/guides/dashboard-authentication.md)
  - [Metrics & Audit Log](docs/guides/metrics-and-audit.md)
  - [SMTP Configuration](docs/guides/smtp-configuration.md)
  - [LDAP Groups Integration](docs/guides/ldap-groups.md)
  - [Active Directory](docs/guides/active-directory.md)
  - [Building with Base Path](docs/guides/building-with-base-path.md)
  - [Testing](docs/guides/testing.md)
  - [Docker](README-DOCKER.md)

- **[Technical Documentation](docs/technical/)** - Implementation details
  - [ACL Engine](docs/technical/acl-engine.md)
  - [PAM Authentication](docs/technical/pam-authentication.md)

- **[CLAUDE.md](CLAUDE.md)** - Complete developer guide

---


## Reporting Issues

Found a bug? Please report it in the [**Issues**](https://github.com/xulek/rustsocks/issues) section:

1. Include RustSocks version (`./target/release/rustsocks --version`)
2. Provide relevant config (with sensitive data redacted)
3. Attach server logs (run with `--log-level debug`, or set `[logging] level = "debug"`)
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
