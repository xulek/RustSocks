# RustSocks - High-Performance SOCKS5 Proxy Server

🚀 Nowoczesny, wydajny serwer SOCKS5 napisany w Rust.

## ✅ Status Implementacji

### Sprint 1 - MVP (UKOŃCZONY) ✅

**Zaimplementowane funkcjonalności:**

- ✅ **SOCKS5 Protocol Parser**
  - Pełna implementacja parsowania protokołu SOCKS5
  - Obsługa CONNECT command
  - IPv4, IPv6, Domain name addressing
  - Types: ClientGreeting, ServerChoice, Socks5Request, Socks5Response

- ✅ **Authentication System**
  - No authentication (0x00)
  - Username/Password authentication (0x02, RFC 1929)
  - Konfigurowalne metody autentykacji

- ✅ **TCP Server (Tokio)**
  - Asynchroniczny server z tokio
  - Obsługa wielu jednoczesnych połączeń
  - Bidirectional proxy data transfer
  - Graceful shutdown (Ctrl+C)

- ✅ **Configuration System**
  - TOML configuration files
  - CLI arguments overrides
  - Configuration validation
  - Example config generation

- ✅ **Logging**
  - Structured logging z tracing
  - Konfigurowalne poziomy (trace, debug, info, warn, error)
  - Pretty i JSON formats

### Sprint 2.1 - ACL Engine (UKOŃCZONY) ✅

**Zaawansowany system kontroli dostępu:**

- ✅ **ACL Data Structures**
  - Action (Allow, Block)
  - Protocol filtering (TCP, UDP, Both)
  - DestinationMatcher (IP, CIDR, Domain, Wildcard)
  - PortMatcher (Single, Range, Multiple, Any)
  - Per-user i per-group rules

- ✅ **Matching Logic**
  - ✅ IP exact matching (IPv4, IPv6)
  - ✅ CIDR ranges (10.0.0.0/8, 192.168.1.0/24)
  - ✅ Domain matching (case-insensitive)
  - ✅ Wildcard domains (*.example.com, api.*.com)
  - ✅ Port ranges (8000-9000), Multiple (80,443,8080)
  - ✅ Protocol filtering

- ✅ **ACL Evaluation Engine**
  - ✅ BLOCK rules priority (zawsze pierwsze)
  - ✅ Group inheritance (users inherit group rules)
  - ✅ Default policy (allow/block)
  - ✅ Compiled rules dla performance
  - ✅ Thread-safe (Arc<RwLock>)
  - ✅ Hot reload ready (atomic swap)

- ✅ **Configuration & Validation**
  - ✅ TOML config parser
  - ✅ Config validation (duplicates, references)
  - ✅ Example config generation
  - ✅ 17/17 tests passed (>90% coverage)

- ✅ **Hot Reload (Sprint 2.1.5) ✨ NOWE!**
  - ✅ File watcher z notify crate
  - ✅ Auto-reload przy zmianie config
  - ✅ Validation przed swap
  - ✅ Rollback przy błędach
  - ✅ Reload time <100ms
  - ✅ Zero-downtime updates
  - ✅ 3/3 integration tests

### Sprint 2.1.6 - ACL Integration (UKOŃCZONE) ✅

- ✅ Serwer inicjalizuje `AclEngine` na starcie (konfiguracja `[acl]` w pliku TOML)
- ✅ Każde żądanie CONNECT przechodzi ewaluację ACL z logowaniem dopasowanej reguły
- ✅ Blokowane połączenia otrzymują `ReplyCode::ConnectionNotAllowed` i są odnotowane w `AclStats`
- ✅ Statystyki allow/block per użytkownik gotowe pod przyszłe metryki oraz Session Manager
- ✅ Test integracyjny (`tests/acl_integration.rs`) symuluje handshake SOCKS5 i weryfikuje blokadę
- ✅ Test wydajnościowy potwierdza średni czas ewaluacji ACL <5 ms

### Sprint 2.2 - Session Manager (W TRAKCIE) 🔄

- ✅ **2.2.1 Session Data Structures**
  - `Session` z pełnym zestawem pól (ID, timing, sieć, statystyki, status, ACL)
  - `SessionStatus` oraz `SessionProtocol` (serde snake_case/lowercase)
  - `ConnectionInfo` i `SessionFilter` z domyślnym limitem 100
  - Testy jednostkowe serializacji i wartości domyślnych
- ✅ **2.2.2 In-Memory Session Manager**
  - `SessionManager` oparty na `DashMap` + `RwLock`
  - Życie sesji: `new_session`, `update_traffic`, `close_session`, `get_session`
  - Liczniki ruchu i snapshoty zamkniętych/odrzuconych sesji
  - Integracja z ACL: odrzucenia logowane jako `RejectedByAcl`
