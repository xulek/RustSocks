# RustSocks - High-Performance SOCKS5 Proxy Server

🚀 Nowoczesny, wydajny serwer SOCKS5 napisany w Rust z zaawansowanym ACL, session tracking i web dashboard.

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## ✨ Kluczowe Funkcje

- **🔐 Autentykacja**
  - No-Auth, Username/Password
  - PAM integration (IP-based & username/password)
  - Two-tier authentication (client + SOCKS levels)

- **🛡️ Access Control Lists (ACL)**
  - Per-user i per-group rules
  - CIDR ranges, wildcard domains, port ranges
  - LDAP groups integration
  - Hot-reload bez downtime
  - Priority-based evaluation

- **📊 Session Management**
  - Real-time session tracking
  - SQLite persistence
  - Traffic statistics
  - Prometheus metrics export

- **🚀 Full SOCKS5 Support**
  - CONNECT, BIND, UDP ASSOCIATE
  - IPv4, IPv6, domain names
  - Async I/O (Tokio)

- **⚡ QoS & Rate Limiting**
  - Hierarchical Token Bucket (HTB)
  - Per-user bandwidth limits
  - Fair sharing algorithm
  - Connection limits

- **🎨 Web Dashboard**
  - Real-time session monitoring
  - ACL rule management
  - Statistics & analytics
  - Modern React UI

- **📡 REST API**
  - Session management
  - Statistics endpoint
  - Health checks
  - Swagger UI documentation

## 🚀 Quick Start

### Instalacja

```bash
# Clone repository
git clone https://github.com/yourusername/rustsocks.git
cd rustsocks

# Build release version
cargo build --release

# Generate example config
./target/release/rustsocks --generate-config config/rustsocks.toml
```

### Podstawowe uruchomienie

```bash
# Start with defaults (127.0.0.1:1080, no-auth)
./target/release/rustsocks

# Start with config file
./target/release/rustsocks --config config/rustsocks.toml

# Override bind address/port
./target/release/rustsocks --bind 0.0.0.0 --port 1080
```

### Test z curl

```bash
curl -x socks5://127.0.0.1:1080 http://example.com
```

## 📚 Dokumentacja

- **[User Guides](docs/guides/)** - Przewodniki użytkownika
  - [LDAP Groups Guide](docs/guides/ldap-groups.md)

- **[Technical Documentation](docs/technical/)** - Szczegóły implementacji
  - [ACL Engine](docs/technical/acl-engine.md)
  - [PAM Authentication](docs/technical/pam-authentication.md)

- **[Examples](docs/examples/)** - Przykładowe konfiguracje
  - `rustsocks.example.toml` - Pełna konfiguracja serwera
  - `acl.example.toml` - Reguły ACL

- **[CLAUDE.md](CLAUDE.md)** - Kompletny przewodnik dla developerów

## 🎨 Web Dashboard

Dashboard administracyjny z real-time monitoring:

```toml
[sessions]
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_port = 9090
```

### Dostęp

- **Dashboard**: http://127.0.0.1:9090/
- **Swagger UI**: http://127.0.0.1:9090/swagger-ui/
- **API**: http://127.0.0.1:9090/api/*

### Development

```bash
cd dashboard
npm install
npm run dev  # Development server on :3000
npm run build  # Production build
```

**Funkcje dashboardu:**
- 📊 Real-time session monitoring
- 🛡️ ACL rules browser
- 👥 User management
- 📈 Statistics & analytics
- ⚙️ Configuration view

## ⚙️ Konfiguracja

### Minimalna konfiguracja

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080

[auth]
socks_method = "none"  # Options: "none", "userpass", "pam.address", "pam.username"

[logging]
level = "info"
format = "pretty"
```

### Przykład z ACL

```toml
[acl]
enabled = true
config_file = "config/acl.toml"
watch = true  # Hot reload

# config/acl.toml
[global]
default_policy = "block"

