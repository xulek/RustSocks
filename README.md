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
  - PAM authentication (pam.address i pam.username) ✨ NOWE!
  - Two-tier authentication (client-level + SOCKS-level)
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
- ✅ **2.2.6 Session Metrics**
  - Prometheus: `rustsocks_active_sessions`, `rustsocks_sessions_total`, `rustsocks_sessions_rejected_total`
  - Histogram czasu trwania (`rustsocks_session_duration_seconds`) z bucketami 0.5s → 2h
  - Liczniki ruchu globalne (`rustsocks_bytes_sent_total`, `rustsocks_bytes_received_total`)
  - `IntCounterVec` per użytkownik (`rustsocks_user_sessions_total`, `rustsocks_user_bandwidth_bytes_total`)
  - `SessionManager` aktualizuje metryki na starcie, ruchu i zamknięciu oraz dla odrzuceń ACL
  - Test `session_metrics_update_counters` zabezpiecza regresje
- ✅ **2.2.7 Session Statistics API**
  - `SessionManager::get_stats(window)` agreguje dane dla konfigurowalnego okna (domyślnie 24 h rolling)
  - Zwraca liczniki: aktywne sesje, liczba i łączny ruch w oknie, top 10 użytkowników oraz destynacji
  - Wbudowane statystyki ACL (`allowed`/`blocked`) na podstawie decyzji wejściowych
  - HTTP GET `/stats` (Axum) udostępnia JSON (`?window_hours=48` nadpisuje okno)
  - Test `get_stats_aggregates_today_sessions` chroni logikę agregacji
- ✅ **2.3 IPv6 & Domain Resolution**
  - Nowy resolver (`server::resolver::resolve_address`) obsługuje IPv4/IPv6 literały i domeny (async DNS via `lookup_host`)
  - Priorytetyzuje adresy IPv6, ale próbuje wszystkie opcje zanim zgłosi błąd
  - `handle_connect` korzysta z listy kandydatów i raportuje `HostUnreachable` przy braku łączności
  - Testy jednostkowe i integracyjne pokrywają IPv4/IPv6 oraz mapowanie domen (`tests/ipv6_domain.rs`)
- ✅ **2.4 ACL + Session Integration**
  - `handle_client` tworzy sesję dopiero po pozytywnej decyzji ACL i przekazuje atrybuty reguły do `SessionManager`
  - Odmowy ACL rejestrowane są przez `track_rejected_session`, co zasila metryki i statystyki
  - Rozszerzony test integracyjny (`tests/acl_integration.rs`) obejmuje zarówno odrzucenie, jak i udany przepływ (sesja + połączenie upstream)
  - Dedykowane scenariusze `#[ignore]`: pomiar średniego narzutu <7 ms oraz stress test 1000 równoległych połączeń

### Sprint 3.6 - QoS & Rate Limiting (UKOŃCZONY) ✅

- ✅ Silnik HTB (`QosEngine`) z kubełkiem globalnym i kubełkami per użytkownik (token bucket)
- ✅ Limity pasma: gwarantowane, maksymalne oraz `burst_size` z konfiguracji TOML
- ✅ Fair sharing – rebalanser przesuwający niewykorzystane pasmo do aktywnych użytkowników
- ✅ Limity połączeń per użytkownik i globalne + automatyczne zwalnianie (`ConnectionGuard`)
- ✅ Nowe metryki Prometheus: `rustsocks_qos_active_users`, `rustsocks_qos_bandwidth_allocated_bytes_total`, `rustsocks_qos_allocation_wait_seconds`
- ✅ Testy: jednostkowe (`src/qos/htb.rs`) i integracyjne (`tests/qos_integration.rs`) potwierdzające throttling oraz równy podział pasma

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
INFO RustSocks v0.2.0 starting
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
- libpam0g-dev (budowanie + testy z integracją PAM)

### Budowanie

```bash
# Development build
cargo build

# Release build (zoptymalizowany)
cargo build --release

# Uruchom testy (wymaga systemowych bibliotek PAM)
cargo test

> ℹ️ Jednostkowe testy PAM weryfikują mapowanie kodów błędów i walidację konfiguracji — upewnij się, że pakiet `libpam0g-dev` jest zainstalowany przed uruchomieniem.
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
stats_window_hours = 24
stats_api_enabled = false
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
```

### Eksport metryk (Prometheus)