- ✅ **2.2.3 Persistence (SQLite/sqlx)**
  - `SessionStore` z migracjami (`migrations/001_create_sessions_table.sql`)
  - Upsert sesji (nowe, ruch, zamknięcie, odrzucenia ACL)
  - Dynamiczne filtrowanie (`SessionFilter`) po user/time/dest/status/min_bytes
  - Konfiguracja `[sessions]` (storage, database_url, batch_* oraz retention/cleanup)
  - Test integracyjny `session::store` na `sqlite::memory:` (`cargo test --features database`)
- ✅ **2.2.4 Batch Writer**
  - `BatchWriter` z kolejką `Mutex<Vec<Session>>`
  - Auto-flush przy osiągnięciu `batch_size` oraz okresowe flush (`batch_interval_ms`)
  - Backpressure poprzez `Notify` (zero busy-loop)
  - Integracja z `SessionManager::new_session/update_traffic/close_session/track_rejected_session`
  - Cleanup task (`SessionStore::spawn_cleanup`) usuwa stare rekordy wg `retention_days`
- ✅ **2.2.5 Traffic Tracking**
  - Proxy loop emituje aktualizacje ruchu do `SessionManager` (upload/download + pakiety)
  - Konfigurowalny próg `traffic_update_packet_interval` ogranicza częstotliwość aktualizacji
  - Finałowy flush na zamknięciu kanałów zapewnia brak utraty danych metrycznych
  - Integracja dwukierunkowa: liczniki `bytes_sent/received` i `packets_sent/received`
  - Nowy test integracyjny (`tests/session_tracking.rs`) weryfikuje flush przy zamknięciu

## 🎯 Weryfikacja Działania

Serwer został **pomyślnie przetestowany** z curl:

```bash
# Start serwera
./target/release/rustsocks --bind 127.0.0.1 --port 1080

# Test z curl
curl -x socks5://127.0.0.1:1080 http://example.com
# ✅ Status: 200 OK - Działa poprawnie!
```

**Logi serwera:**
```
INFO RustSocks v0.1.0 starting
INFO RustSocks server listening on 127.0.0.1:1080
INFO Authentication method: none
INFO New connection from 127.0.0.1:47554
INFO SOCKS5 request: command=Connect, dest=23.220.75.245:80
INFO Connected to 23.220.75.245:80, proxying data
```

## 📦 Instalacja

### Wymagania
- Rust 1.70+ (zainstalowano: 1.90.0)
- Linux/WSL

### Budowanie

```bash
# Development build
cargo build

# Release build (zoptymalizowany)
cargo build --release

# Uruchom testy
cargo test
```

## 🚀 Użycie

### Podstawowe uruchomienie

```bash
# Z domyślnymi ustawieniami (127.0.0.1:1080, no-auth)
./target/release/rustsocks

# Z konfiguracją z pliku
./target/release/rustsocks --config config/rustsocks.toml

# Z override parametrów
./target/release/rustsocks --bind 0.0.0.0 --port 1080

# Wygeneruj przykładowy plik konfiguracji
./target/release/rustsocks --generate-config config/rustsocks.toml
```

### Przykładowa konfiguracja

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 1000

[auth]
method = "none"  # Options: "none", "userpass"

# Dla userpass authentication:
[[auth.users]]
username = "alice"
password = "secret123"

[logging]
level = "info"
format = "pretty"

[sessions]
enabled = false
storage = "memory"  # Opcje: "memory", "sqlite"
# database_url = "sqlite://var/lib/rustsocks/sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24
traffic_update_packet_interval = 10
```

### ACL Configuration (Nowe! ✨)

```toml
# config/acl.toml
[global]
default_policy = "block"  # Deny by default

[[users]]
username = "alice"
groups = ["developers"]

  # BLOCK rules have highest priority
  [[users.rules]]
  action = "block"
  description = "Block admin panel"
  destinations = ["admin.company.com"]
  ports = ["*"]
  priority = 1000

  [[users.rules]]
  action = "allow"
  description = "Allow HTTPS to company network"
  destinations = ["10.0.0.0/8", "192.168.0.0/16"]
  ports = ["443", "8000-9000"]
  protocols = ["tcp"]
  priority = 100

[[groups]]
name = "developers"

  [[groups.rules]]
  action = "allow"
  description = "Dev environments"
  destinations = ["*.dev.company.com"]
  ports = ["*"]
  priority = 50
```

**Features:**
- ✅ Per-user i per-group rules
- ✅ BLOCK priority (zawsze pierwsze)
- ✅ CIDR ranges (10.0.0.0/8)
- ✅ Wildcard domains (*.dev.company.com)
- ✅ Port ranges (8000-9000)
- ✅ Group inheritance
- ✅ Hot reload (zero-downtime)

**Hot Reload Example:**
```rust
use rustsocks::acl::{AclEngine, AclWatcher};
use std::sync::Arc;

// Create engine
let engine = Arc::new(AclEngine::new(acl_config)?);