[[users]]
username = "alice"
groups = ["developers"]

  [[users.rules]]
  action = "allow"
  description = "Allow HTTPS"
  destinations = ["*.example.com"]
  ports = ["443"]
  protocols = ["tcp"]
  priority = 100
```

### Session tracking z SQLite

```toml
[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite://sessions.db"
stats_api_enabled = true
```

## 🔌 REST API

```bash
# Active sessions
curl http://127.0.0.1:9090/api/sessions/active

# Session statistics
curl http://127.0.0.1:9090/api/sessions/stats

# Health check
curl http://127.0.0.1:9090/health

# Prometheus metrics
curl http://127.0.0.1:9090/metrics
```

Pełna dokumentacja API: http://127.0.0.1:9090/swagger-ui/

## 🧪 Testing

```bash
# All tests
cargo test --all-features

# Specific module
cargo test --lib acl

# Integration tests
cargo test --test '*'

# With output
cargo test -- --nocapture
```

**Status testów:** ✅ 76/76 passed

## 📁 Struktura Projektu

```
rustsocks/
├── src/                    # Source code
│   ├── protocol/          # SOCKS5 protocol
│   ├── auth/              # Authentication
│   ├── acl/               # Access Control Lists
│   ├── session/           # Session management
│   ├── server/            # Server logic
│   ├── qos/               # QoS & rate limiting
│   └── api/               # REST API
├── dashboard/             # Web UI (React + Vite)
├── docs/                  # Documentation
│   ├── guides/           # User guides
│   ├── technical/        # Technical docs
│   └── examples/         # Config examples
├── config/                # Active configuration
├── examples/              # Rust code examples
├── tests/                 # Integration tests
├── migrations/            # SQLx migrations
└── Cargo.toml            # Dependencies
```

## 📊 Metryki Prometheus

```
rustsocks_active_sessions               # Current active sessions
rustsocks_sessions_total                # Total accepted sessions
rustsocks_sessions_rejected_total       # Rejected by ACL
rustsocks_session_duration_seconds      # Session duration histogram
rustsocks_bytes_sent_total              # Total bytes sent
rustsocks_bytes_received_total          # Total bytes received
rustsocks_user_sessions_total{user}     # Per-user sessions
rustsocks_qos_active_users              # QoS active users
rustsocks_qos_bandwidth_allocated_*     # QoS bandwidth
```

## 🛠️ Development

### Wymagania

- Rust 1.70+
- Node.js 18+ (dla dashboard)
- libpam0g-dev (Linux, dla PAM auth)
- SQLite (dla session persistence)
- Na Red Hat / CentOS upewnij się, że zainstalowane są `gcc`, `nodejs`, `rust`, `cargo` oraz pakiet `pam-devel`

### Kompilacja

```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Check without building
cargo check --all-features

# Linting
cargo clippy --all-features -- -D warnings
```

### Features

```toml
default = ["metrics", "fast-allocator"]
metrics = ["prometheus", "lazy_static"]
database = ["sqlx"]
fast-allocator = ["mimalloc"]
```

## 🎯 Roadmap

- [x] Sprint 1: MVP (SOCKS5 protocol, auth, proxy)
- [x] Sprint 2: ACL engine + session manager
- [x] Sprint 3.1-3.8: UDP, BIND, REST API, QoS, PAM, LDAP Groups
- [x] Sprint 3.9: Web Dashboard
- [ ] Sprint 4: Production packaging, performance tuning
- [ ] Future: Grafana dashboards, Docker, systemd integration

## 📝 License

MIT License - see [LICENSE](LICENSE) file

## 🙏 Acknowledgments

- Inspirowane przez Dante SOCKS server
- Built with [Tokio](https://tokio.rs/) async runtime
- Powered by Rust 🦀

## 📞 Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/rustsocks/issues)
- **Documentation**: [docs/README.md](docs/README.md)
- **Developer Guide**: [CLAUDE.md](CLAUDE.md)

---

**Status:** 🟢 Production Ready
**Version:** 0.2.0
**Tests:** 76/76 passed ✅
**Last Updated:** 2025-10-28