RustSocks rejestruje metryki sesji w globalnym rejestrze Prometheusa (`prometheus::default_registry()`):

- `rustsocks_active_sessions` (`IntGauge`) – aktualna liczba aktywnych sesji
- `rustsocks_sessions_total` / `rustsocks_sessions_rejected_total` (`IntCounter`) – przyjęte i odrzucone próby
- `rustsocks_session_duration_seconds` (`Histogram`) – długość życia sesji (buckety 0.5s → 2h)
- `rustsocks_bytes_sent_total` / `rustsocks_bytes_received_total` (`IntCounter`) – łączny ruch
- `rustsocks_user_sessions_total` (`IntCounterVec{user}`) – sesje per użytkownik
- `rustsocks_user_bandwidth_bytes_total` (`IntCounterVec{user,direction}`) – transfer per użytkownik i kierunek
- `rustsocks_qos_active_users` (`IntGauge`) – liczba użytkowników z aktywnymi limitami QoS
- `rustsocks_qos_bandwidth_allocated_bytes_total` (`IntCounterVec{user,direction}`) – ile bajtów zostało przydzielonych przez silnik QoS
- `rustsocks_qos_allocation_wait_seconds` (`Histogram`) – czas oczekiwania na tokeny (w sekundach) przy throttlowaniu

Eksport HTTP można zrealizować w dowolnym handlerze, np.:

```rust
use prometheus::{Encoder, TextEncoder};

let metric_families = prometheus::gather();
let mut buffer = Vec::new();
TextEncoder::new().encode(&metric_families, &mut buffer)?;
// zwróć buffer jako body `text/plain; version=0.0.4`
```

### Session Statistics API

`SessionManager::get_stats(window)` udostępnia agregaty dla dowolnego okna czasowego (`std::time::Duration`, domyślnie 24 h):

- `active_sessions` – liczba aktywnych sesji w momencie wywołania
- `total_sessions` – ile sesji (aktywnych/zamkniętych/odrzuconych) rozpoczęło się w wybranym oknie
- `total_bytes` – suma `bytes_sent + bytes_received` w zadanym oknie
- `top_users` / `top_destinations` – Top 10 użytkowników i hostów wg liczby połączeń
- `acl.allowed` / `acl.blocked` – podsumowanie decyzji ACL dla połączeń w oknie

```rust
use std::time::Duration;

let stats = session_manager
    .get_stats(Duration::from_secs(24 * 3600))
    .await;
println!(
    "Aktywne: {}, sesje w oknie: {}, bajty w oknie: {}",
    stats.active_sessions, stats.total_sessions, stats.total_bytes
);
```

Po ustawieniu `sessions.stats_api_enabled = true` serwer HTTP (domyślnie `127.0.0.1:9090`) udostępnia endpoint:

```text
GET /stats            # JSON snapshot dla domyślnego okna
GET /stats?window_hours=48
```

Przykładowa odpowiedź:

```json
{
  "generated_at": "2024-05-01T12:00:00Z",
  "active_sessions": 3,
  "total_sessions": 42,
  "total_bytes": 987654321,
  "top_users": [{"user":"alice","sessions":10}],
  "top_destinations": [{"dest_ip":"example.com","connections":7}],
  "acl": {"allowed": 40, "blocked": 2}
}
```

### REST API for Monitoring (Nowe! ✨)

RustSocks udostępnia REST API do monitorowania sesji i zarządzania. API można włączyć w konfiguracji:

```toml
[sessions]
stats_api_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
```

**Session Endpoints:**

```bash
# Get active sessions
curl http://127.0.0.1:9090/api/sessions/active

# Get session history with filtering
curl "http://127.0.0.1:9090/api/sessions/history?user=alice&hours=24&page=1&page_size=50"

# Get session statistics (top users, destinations, traffic)
curl http://127.0.0.1:9090/api/sessions/stats

# Get specific session details
curl http://127.0.0.1:9090/api/sessions/{session_id}

# Get sessions for specific user
curl http://127.0.0.1:9090/api/users/alice/sessions
```

**Management Endpoints:**

```bash
# Health check
curl http://127.0.0.1:9090/health
# Response: {"status":"healthy","version":"0.4.0","uptime_seconds":0}

# Prometheus metrics
curl http://127.0.0.1:9090/metrics
```

**API Response Example** (`/api/sessions/stats`):