// Start watcher (auto-reloads on file change)
let mut watcher = AclWatcher::new("config/acl.toml".into(), engine.clone());
watcher.start().await?;

// Config changes are automatically applied with:
// - Validation before swap
// - Rollback on error
// - <100ms reload time
```

### Testowanie z klientami

```bash
# curl
curl -x socks5://127.0.0.1:1080 http://example.com

# curl z autentykacją
curl -x socks5://alice:secret123@127.0.0.1:1080 http://example.com

# proxychains
proxychains4 curl http://example.com

# SSH przez proxy
ssh -o ProxyCommand='nc -X 5 -x 127.0.0.1:1080 %h %p' user@remote-host
```

## 📁 Struktura Projektu

```
rustsocks/
├── src/
│   ├── main.rs           # Entry point, CLI
│   ├── lib.rs            # Library exports
│   ├── protocol/         # SOCKS5 protocol
│   │   ├── types.rs      # Protocol structures
│   │   └── parser.rs     # Parsing logic
│   ├── auth/             # Authentication
│   │   └── mod.rs        # Auth manager
│   ├── acl/              # ACL Engine ✨
│   │   ├── types.rs      # ACL structures
│   │   ├── matcher.rs    # Matching logic
│   │   ├── engine.rs     # Evaluation engine
│   │   ├── loader.rs     # Config loading
│   │   └── watcher.rs    # Hot reload ✨ NEW
│   ├── server/           # Server logic
│   │   ├── listener.rs   # TCP listener
│   │   ├── handler.rs    # Connection handler
│   │   └── proxy.rs      # Data proxying
│   ├── config/           # Configuration
│   │   └── mod.rs        # Config loading
│   └── utils/            # Utilities
│       └── error.rs      # Error types
├── migrations/           # sqlx migrations (sessions)
├── tests/                # Integration tests
├── config/               # Config examples
├── Cargo.toml           # Dependencies
└── README.md            # Ta dokumentacja
```

## 🧪 Testy

```bash
# Uruchom wszystkie testy (domyślna konfiguracja)
cargo test

# Testy z rozszerzeniem bazy danych (sqlx + SQLite)
cargo test --features database

# Testy z wyświetlaniem logów
cargo test -- --nocapture
```

**Status testów:** ✅ 37/37 (default) · ✅ 38/38 (`--features database`)

**Zakres pokrycia:**
- Protocol/Auth/Config – testy jednostkowe ✅
- ACL Engine (matcher, loader, engine, watcher) – 17+ przypadków ✅
- Session Manager & Store – cykl życia, batch writer, odrzucenia ACL ✅
- Integracje: `tests/acl_integration.rs`, `session::store` (SQLite in-memory) ✅

## 🎯 Roadmap

### Sprint 2 - ACL & Sessions (W TRAKCIE ⏳)
- [x] ACL Engine (rules, matching, priorities, hot reload) ✅
- [x] Session Manager (in-memory) ✅
- [x] Session persistence (SQLite + batch writer + cleanup) ✅
- [ ] Traffic tracking (bytes sent/received)
- [ ] ACL enforcement telemetry integration z Session Manager (rozszerzenie metryk)
- [ ] BIND command
- [ ] UDP ASSOCIATE command

### Sprint 3 - Production & API
- [ ] REST API dla monitoringu
- [ ] Prometheus metrics
- [ ] Grafana dashboards
- [ ] systemd integration
- [ ] Docker packaging
- [ ] PAM authentication

## 📊 Performance

**Obecne możliwości:**
- Asynchroniczny I/O (tokio)
- Zero-copy data transfer gdzie możliwe
- Minimal memory allocations
- Wydajne parsowanie protokołu

**Docelowe cele (Sprint 2/3):**
- 5000+ concurrent connections
- <50ms latency (p99)
- <5ms ACL check overhead
- >1000 sessions/sec database writes

## 🤝 Contributing

1. Fork the repository
2. Create feature branch (`git checkout -b feature/amazing`)
3. Commit changes (`git commit -m 'Add amazing feature'`)
4. Push to branch (`git push origin feature/amazing`)
5. Open Pull Request

## 📝 License

MIT License

## 🙏 Acknowledgments

- Inspirowane przez Dante SOCKS server
- Zbudowane z pomocą Claude AI (Anthropic)
- Tokio async runtime
- Rust community

## 📞 Support

- Issues: [GitHub Issues](https://github.com/yourusername/rustsocks/issues)
- Dokumentacja: [docs/](docs/)

---

**Status:** 🟢 Sprint 1 MVP + Sprint 2.1 ACL + Sprint 2.1.5 Hot Reload UKOŃCZONE!
**Wersja:** 0.2.1 (MVP + ACL Engine + Hot Reload)
**Testy:** 37/37 passed (default) ✅ · `cargo test --features database` → 38/38 ✅
**Data:** 2025-10-24