```json
{
  "total_sessions": 142,
  "active_sessions": 5,
  "closed_sessions": 135,
  "failed_sessions": 2,
  "total_bytes_sent": 1234567890,
  "total_bytes_received": 9876543210,
  "top_users": [
    {
      "user": "alice",
      "session_count": 45,
      "bytes_sent": 500000000,
      "bytes_received": 300000000
    }
  ],
  "top_destinations": [
    {
      "destination": "example.com:443",
      "session_count": 20,
      "bytes_sent": 100000000,
      "bytes_received": 50000000
    }
  ]
}
```

**History with Pagination** (`/api/sessions/history`):

```json
{
  "data": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "user": "alice",
      "source_ip": "192.168.1.100",
      "source_port": 54321,
      "dest_ip": "example.com",
      "dest_port": 443,
      "protocol": "tcp",
      "status": "closed",
      "acl_decision": "allow",
      "acl_rule": "Allow HTTPS to company network",
      "bytes_sent": 1024000,
      "bytes_received": 512000,
      "start_time": "2025-10-25T10:30:00Z",
      "end_time": "2025-10-25T10:35:00Z",
      "duration_seconds": 300
    }
  ],
  "total": 142,
  "page": 1,
  "page_size": 50,
  "total_pages": 3
}
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

**Status testów:** ✅ 69/69 passed (49 unit + 2 ACL + 7 API + 4 BIND + 1 IPv6 + 1 session + 3 UDP + 2 QoS integration)

**Zakres pokrycia:**
- Protocol/Auth/Config – testy jednostkowe ✅
- ACL Engine (matcher, loader, engine, watcher) – 17+ przypadków ✅
- Session Manager & Store – cykl życia, batch writer, odrzucenia ACL ✅
- REST API – 7 endpoint tests (health, metrics, sessions, stats, history, pagination) ✅
- BIND Command – 4 integration tests ✅
- UDP ASSOCIATE – 3 integration tests ✅
- QoS / Rate limiting – testy HTB, throttling i fair sharing (2 unit + 2 integration) ✅
- Integracje: `tests/acl_integration.rs`, `tests/api_endpoints.rs`, `tests/bind_command.rs`, `tests/udp_associate.rs`, `tests/qos_integration.rs` ✅

## 🔐 PAM Authentication (Sprint 3.7 ✅)

RustSocks wspiera **PAM (Pluggable Authentication Modules)** dla elastycznej autentykacji na poziomie systemowym, zainspirowanej przez Dante SOCKS server.

### Metody autentykacji PAM

#### 1. pam.address - Autentykacja po IP
Autentykuje klientów tylko na podstawie adresu IP (bez username/password).

```toml
[auth]
client_method = "pam.address"    # Przed SOCKS handshake
# lub
socks_method = "pam.address"     # Po SOCKS handshake

[auth.pam]
address_service = "rustsocks-client"
default_user = "rhostusr"
```

**Zastosowania:**
- Zaufane sieci wewnętrzne
- ACL oparte na IP
- Defense in depth (kombinacja z innymi metodami)

#### 2. pam.username - Autentykacja username/password
Tradycyjna autentykacja SOCKS5 przez PAM.

```toml
[auth]
socks_method = "pam.username"

[auth.pam]
username_service = "rustsocks"
verbose = false
verify_service = true
```

**Uwaga:** ⚠️ SOCKS5 username/password przesyła hasła w clear-text. Używaj tylko w zaufanych sieciach lub z dodatkowym szyfrowaniem (VPN, SSH tunnel).

### Two-tier authentication (obrona w głąb)

```toml
[auth]
client_method = "pam.address"      # 1. Sprawdzenie IP przed SOCKS
socks_method = "pam.username"      # 2. Username/password po SOCKS
```

### Instalacja PAM service files

```bash
# Skopiuj przykładowe pliki do systemu
sudo cp config/pam.d/rustsocks /etc/pam.d/rustsocks
sudo cp config/pam.d/rustsocks-client /etc/pam.d/rustsocks-client

# Ustaw uprawnienia
sudo chmod 644 /etc/pam.d/rustsocks*

# Zweryfikuj konfigurację (wymaga pamtester)
pamtester rustsocks username authenticate
```

### Przykładowe pliki PAM service

**Lokalizacja:** `config/pam.d/`
- `rustsocks` - Username/password (produkcja)
- `rustsocks-client` - IP-based (produkcja)
- `rustsocks-test` - Permissive (testy)
- `rustsocks-client-test` - Permissive (testy)

**Szczegółowa dokumentacja:** `config/pam.d/README.md`

### Funkcje

- ✅ Two-tier authentication (client + SOCKS levels)
- ✅ pam.address - IP-based authentication
- ✅ pam.username - Username/password authentication
- ✅ Async PAM operations via `spawn_blocking`
- ✅ Cross-platform support (Unix + fallback)
- ✅ Configurable PAM service names
- ✅ Integration with ACL engine
- ✅ Session tracking with PAM decisions

### Testy

```bash
# Testy PAM (wymagają konfiguracji PAM)
cargo test --all-features pam -- --ignored

# Unit testy (bez PAM setup)
cargo test --all-features --lib pam
```

### Security Considerations

1. **Clear-text passwords**: SOCKS5 username/password nie jest szyfrowane
   - Używaj tylko w zaufanych sieciach
   - Rozważ TLS wrapper, VPN, lub SSH tunnel
2. **PAM service configuration**:
   - ⚠️ Brak pliku PAM service może zezwolić na wszystkie połączenia!
   - Zawsze weryfikuj `/etc/pam.d/<service>`
3. **Wymagania uprawnień**:
   - PAM wymaga zazwyczaj root dla weryfikacji haseł
   - Server powinien drop privileges po zbindowaniu socketu

**Pełna dokumentacja:** `CLAUDE.md` - sekcja "PAM Authentication"

## ⚙️ QoS & HTB Rate Limiting (Sprint 3.6 ✅)

Zaawansowana warstwa kontroli ruchu zapewnia gwarantowane pasmo dla każdego użytkownika, sprawiedliwe współdzielenie niewykorzystanej przepustowości oraz limity połączeń w ramach jednego silnika QoS.

### Kluczowe funkcje
- **Hierarchical Token Bucket (HTB)** – globalny kubełek + kubełki per użytkownik z parametrami: `guaranteed_bandwidth`, `max_bandwidth`, `burst_size`, `refill_interval_ms`.
- **Integracja z pętlą proxy** – `proxy_direction` synchronizuje się z `QosEngine::allocate_bandwidth`, dzięki czemu każde odczytane pakiety są throttlowane zanim trafią do drugiej strony.
- **Sprawiedliwe współdzielenie** – okresowy rebalanser (`rebalance_interval_ms`) monitoruje aktywność użytkowników i dynamicznie przekierowuje niewykorzystane pasmo do najbardziej obciążonych klientów, respektując limity maksymalne.
- **Limity połączeń** – `check_and_inc_connection`/`dec_user_connection` egzekwują globalne i per‑użytkownik limity jednoczesnych połączeń (zabezpieczenie anty-DDOS).
- **Obserwowalność** – metryki Prometheusa (`rustsocks_qos_active_users`, `rustsocks_qos_bandwidth_allocated_bytes_total`, `rustsocks_qos_allocation_wait_seconds`) śledzą aktywnych użytkowników QoS, przydzielone bajty oraz czasy oczekiwania na tokeny.
- **Testy jakości** – testy jednostkowe weryfikują HTB, throttling i rebalans, a testy integracyjne potwierdzają realne ograniczanie przepustowości oraz równe dzielenie pasma między wielu użytkowników.

### Konfiguracja QoS (przykład)

```toml
[qos]
enabled = true
algorithm = "htb"

[qos.htb]
global_bandwidth_bytes_per_sec = 125_000_000    # 1 Gbps
guaranteed_bandwidth_bytes_per_sec = 1_048_576  # 1 MB/s na użytkownika
max_bandwidth_bytes_per_sec = 12_500_000        # 100 Mbps przy pożyczaniu
burst_size_bytes = 1_048_576                    # 1 MB natychmiastowego transferu
refill_interval_ms = 50                         # częstotliwość uzupełniania tokenów
fair_sharing_enabled = true                     # dynamiczne współdzielenie pasma
rebalance_interval_ms = 100                     # jak często liczyć fair-share
idle_timeout_secs = 5                           # po tylu sekundach user uznany za nieaktywny

[qos.connection_limits]
max_connections_per_user = 20
max_connections_global = 10_000
```

Parametry można dostosować do przepustowości środowiska (np. mniejsze `burst_size` dla łączy o ograniczonej pojemności lub wyższe `max_connections_global` w przypadku klastrów).

### Jak działa fair sharing?
1. Każdy aktywny użytkownik otrzymuje gwarantowane minimum (`guaranteed_bandwidth`).
2. Niewykorzystane pasmo z kubełka globalnego jest proporcjonalnie dzielone pomiędzy użytkowników o największym zapotrzebowaniu, ale nigdy nie przekracza `max_bandwidth`.
3. Rebalanser ignoruje nieaktywnych klientów po `idle_timeout_secs`, dzięki czemu zasoby trafiają do realnie korzystających.
4. Wyniki rebalancingu można obserwować przez `QosEngine::get_user_allocations()` lub nowe metryki Prometheusa.

## 🎯 Roadmap

### Sprint 2 - ACL & Sessions (W TRAKCIE ⏳)
- [x] ACL Engine (rules, matching, priorities, hot reload) ✅
- [x] Session Manager (in-memory) ✅
- [x] Session persistence (SQLite + batch writer + cleanup) ✅
- [x] Traffic tracking (bytes sent/received) ✅
- [x] ACL enforcement telemetry integration z Session Manager (rozszerzenie metryk) ✅
- [x] UDP ASSOCIATE command ✅
- [x] BIND command ✅

### Sprint 3 - Production & API (W TRAKCIE) 🔄

- ✅ **Sprint 3.1 - UDP ASSOCIATE** ✅
  - UDP relay implementation
  - Packet forwarding
  - Timeout management
  - UDP session tracking
  - ACL integration

- ✅ **Sprint 3.2 - BIND Command** ✅
  - BIND implementation (reverse connections)
  - Port allocation mechanism
  - Incoming connection handling
  - ACL integration
  - 4 integration tests

- ✅ **Sprint 3.3 - REST API Core** ✅
  - **Axum server setup** with state management
  - **Session Endpoints:**
    - `GET /api/sessions/active` - List active sessions
    - `GET /api/sessions/history` - History with filtering (user, dest_ip, hours, status) & pagination
    - `GET /api/sessions/{id}` - Session details
    - `GET /api/sessions/stats` - Aggregated statistics (top users, destinations, traffic)
    - `GET /api/users/{user}/sessions` - User-specific sessions
  - **Management Endpoints:**
    - `GET /health` - Health check with version
    - `GET /metrics` - Prometheus text format metrics
    - `POST /api/admin/reload-acl` - ACL hot reload from file ✅
    - `GET /api/acl/rules` - Get ACL rules summary (user/group counts) ✅
    - `POST /api/acl/test` - Test ACL decision for user/dest/port/protocol ✅
  - **7 integration tests** for API endpoints
  - JSON request/response types with proper error handling

- ✅ **Sprint 3.6 - QoS & Rate Limiting** ✅
  - HTB silnik z kubełkami globalnymi i per użytkownik (token bucket)
  - Ograniczanie pasma w `proxy_direction` + sprawiedliwe dzielenie niewykorzystanego pasma
  - Limity połączeń (globalne i per-user) z automatycznym zwalnianiem (`ConnectionGuard`)
  - Metryki Prometheus: `rustsocks_qos_active_users`, `rustsocks_qos_bandwidth_allocated_bytes_total`, `rustsocks_qos_allocation_wait_seconds`
  - Testy: jednostkowe (`src/qos/htb.rs`) oraz integracyjne (`tests/qos_integration.rs`) pokrywające throttling i fair sharing

- ✅ **Sprint 3.7 - PAM Authentication** ✅
  - PAM integration (`pam.address` i `pam.username`)
  - Two-tier authentication (client-level + SOCKS-level)
  - Example PAM service files (`config/pam.d/`)
  - Integration tests (`tests/pam_integration.rs`)
  - Cross-platform support (Unix + fallback)
  - Dokumentacja w CLAUDE.md i config/pam.d/README.md

- [ ] **Sprint 3.8+ - Pozostałe**
  - [ ] Extended Prometheus metrics & dashboards
  - [ ] Grafana dashboards
  - [ ] systemd integration
  - [ ] Docker packaging

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

**Status:** 🟢 Sprint 1-2 UKOŃCZONE + Sprint 3.1-3.7 UKOŃCZONE! (UDP + BIND + REST API + QoS + PAM)
**Wersja:** 0.5.0 (MVP + ACL + Sessions + UDP + BIND + REST API + QoS + PAM Auth)
**Testy:** 74/74 passed ✅ (51 unit + 23 integration tests)
**Data:** 2025-10-27
