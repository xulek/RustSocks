# Projekt: RustSocks - Wydajny Serwer SOCKS5

## 🎯 Cel Projektu

Stworzenie wydajnego, skalowalnego serwera SOCKS5 w Rust, zdolnego do obsługi tysięcy równoległych połączeń z naciskiem na:
- **Wydajność** - wykorzystanie async/await i io_uring
- **Bezpieczeństwo** - memory safety dzięki Rust + zaawansowane ACL
- **Łatwość użycia** - prosty config, czytelne logi
- **Produkcyjność** - monitoring, graceful shutdown, hot reload
- **Granularna kontrola dostępu** - per-user ACL z allow/block rules
- **Szczegółowe audytowanie** - kompletny tracking wszystkich sesji

## 📈 Executive Summary - Timeline z AI

### Oszacowany czas realizacji (z pomocą AI):
- **Sprint 1 - MVP + PAM:** 2.5 tygodni (SOCKS5 CONNECT + auth + PAM)  ⭐
- **Sprint 2 - ACL + Sessions:** 3 tygodnie (zaawansowane ACL + session tracking)
- **Sprint 3 - Production + API:** 3 tygodnie (REST API + monitoring + deployment)
- **TOTAL:** ~8.5 tygodni (330h roboczych)

### Kluczowe komponenty:
✅ **PAM Authentication** - pam.address + pam.username (jak Dante)  
✅ **ACL Engine** - per-user rules, CIDR, wildcards, hot reload  
✅ **Session Manager** - kompletny tracking, database persistence  
✅ **REST API** - real-time monitoring, session queries  
✅ **Extended Metrics** - Prometheus + Grafana dashboards  
✅ **Privilege Management** - bezpieczne root → unprivileged dropping  

### Przewaga konkurencyjna vs. Dante:
- 🚀 Nowoczesny async Rust (vs. C)
- 🔒 Memory safety z natury
- 🔐 **Full PAM support (pam.address + pam.username)** ⭐
- 📊 Built-in monitoring i REST API
- ⚡ Zaawansowane ACL z hot reload
- 📈 Detailed session tracking
- 🎯 Łatwiejsza konfiguracja i maintenance
- 🛡️ **Bezpieczne privilege dropping** ⭐

## 📋 Specyfikacja Funkcjonalna

### Wersja 0.1 - MVP (Milestone 1)
- [x] SOCKS5 CONNECT command
- [x] No authentication (0x00)
- [x] Username/Password authentication (0x02)
- [x] **PAM authentication (pam.address + pam.username)** ⭐
- [x] IPv4 addressing
- [x] Podstawowy config file (TOML)
- [x] Logging do stdout/stderr
- [x] Graceful shutdown
- [x] **Privilege dropping (root → unprivileged)** ⭐

### Wersja 0.5 - Beta (Milestone 2)
- [ ] IPv6 addressing
- [ ] Domain name resolution
- [ ] UDP ASSOCIATE command
- [ ] Connection pooling/reuse
- [ ] **Zaawansowane ACL (per-user, allow/block, CIDR, wildcards)** ⭐
- [ ] **Hot reload ACL bez wpływu na aktywne sesje** ⭐
- [ ] **Session tracking (in-memory)** ⭐
- [ ] **Session history (database)** ⭐
- [ ] **Per-user bandwidth tracking** ⭐
- [ ] Structured logging (JSON)
- [ ] Basic metrics (Prometheus)

### Wersja 1.0 - Production (Milestone 3)
- [ ] BIND command
- [ ] Multiple authentication backends
- [ ] Hot config reload (wszystkie komponenty)
- [ ] systemd integration
- [ ] Rate limiting
- [ ] Connection timeout management
- [ ] **REST API dla monitoringu sesji** ⭐
- [ ] **Extended metrics (per-user, per-destination, ACL stats)** ⭐
- [ ] **Grafana dashboards** ⭐
- [ ] Health check endpoint
- [ ] Comprehensive documentation

### Wersja 1.5+ - Advanced (Future)
- [ ] Traffic shaping
- [ ] Geo-blocking
- [ ] Web dashboard
- [ ] Clustering/HA
- [ ] Traffic encryption (SOCKS over TLS)

## 🔐 System Kontroli Dostępu (ACL)

### Wymagania ACL

#### Hierarchia Reguł (Priority Order)
```
1. BLOCK rules (najwyższy priorytet)
2. ALLOW rules
3. Default policy (domyślnie DENY)
```

**Kluczowa zasada:** Jeśli jakakolwiek reguła BLOCK pasuje - połączenie jest odrzucane, niezależnie od reguł ALLOW.

#### Granularność Kontroli

Per-user rules wspierające:
- **Pojedyncze IP:** `192.168.1.100`
- **Zakresy CIDR:** `10.0.0.0/8`, `172.16.0.0/12`
- **Wildcard domains:** `*.example.com`, `api.*.com`
- **Porty:** pojedyncze `443`, zakresy `8000-9000`, multiple `80,443,8080`
- **Protokoły:** TCP, UDP, lub oba

#### Format Konfiguracji ACL

```toml
# config/acl.toml

# Globalne default policy
[global]
default_policy = "deny"  # "allow" or "deny"

# Reguły per-user
[[users]]
username = "alice"
groups = ["developers", "ssh-users"]

  [[users.rules]]
  action = "allow"
  description = "Allow access to company network"
  destinations = ["10.0.0.0/8"]
  ports = ["22", "80", "443", "3000-4000"]
  protocols = ["tcp", "udp"]
  
  [[users.rules]]
  action = "allow"
  description = "Allow access to production servers"
  destinations = ["prod-*.company.com", "192.168.100.0/24"]
  ports = ["443", "5432"]
  protocols = ["tcp"]
  
  [[users.rules]]
  action = "block"
  description = "Block access to internal admin panel"
  destinations = ["admin.company.com", "192.168.100.10"]
  ports = ["*"]
  priority = 1000  # Higher = evaluated first

[[users]]
username = "bob"
groups = ["readonly"]

  [[users.rules]]
  action = "allow"
  description = "Read-only database access"
  destinations = ["db-replica.company.com"]
  ports = ["5432"]
  protocols = ["tcp"]
  
  [[users.rules]]
  action = "block"
  description = "Block all write operations"
  destinations = ["db-master.company.com"]
  ports = ["*"]

# Reguły grupowe (dziedziczone przez wszystkich w grupie)
[[groups]]
name = "developers"

  [[groups.rules]]
  action = "allow"
  description = "Access to dev environments"
  destinations = ["*.dev.company.com", "10.1.0.0/16"]
  ports = ["*"]

[[groups]]
name = "readonly"

  [[groups.rules]]
  action = "block"
  description = "Block SSH access"
  destinations = ["*"]
  ports = ["22"]
```

#### ACL Evaluation Algorithm

```rust
// Pseudokod
fn evaluate_acl(user: &User, dest: &Address, port: u16) -> Decision {
    // 1. Zbierz wszystkie reguły (user + grupy)
    let mut rules = collect_rules(user);
    
    // 2. Sortuj po priorytecie (BLOCK > ALLOW)
    rules.sort_by_priority();
    
    // 3. Ewaluuj w kolejności
    for rule in rules {
        if rule.matches(dest, port) {
            match rule.action {
                Action::Block => return Decision::Deny,
                Action::Allow => return Decision::Allow,
            }
        }
    }
    
    // 4. Default policy
    return global_default_policy();
}
```

### Hot Reload ACL (Zero-Downtime)

**Wymaganie:** Zmiana reguł ACL nie może przerwać aktywnych połączeń.

**Implementacja:**
```rust
// ACL Manager with Arc<RwLock> dla thread-safety
pub struct AclManager {
    rules: Arc<RwLock<AclRules>>,
    watcher: FileWatcher,
}

impl AclManager {
    pub async fn reload(&self) -> Result<()> {
        // 1. Wczytaj nową konfigurację
        let new_rules = AclRules::load_from_file("acl.toml")?;
        
        // 2. Waliduj (czy nie ma błędów składni)
        new_rules.validate()?;
        
        // 3. Atomic swap - tylko write lock na krótki moment
        {
            let mut rules = self.rules.write().await;
            *rules = new_rules;
        } // Write lock released
        
        // 4. Aktywne połączenia używają starej wersji do końca
        // Nowe połączenia używają nowej wersji
        
        info!("ACL rules reloaded successfully");
        Ok(())
    }
}

// W connection handler
pub async fn handle_connection(
    stream: TcpStream,
    acl: Arc<AclManager>,
) {
    // Pobierz snapshot reguł dla tego połączenia
    let rules_snapshot = acl.rules.read().await.clone();
    
    // Używaj snapshot przez całe życie połączenia
    // Nawet jak ACL się zmieni, to połączenie używa starej wersji
}
```

**Mechanizm File Watching:**
```rust
use notify::{Watcher, RecursiveMode};

pub struct AclFileWatcher {
    watcher: RecommendedWatcher,
}

impl AclFileWatcher {
    pub fn watch(path: &Path, acl_manager: Arc<AclManager>) {
        let (tx, rx) = channel();
        
        let mut watcher = notify::watcher(tx, Duration::from_secs(2))?;
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        
        tokio::spawn(async move {
            while let Ok(event) = rx.recv() {
                match event {
                    DebouncedEvent::Write(_) | DebouncedEvent::Create(_) => {
                        info!("ACL config changed, reloading...");
                        if let Err(e) = acl_manager.reload().await {
                            error!("Failed to reload ACL: {}", e);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}
```

## 📊 System Monitoringu Sesji

### Session Tracking - Wymagania

Każda sesja SOCKS musi być śledzona z następującymi danymi:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    // Identyfikacja
    pub session_id: Uuid,
    pub user: String,
    
    // Timing
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_secs: Option<u64>,
    
    // Network
    pub source_ip: IpAddr,
    pub source_port: u16,
    pub dest_ip: IpAddr,
    pub dest_port: u16,
    pub protocol: Protocol,  // TCP/UDP
    
    // Traffic stats
    pub bytes_sent: u64,      // client -> upstream
    pub bytes_received: u64,  // upstream -> client
    pub packets_sent: u64,
    pub packets_received: u64,
    
    // Status
    pub status: SessionStatus,  // Active, Closed, Failed, Rejected
    pub close_reason: Option<String>,
    
    // ACL decision
    pub acl_rule_matched: Option<String>,
    pub acl_decision: AclDecision,
}

#[derive(Debug, Clone, Serialize)]
pub enum SessionStatus {
    Active,
    Closed,
    Failed(String),
    RejectedByAcl,
}
```

### Session Manager Architecture

```rust
pub struct SessionManager {
    // Active sessions (in-memory)
    active_sessions: Arc<DashMap<Uuid, Session>>,
    
    // Historical sessions (persistent storage)
    history_store: SessionHistoryStore,
    
    // Metrics aggregator
    metrics: SessionMetrics,
    
    // Cleanup task
    cleanup_interval: Duration,
}

impl SessionManager {
    pub fn new_session(&self, user: &str, conn_info: ConnectionInfo) -> Uuid {
        let session = Session {
            session_id: Uuid::new_v4(),
            user: user.to_string(),
            start_time: Utc::now(),
            source_ip: conn_info.source_ip,
            dest_ip: conn_info.dest_ip,
            // ...
            status: SessionStatus::Active,
            bytes_sent: 0,
            bytes_received: 0,
        };
        
        let session_id = session.session_id;
        self.active_sessions.insert(session_id, session);
        
        // Update metrics
        self.metrics.active_sessions.inc();
        self.metrics.total_sessions.inc();
        
        session_id
    }
    
    pub fn update_traffic(&self, session_id: &Uuid, bytes_sent: u64, bytes_recv: u64) {
        if let Some(mut session) = self.active_sessions.get_mut(session_id) {
            session.bytes_sent += bytes_sent;
            session.bytes_received += bytes_recv;
            
            // Update metrics
            self.metrics.total_bytes_sent.add(bytes_sent);
            self.metrics.total_bytes_received.add(bytes_recv);
        }
    }
    
    pub async fn close_session(&self, session_id: &Uuid, reason: Option<String>) {
        if let Some((_, mut session)) = self.active_sessions.remove(session_id) {
            session.end_time = Some(Utc::now());
            session.duration_secs = Some(
                (session.end_time.unwrap() - session.start_time).num_seconds() as u64
            );
            session.close_reason = reason;
            session.status = SessionStatus::Closed;
            
            // Persist to history
            self.history_store.save(session).await;
            
            // Update metrics
            self.metrics.active_sessions.dec();
        }
    }
    
    pub fn get_active_sessions(&self) -> Vec<Session> {
        self.active_sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
    
    pub async fn query_history(&self, filter: SessionFilter) -> Vec<Session> {
        self.history_store.query(filter).await
    }
}
```

### Session History Storage

**Opcje storage:**

1. **SQLite** (rekomendowane dla start):
   - Prosty setup
   - Dobre dla średnich wolumenów (1M sessions/day)
   - Queries z SQL

2. **PostgreSQL** (dla większej skali):
   - Lepsze performance dla >10M sessions/day
   - Advanced queries
   - Replikacja

3. **ClickHouse** (dla bardzo dużej skali):
   - Kolumnowa baza dla analytics
   - Świetne dla time-series data
   - Kompresja

**Schema (SQL):**
```sql
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    user TEXT NOT NULL,
    start_time TIMESTAMP NOT NULL,
    end_time TIMESTAMP,
    duration_secs INTEGER,
    
    source_ip TEXT NOT NULL,
    source_port INTEGER NOT NULL,
    dest_ip TEXT NOT NULL,
    dest_port INTEGER NOT NULL,
    protocol TEXT NOT NULL,
    
    bytes_sent BIGINT DEFAULT 0,
    bytes_received BIGINT DEFAULT 0,
    packets_sent BIGINT DEFAULT 0,
    packets_received BIGINT DEFAULT 0,
    
    status TEXT NOT NULL,
    close_reason TEXT,
    
    acl_rule_matched TEXT,
    acl_decision TEXT,
    
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indeksy dla częstych queries
CREATE INDEX idx_sessions_user ON sessions(user);
CREATE INDEX idx_sessions_start_time ON sessions(start_time DESC);
CREATE INDEX idx_sessions_dest_ip ON sessions(dest_ip);
CREATE INDEX idx_sessions_status ON sessions(status);
```

### Real-Time Session Monitoring API

**REST API dla monitoringu:**

```rust
// GET /api/sessions/active
// Response: Lista aktywnych sesji
[
  {
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "user": "alice",
    "start_time": "2025-10-21T10:30:00Z",
    "duration_secs": 1234,
    "source_ip": "192.168.1.100",
    "dest_ip": "93.184.216.34",
    "dest_port": 443,
    "bytes_sent": 102400,
    "bytes_received": 2048000,
    "status": "active"
  }
]

// GET /api/sessions/history?user=alice&start=2025-10-20&end=2025-10-21
// Response: Historia sesji z filtrowaniem

// GET /api/sessions/{session_id}
// Response: Szczegóły konkretnej sesji

// GET /api/sessions/stats
// Response: Agregowane statystyki
{
  "active_sessions": 1234,
  "total_sessions": 5678,
  "total_bytes": 1099511627776,
  "top_users": [
    {"user": "alice", "sessions": 234},
    {"user": "bob", "sessions": 123}
  ],
  "top_destinations": [
    {"ip": "93.184.216.34", "connections": 456}
  ]
}
```

### Prometheus Metrics - Extended

```rust
// Metrics oprócz podstawowych
lazy_static! {
    // Session metrics
    pub static ref ACTIVE_SESSIONS: IntGauge = 
        register_int_gauge!("rustsocks_active_sessions", "Active sessions").unwrap();
    
    pub static ref TOTAL_SESSIONS: IntCounter = 
        register_int_counter!("rustsocks_total_sessions", "Total sessions").unwrap();
    
    pub static ref SESSION_DURATION_SECONDS: Histogram = 
        register_histogram!("rustsocks_session_duration_seconds", "Session duration").unwrap();
    
    // Traffic metrics
    pub static ref BYTES_SENT_TOTAL: IntCounter = 
        register_int_counter!("rustsocks_bytes_sent_total", "Total bytes sent").unwrap();
    
    pub static ref BYTES_RECEIVED_TOTAL: IntCounter = 
        register_int_counter!("rustsocks_bytes_received_total", "Total bytes received").unwrap();
    
    // ACL metrics
    pub static ref ACL_DECISIONS: IntCounterVec = 
        register_int_counter_vec!(
            "rustsocks_acl_decisions_total", 
            "ACL decisions",
            &["user", "decision"]
        ).unwrap();
    
    pub static ref ACL_RELOAD_TIME: Histogram = 
        register_histogram!("rustsocks_acl_reload_seconds", "ACL reload time").unwrap();
    
    // Per-user metrics
    pub static ref USER_SESSIONS: IntGaugeVec = 
        register_int_gauge_vec!(
            "rustsocks_user_active_sessions",
            "Active sessions per user",
            &["user"]
        ).unwrap();
    
    pub static ref USER_BANDWIDTH: IntCounterVec = 
        register_int_counter_vec!(
            "rustsocks_user_bytes_total",
            "Total bytes per user",
            &["user", "direction"]
        ).unwrap();
}
```

### Grafana Dashboard Template

```json
{
  "dashboard": {
    "title": "RustSocks Monitoring",
    "panels": [
      {
        "title": "Active Sessions",
        "targets": [{"expr": "rustsocks_active_sessions"}],
        "type": "graph"
      },
      {
        "title": "Sessions Rate",
        "targets": [{"expr": "rate(rustsocks_total_sessions[5m])"}],
        "type": "graph"
      },
      {
        "title": "Bandwidth (MB/s)",
        "targets": [
          {"expr": "rate(rustsocks_bytes_sent_total[1m])/1024/1024"},
          {"expr": "rate(rustsocks_bytes_received_total[1m])/1024/1024"}
        ],
        "type": "graph"
      },
      {
        "title": "Top Users by Sessions",
        "targets": [{"expr": "topk(10, rustsocks_user_active_sessions)"}],
        "type": "table"
      },
      {
        "title": "ACL Decisions",
        "targets": [{"expr": "rustsocks_acl_decisions_total"}],
        "type": "pie"
      }
    ]
  }
}
```

## 🏗️ Architektura Techniczna

### Stack Technologiczny

```
┌─────────────────────────────────────────────┐
│         RustSocks Server                    │
├─────────────────────────────────────────────┤
│  Tokio Runtime (async/await)                │
│  ├─ TCP Listener (accept loop)              │
│  ├─ Connection Handler Pool                 │
│  └─ Upstream Connector                      │
├─────────────────────────────────────────────┤
│  Core Components:                           │
│  ├─ SOCKS5 Protocol Parser                  │
│  ├─ Authentication Manager                  │
│  ├─ ACL Engine (with hot reload) ⭐ NEW     │
│  ├─ Session Manager ⭐ NEW                   │
│  │   ├─ Active Sessions Tracker             │
│  │   ├─ Traffic Counter                     │
│  │   └─ History Store (SQLite/PostgreSQL)   │
│  ├─ Metrics Collector (Extended)            │
│  └─ Config Manager                          │
├─────────────────────────────────────────────┤
│  Infrastructure:                            │
│  ├─ tracing (logging)                       │
│  ├─ prometheus (metrics)                    │
│  ├─ serde (config)                          │
│  ├─ clap (CLI)                              │
│  ├─ sqlx (database) ⭐ NEW                   │
│  ├─ notify (file watching) ⭐ NEW           │
│  └─ axum (REST API) ⭐ NEW                   │
└─────────────────────────────────────────────┘

Connection Flow with ACL & Session Tracking:
┌────────┐     ┌─────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│ Client │────▶│ Auth    │────▶│   ACL    │────▶│ Session  │────▶│ Upstream │
│        │     │ Manager │     │ Engine   │     │ Manager  │     │ Server   │
└────────┘     └─────────┘     └──────────┘     └──────────┘     └──────────┘
                                     │                │
                                     ▼                ▼
                               [BLOCK/ALLOW]   [Track Traffic]
                                     │                │
                                     └────────────────┴──────▶ Metrics/Logs
```

### Kluczowe Dependencje

```toml
[dependencies]
# Async runtime
tokio = { version = "1.35", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }

# Networking
bytes = "1.5"
futures = "0.3"

# Configuration
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
config = "0.14"

# Logging & Metrics
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
prometheus = "0.13"

# CLI
clap = { version = "4.4", features = ["derive"] }

# Error handling
thiserror = "1.0"
anyhow = "1.0"

# Security
argon2 = "0.5"  # password hashing
ring = "0.17"    # crypto primitives

# ⭐ NEW: PAM Authentication
pam = "0.7"      # PAM bindings for Rust
nix = "0.27"     # UNIX system calls (setuid, setgid, capabilities)
caps = "0.5"     # Linux capabilities management

# ⭐ NEW: ACL & Session Management
notify = "6.1"   # file watching for hot reload
ipnet = "2.9"    # CIDR parsing and matching
glob = "0.3"     # wildcard domain matching
dashmap = "5.5"  # concurrent hashmap for active sessions
uuid = { version = "1.6", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

# ⭐ NEW: Database for session history
sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "sqlite", "chrono", "uuid"] }
# Alternative for PostgreSQL:
# sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "postgres", "chrono", "uuid"] }

# ⭐ NEW: REST API for monitoring
axum = "0.7"
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace"] }
serde_json = "1.0"

[dev-dependencies]
criterion = "0.5"  # benchmarking
proptest = "1.4"   # property testing
tokio-test = "0.4"
mockall = "0.12"   # mocking dla testów
```

### Struktura Katalogów

```
rustsocks/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── .github/
│   └── workflows/
│       ├── ci.yml
│       └── release.yml
├── benches/
│   ├── connection_bench.rs
│   ├── parser_bench.rs
│   └── acl_bench.rs  ⭐ NEW
├── config/
│   ├── rustsocks.example.toml
│   ├── acl.example.toml  ⭐ NEW
│   ├── users.example.txt
│   └── systemd/
│       └── rustsocks.service
├── migrations/  ⭐ NEW
│   └── 001_create_sessions_table.sql
├── docs/
│   ├── architecture.md
│   ├── configuration.md
│   ├── acl-guide.md  ⭐ NEW
│   ├── pam-authentication.md  ⭐ NEW
│   ├── monitoring.md  ⭐ NEW
│   └── deployment.md
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── config/
│   │   ├── mod.rs
│   │   └── types.rs
│   ├── protocol/
│   │   ├── mod.rs
│   │   ├── socks5.rs
│   │   ├── parser.rs
│   │   └── types.rs
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── none.rs
│   │   ├── userpass.rs
│   │   ├── pam.rs          ⭐ NEW
│   │   ├── manager.rs
│   │   └── privilege.rs    ⭐ NEW (privilege dropping)
│   ├── server/
│   │   ├── mod.rs
│   │   ├── listener.rs
│   │   ├── handler.rs
│   │   └── proxy.rs
│   ├── acl/  ⭐ EXTENDED
│   │   ├── mod.rs
│   │   ├── engine.rs        # ACL evaluation engine
│   │   ├── rules.rs         # Rule definitions
│   │   ├── matcher.rs       # IP/domain/port matching
│   │   ├── loader.rs        # Config loading
│   │   ├── watcher.rs       # File watching for hot reload
│   │   └── tests.rs
│   ├── session/  ⭐ NEW
│   │   ├── mod.rs
│   │   ├── manager.rs       # Session lifecycle management
│   │   ├── tracker.rs       # Traffic tracking
│   │   ├── store.rs         # Database storage
│   │   ├── types.rs         # Session data structures
│   │   └── query.rs         # Query API
│   ├── api/  ⭐ NEW
│   │   ├── mod.rs
│   │   ├── handlers.rs      # REST API handlers
│   │   ├── routes.rs        # Route definitions
│   │   └── types.rs         # API request/response types
│   ├── metrics/
│   │   ├── mod.rs
│   │   └── collector.rs
│   ├── utils/
│   │   ├── mod.rs
│   │   └── error.rs
│   └── tests/
│       ├── integration_tests.rs
│       ├── protocol_tests.rs
│       ├── acl_tests.rs     ⭐ NEW
│       └── session_tests.rs ⭐ NEW
└── tests/
    ├── e2e/
    │   ├── basic_connect.rs
    │   ├── authentication.rs
    │   ├── acl_enforcement.rs  ⭐ NEW
    │   └── session_tracking.rs ⭐ NEW
    └── load/
        └── stress_test.rs
```

## 📅 Szczegółowy Harmonogram

### Ogólny Przegląd
- **Sprint 1:** Fundament + MVP + PAM (Tydzień 1-2.5) - 90h  ⭐ EXTENDED
- **Sprint 2:** Performance + ACL + Sessions (Tydzień 3-5.5) - 120h
- **Sprint 3:** Production + API (Tydzień 6-8.5) - 120h
- **TOTAL:** 8.5 tygodni, ~330h roboczych

### Sprint 1: Fundament (Tydzień 1-2.5) - 90h  ⭐ EXTENDED (was 80h)

#### Dzień 1-2: Setup projektu (10h)
- [ ] Inicjalizacja repo, Cargo.toml
- [ ] Struktura katalogów
- [ ] CI/CD pipeline (GitHub Actions)
- [ ] Pre-commit hooks (rustfmt, clippy)
- [ ] README z podstawową dokumentacją

**Deliverable:** Skeleton projektu kompilujący się

#### Dzień 3-5: SOCKS5 Protocol Parser (20h)
- [ ] Definicja struktur protokołu
- [ ] Parser handshake'u
- [ ] Parser CONNECT request
- [ ] Parser response format
- [ ] Unit testy dla parsera
- [ ] Proptest dla edge cases

**Deliverable:** Parser obsługujący pełny handshake

**Przykładowy kod:**
```rust
// src/protocol/types.rs
#[derive(Debug, Clone, Copy)]
pub enum SocksVersion {
    V5 = 0x05,
}

#[derive(Debug, Clone, Copy)]
pub enum AuthMethod {
    NoAuth = 0x00,
    UserPass = 0x02,
    NoAcceptable = 0xFF,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Connect = 0x01,
    Bind = 0x02,
    UdpAssociate = 0x03,
}

#[derive(Debug, Clone)]
pub enum Address {
    IPv4([u8; 4]),
    IPv6([u8; 16]),
    Domain(String),
}

// src/protocol/parser.rs
pub async fn parse_client_greeting(
    stream: &mut TcpStream
) -> Result<Vec<AuthMethod>> {
    // Implementation
}
```

#### Dzień 6-8: Basic Server (25h)
- [ ] Tokio TCP listener
- [ ] Accept loop
- [ ] Basic connection handler
- [ ] No-auth flow end-to-end
- [ ] Error handling
- [ ] Graceful shutdown

**Deliverable:** Działający serwer SOCKS5 (no-auth tylko)

#### Dzień 9-11: Authentication System (25h)  ⭐ EXTENDED
- [ ] RFC 1929 Username/Password implementation
- [ ] Hardcoded credentials (config file)
- [ ] **PAM integration (libpam bindings)** ⭐
- [ ] **pam.address method (IP-only auth)** ⭐
- [ ] **pam.username method (user/pass auth)** ⭐
- [ ] **Per-rule PAM service names** ⭐
- [ ] Auth flow integration
- [ ] Testy auth success/failure
- [ ] **PAM auth tests** ⭐

**Deliverable:** MVP z autentykacją (username/password + PAM)

**Przykładowy kod:**
```rust
// src/auth/pam.rs
use pam::Authenticator;

pub enum PamMethod {
    Address,  // pam.address - tylko IP
    Username, // pam.username - user+pass
}

pub struct PamAuth {
    service_name: String,
    method: PamMethod,
}

impl PamAuth {
    pub async fn authenticate(
        &self,
        client_ip: IpAddr,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<bool> {
        let mut auth = Authenticator::with_password(&self.service_name)?;
        
        match self.method {
            PamMethod::Address => {
                // Tylko IP-based auth (jak pam_rhosts)
                auth.get_handler().set_credentials(
                    "rhostusr",  // default PAM user
                    client_ip.to_string(),
                );
            }
            PamMethod::Username => {
                // Username + password auth
                let user = username.ok_or("Username required")?;
                let pass = password.ok_or("Password required")?;
                auth.get_handler().set_credentials(user, pass);
            }
        }
        
        Ok(auth.authenticate().is_ok())
    }
}
```

#### Dzień 12: Privilege Management + Config (10h)  ⭐ NEW
- [ ] **Root privilege detection** ⭐
- [ ] **Capability dropping (Linux capabilities)** ⭐
- [ ] **User switching (setuid/setgid)** ⭐
- [ ] TOML config file enhancement
- [ ] Config loading & validation
- [ ] tracing setup
- [ ] Structured logging
- [ ] CLI arguments (clap)

**Deliverable:** Bezpieczne zarządzanie uprawnieniami + konfiguracja

**Config example:**
```toml
[server]
bind_address = "0.0.0.0"
bind_port = 1080

# Privilege management (jak Dante)
user_privileged = "root"       # Start as root for PAM
user_unprivileged = "socks"    # Drop to this after auth

[auth]
# Client-level auth (przed SOCKS handshake)
client_method = "none"  # "none" or "pam.address"
client_pam_service = "rustsocks-client"

# SOCKS-level auth (po SOCKS handshake)
socks_method = "pam.username"  # "none", "userpass", "pam.address", "pam.username"
socks_pam_service = "rustsocks"  # default PAM service name

# Opcjonalne: per-rule PAM service names będą w ACL
```

**Milestone 1 Exit Criteria:**
- ✅ SOCKS5 CONNECT działa
- ✅ No-auth, user/pass, i **PAM auth** ⭐
- ✅ **pam.address i pam.username** ⭐
- ✅ **Privilege dropping działa** ⭐
- ✅ Config z pliku
- ✅ Testy jednostkowe >80% coverage
- ✅ Można się połączyć przez curl/proxy client
- ✅ **PAM auth weryfikacja (success/failure)** ⭐

**Przykładowy config:**
```toml
# config/rustsocks.toml

[server]
bind_address = "0.0.0.0"
bind_port = 1080
worker_threads = 4

# ⭐ Privilege Management (PAM support)
user_privileged = "root"        # Required for PAM auth
user_unprivileged = "socks"     # Drop to this after bind
group_unprivileged = "socks"    # Optional

[auth]
# Client-level auth (before SOCKS handshake)
client_method = "none"  # "none" or "pam.address"
client_pam_service = "rustsocks-client"

# SOCKS-level auth (after SOCKS handshake)
# Options: "none", "userpass", "pam.address", "pam.username"
socks_method = "pam.username"
socks_pam_service = "rustsocks"

# PAM configuration
[auth.pam]
default_user = "rhostusr"       # For pam.address method
default_ruser = "rhostusr"
verbose = true
verify_service_files = true     # Check /etc/pam.d/ at startup

# Fallback userpass (if not using PAM)
# users_file = "/etc/rustsocks/users.txt"

# ⭐ ACL Configuration
[acl]
enabled = true
config_file = "/etc/rustsocks/acl.toml"
default_policy = "deny"
watch_config = true

# ⭐ Session Tracking
[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite:///var/lib/rustsocks/sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24

# ⭐ REST API
[api]
enabled = true
bind_address = "127.0.0.1"
bind_port = 8080
auth_token = "your-secret-token-here"
cors_origins = ["http://localhost:3000"]
timeout_secs = 30

[limits]
max_connections = 10000
connection_timeout_secs = 300
max_connections_per_user = 100
bandwidth_limit_mbps = 100

[logging]
level = "info"
format = "json"
session_log = "/var/log/rustsocks/sessions.log"

[metrics]
enabled = true
bind_address = "0.0.0.0"
bind_port = 9090
export_interval_secs = 60
```

**Milestone 1 Exit Criteria:**
- ✅ SOCKS5 CONNECT działa
- ✅ No-auth i user/pass auth
- ✅ Config z pliku
- ✅ Testy jednostkowe >80% coverage
- ✅ Można się połączyć przez curl/proxy client

---

### Sprint 2: Performance, ACL & Session Tracking (Tydzień 3-5) - 120h

#### Dzień 1-3: Connection Management (20h)
- [ ] Connection pool dla upstream
- [ ] Timeout handling
- [ ] Keep-alive management
- [ ] Resource cleanup
- [ ] Memory profiling

**Deliverable:** Efektywne zarządzanie połączeniami

#### Dzień 4-6: ACL Engine - Core (25h)  ⭐ NEW
- [ ] ACL rule data structures (`AclRule`, `Action`, `Matcher`)
- [ ] IP matching (single IP, CIDR ranges)
- [ ] Domain matching (exact, wildcard)
- [ ] Port matching (single, ranges, multiple)
- [ ] Rule evaluation algorithm (BLOCK priority)
- [ ] Unit tests dla matching logic

**Deliverable:** Działający ACL engine (w pamięci)

**Przykładowy kod:**
```rust
// src/acl/rules.rs
#[derive(Debug, Clone)]
pub struct AclRule {
    pub action: Action,
    pub destinations: Vec<DestinationMatcher>,
    pub ports: Vec<PortMatcher>,
    pub protocols: Vec<Protocol>,
    pub priority: u32,
    pub description: String,
}

impl AclRule {
    pub fn matches(&self, dest: &Address, port: u16, proto: Protocol) -> bool {
        self.destinations.iter().any(|m| m.matches(dest))
            && self.ports.iter().any(|m| m.matches(port))
            && self.protocols.contains(&proto)
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Allow,
    Block,
}

#[derive(Debug, Clone)]
pub enum DestinationMatcher {
    Ip(IpAddr),
    Cidr(IpNet),
    Domain(String),      // exact
    DomainWildcard(String), // *.example.com
}

impl DestinationMatcher {
    pub fn matches(&self, addr: &Address) -> bool {
        match (self, addr) {
            (Self::Ip(ip), Address::IPv4(octets)) => {
                ip == &IpAddr::V4(Ipv4Addr::from(*octets))
            }
            (Self::Cidr(net), Address::IPv4(octets)) => {
                let ip = IpAddr::V4(Ipv4Addr::from(*octets));
                net.contains(&ip)
            }
            (Self::Domain(domain), Address::Domain(d)) => domain == d,
            (Self::DomainWildcard(pattern), Address::Domain(d)) => {
                glob_match(pattern, d)
            }
            _ => false,
        }
    }
}
```

#### Dzień 7-9: ACL Config & Hot Reload (25h)  ⭐ NEW
- [ ] TOML config parser dla ACL
- [ ] Per-user i per-group rules loading
- [ ] Rule inheritance (groups -> users)
- [ ] Config validation
- [ ] File watcher z `notify`
- [ ] Hot reload bez disconnecting active sessions
- [ ] Integration testy ACL reload

**Deliverable:** ACL z hot reload działający

#### Dzień 10-12: Session Manager - Core (25h)  ⭐ NEW
- [ ] Session data structure
- [ ] SessionManager z DashMap (concurrent)
- [ ] Session lifecycle (new, update, close)
- [ ] Traffic counting (bytes sent/received)
- [ ] Active sessions tracking
- [ ] Metrics integration

**Deliverable:** Session tracking w pamięci

#### Dzień 13-15: Session History & Database (25h)  ⭐ NEW
- [ ] SQLite schema design
- [ ] Database migrations (sqlx)
- [ ] Session persistence (async writes)
- [ ] Query API (filter by user, date, dest IP)
- [ ] Batch insert optimization
- [ ] Database cleanup task (old sessions)
- [ ] Integration testy z DB

**Deliverable:** Persistent session history

**SQL Migration:**
```sql
-- migrations/001_create_sessions_table.sql
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    user TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT,
    duration_secs INTEGER,
    
    source_ip TEXT NOT NULL,
    source_port INTEGER NOT NULL,
    dest_ip TEXT NOT NULL,
    dest_port INTEGER NOT NULL,
    protocol TEXT NOT NULL,
    
    bytes_sent INTEGER DEFAULT 0,
    bytes_received INTEGER DEFAULT 0,
    packets_sent INTEGER DEFAULT 0,
    packets_received INTEGER DEFAULT 0,
    
    status TEXT NOT NULL,
    close_reason TEXT,
    
    acl_rule_matched TEXT,
    acl_decision TEXT NOT NULL,
    
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_sessions_user ON sessions(user);
CREATE INDEX idx_sessions_start_time ON sessions(start_time);
CREATE INDEX idx_sessions_dest_ip ON sessions(dest_ip);
CREATE INDEX idx_sessions_status ON sessions(status);
```

#### Dzień 16-17: IPv6 & Domain Resolution (10h)
- [ ] IPv6 address parsing
- [ ] Domain name resolution (async DNS)
- [ ] Address type selection
- [ ] Testy wszystkich typów adresów

**Deliverable:** Obsługa IPv4/IPv6/Domain

#### Dzień 18: Integration - ACL + Session (10h)
- [ ] Connection handler integration
- [ ] ACL check przed tworzeniem sesji
- [ ] Reject tracking (ACL denied sessions)
- [ ] End-to-end test flow
- [ ] Performance test ACL overhead

**Deliverable:** Pełna integracja ACL i Session

**Milestone 2 Exit Criteria:**
- ✅ ACL działa (allow/block rules, BLOCK priority)
- ✅ Hot reload ACL bez impactu na aktywne połączenia
- ✅ Session tracking działa (active + history)
- ✅ Database persistence
- ✅ IPv6 + domain resolution
- ✅ Testy ACL coverage >85%
- ✅ Load test: 1000 równoległych z ACL checking <5ms overhead

---

### Sprint 3: Production Readiness & API (Tydzień 6-8) - 120h

#### Dzień 1-2: UDP ASSOCIATE (15h)
- [ ] UDP socket handling
- [ ] UDP relay implementation
- [ ] Packet forwarding
- [ ] UDP timeout management
- [ ] UDP session tracking
- [ ] Testy UDP flow

**Deliverable:** Działające UDP relay

#### Dzień 3-4: BIND Command (15h)
- [ ] BIND implementation
- [ ] Port allocation
- [ ] Incoming connection handling
- [ ] Testy BIND flow

**Deliverable:** Pełna implementacja SOCKS5

#### Dzień 5-7: REST API dla Monitoringu (25h)  ⭐ NEW
- [ ] Axum server setup
- [ ] GET /api/sessions/active endpoint
- [ ] GET /api/sessions/history endpoint (z filtrowaniem)
- [ ] GET /api/sessions/{id} endpoint
- [ ] GET /api/sessions/stats endpoint
- [ ] GET /api/users/{user}/sessions endpoint
- [ ] Authentication dla API (token-based)
- [ ] CORS configuration
- [ ] API documentation (OpenAPI/Swagger)
- [ ] Rate limiting dla API

**Deliverable:** Działające REST API

**Przykładowe endpointy:**
```rust
// src/api/routes.rs
use axum::{Router, routing::get};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/sessions/active", get(handlers::get_active_sessions))
        .route("/api/sessions/history", get(handlers::get_session_history))
        .route("/api/sessions/:id", get(handlers::get_session))
        .route("/api/sessions/stats", get(handlers::get_stats))
        .route("/api/users/:user/sessions", get(handlers::get_user_sessions))
        .route("/metrics", get(handlers::metrics))
        .route("/health", get(handlers::health_check))
        .with_state(state)
        .layer(/* auth, cors, tracing */)
}

// src/api/handlers.rs
pub async fn get_active_sessions(
    State(session_mgr): State<Arc<SessionManager>>,
    Query(params): Query<SessionQuery>,
) -> Result<Json<Vec<SessionInfo>>, ApiError> {
    let sessions = session_mgr.get_active_sessions();
    
    // Apply filters
    let filtered = sessions.into_iter()
        .filter(|s| params.matches(s))
        .collect();
    
    Ok(Json(filtered))
}
```

#### Dzień 8-9: Extended Metrics & Dashboards (15h)  ⭐ NEW
- [ ] Per-user bandwidth metrics
- [ ] Per-destination metrics
- [ ] ACL decision metrics
- [ ] Session duration histograms
- [ ] Connection error tracking
- [ ] Grafana dashboard JSON
- [ ] Alerting rules (Prometheus)

**Deliverable:** Production-ready monitoring

#### Dzień 10-11: Advanced Auth (15h)
- [ ] Auth backend trait
- [ ] File-based user DB
- [ ] Password hashing (argon2)
- [ ] Auth caching
- [ ] Reload users bez restartu

**Deliverable:** Elastyczny system auth

#### Dzień 12-13: Rate Limiting (15h)
- [ ] Token bucket algorithm
- [ ] Per-IP rate limiting
- [ ] Per-user bandwidth limits
- [ ] Per-user connection limits
- [ ] Backpressure handling
- [ ] Metryki rate limiting

**Deliverable:** Rate limiting działający

#### Dzień 14: Hot Reload - Extended (10h)
- [ ] SIGHUP handler dla wszystkich configs
- [ ] ACL reload (już zrobione)
- [ ] Users reload
- [ ] Main config reload
- [ ] Rotacja logów
- [ ] Zero-downtime validation

**Deliverable:** Comprehensive hot reload

#### Dzień 15-16: systemd & Packaging (15h)
- [ ] systemd service file z watchdog
- [ ] Installation script
- [ ] Debian package (.deb)
- [ ] Docker image (multi-stage)
- [ ] Docker Compose example
- [ ] Kubernetes manifests
- [ ] Documentation deployment

**Deliverable:** Pakiety produkcyjne

#### Dzień 17-18: Load Testing & Optimization (15h)
- [ ] Load test suite (wrk, k6)
- [ ] ACL performance profiling
- [ ] Session tracking overhead measurement
- [ ] Database write optimization
- [ ] Memory leak checks (valgrind)
- [ ] Optimization hot paths
- [ ] Benchmark regression tests

**Deliverable:** Zoptymalizowana aplikacja

**Milestone 3 Exit Criteria:**
- ✅ Pełna spec SOCKS5 (CONNECT, BIND, UDP)
- ✅ REST API działa i jest dokumentowane
- ✅ Extended metrics w Prometheus
- ✅ Grafana dashboard gotowy
- ✅ Hot reload wszystkich configs
- ✅ systemd integration + Docker
- ✅ Load test: 5000+ połączeń z ACL + session tracking
- ✅ API response time <100ms (p99)
- ✅ ACL check overhead <5ms
- ✅ Database write throughput >1000 sessions/sec
- ✅ Memory usage <500MB (5k sessions)
- ✅ <50ms latency p99 dla proxy traffic
- ✅ Dokumentacja kompletna (API + Admin)

---

## 🧪 Strategia Testowania

### Unit Tests
```bash
cargo test --lib
```
- Każdy moduł ma własne testy
- Property-based testing (proptest) dla parserów
- Mock network I/O gdzie możliwe
- Target: >80% coverage

### Integration Tests
```bash
cargo test --test '*'
```
- Testy end-to-end flow
- Autentykacja scenarios
- Error handling paths
- Real network operations

### Load Tests
```rust
// tests/load/stress_test.rs
#[tokio::test]
async fn stress_test_1000_concurrent() {
    // Spawn 1000 clients
    // Measure latency, throughput
    // Check for memory leaks
}
```

### Benchmarks
```bash
cargo bench
```
- Connection setup time
- Parser throughput
- Proxy latency
- Memory allocations per request

### Performance Targets

| Metric | Target | Method |
|--------|--------|--------|
| Concurrent connections | 5000+ | Load test |
| Latency (p50) | <10ms | Benchmark |
| Latency (p99) | <50ms | Benchmark |
| **ACL check overhead** ⭐ | **<5ms** | Benchmark |
| **Session tracking overhead** ⭐ | **<2ms** | Benchmark |
| Throughput | 1GB/s | iperf through proxy |
| Memory (idle) | <100MB | /proc/self/status |
| Memory (5k conn) | <500MB | Load test |
| **Memory (5k conn + sessions)** ⭐ | **<800MB** | Load test |
| CPU (idle) | <1% | top/htop |
| **Database write throughput** ⭐ | **>1000 sessions/sec** | Benchmark |
| **API response time (p99)** ⭐ | **<100ms** | Benchmark |
| **Hot reload time** ⭐ | **<100ms** | Integration test |

## 📊 Metryki Sukcesu Projektu

### Milestone 1 (MVP + PAM) - Tydzień 2.5  ⭐ UPDATED
- ✅ Kompiluje się bez warnings
- ✅ clippy nie zgłasza issues
- ✅ Działa z curl/proxychains
- ✅ **PAM authentication działa (address + username)** ⭐
- ✅ **Privilege dropping weryfikowane** ⭐
- ✅ **PAM service files checked** ⭐
- ✅ Testy przechodzą (>80% coverage)
- ✅ README z przykładami użycia

### Milestone 2 (Beta + ACL + Sessions) - Tydzień 5  ⭐ UPDATED
- ✅ 1000 równoległych połączeń stabilnie
- ✅ **ACL działa (allow/block, per-user, hot reload)** ⭐
- ✅ **Session tracking (active + database)** ⭐
- ✅ **Hot reload bez wpływu na aktywne sesje** ⭐
- ✅ **ACL check <5ms overhead** ⭐
- ✅ IPv6 + Domain resolution
- ✅ Metryki Prometheus działają
- ✅ Zero panics w testach stress

### Milestone 3 (Production + API) - Tydzień 8  ⭐ UPDATED
- ✅ 5000+ połączeń
- ✅ **REST API działa** ⭐
- ✅ **Extended monitoring (Grafana dashboards)** ⭐
- ✅ **Database write >1000 sessions/sec** ⭐
- ✅ p99 latency <50ms
- ✅ **ACL + Session tracking overhead łącznie <7ms** ⭐
- ✅ Memory stable (no leaks, <800MB @ 5k conn)
- ✅ systemd service działa
- ✅ Dokumentacja kompletna (Admin + API)
- ✅ Docker image na DockerHub

## 🚀 Development Workflow

### Daily Routine
```bash
# 1. Pull latest
git pull origin main

# 2. Create feature branch
git checkout -b feature/udp-associate

# 3. Development cycle
cargo watch -x "check" -x "test"

# 4. Before commit
cargo fmt
cargo clippy -- -D warnings
cargo test

# 5. Commit
git commit -m "feat: implement UDP ASSOCIATE command"

# 6. Push & PR
git push origin feature/udp-associate
```

### Code Review Checklist
- [ ] Kompiluje się bez warnings
- [ ] Testy przechodzą
- [ ] clippy happy
- [ ] Dokumentacja zaktualizowana
- [ ] CHANGELOG.md entry
- [ ] Performance nie pogorszone

## 📚 Dokumentacja

### README.md
- Quick start (instalacja, basic usage)
- Features lista
- Configuration overview
- Contributing guidelines

### docs/architecture.md
- System design
- Component diagrams
- Data flow
- Concurrency model

### docs/configuration.md
- Pełna referencja config options
- Przykłady konfiguracji
- Best practices

### docs/deployment.md
- systemd setup
- Docker deployment
- Kubernetes example
- Performance tuning

### docs/api.md (jeśli applicable)
- Metrics endpoint
- Health check
- Admin API (future)

## 🔒 Security Considerations

### Development Phase
- [ ] Input validation (wszystkie parsery)
- [ ] Password hashing (argon2, never plaintext)
- [ ] Resource limits (prevent DoS)
- [ ] Rate limiting per-IP
- [ ] Audit logging (auth failures)

### Pre-Production Audit
- [ ] Dependency audit (`cargo audit`)
- [ ] Memory safety (Rust daje dużo za darmo)
- [ ] Fuzzing critical parsers
- [ ] Penetration testing
- [ ] Security.md w repo

## 📦 Release Process

### Version 0.1.0 (MVP)
```bash
git tag v0.1.0
cargo build --release
# Binary w target/release/rustsocks
```

### Version 0.5.0 (Beta)
- GitHub Release
- Binaries dla Linux (x64, ARM64)
- Docker image
- AUR package (Arch)

### Version 1.0.0 (Production)
- Full release notes
- Migration guide
- .deb i .rpm packages
- Homebrew formula
- Official documentation site

## 🎯 Success Criteria - Final Checklist

### Functional
- [x] SOCKS5 full spec implemented
- [x] Auth (none + userpass + **PAM.address + PAM.username**) ⭐
- [x] **Privilege dropping (root → unprivileged)** ⭐
- [x] IPv4, IPv6, Domain
- [x] TCP (CONNECT, BIND) + UDP
- [x] Config file
- [x] **ACL system (allow/block, per-user, hot reload)** ⭐
- [x] **Per-rule PAM service names** ⭐
- [x] **Session tracking (active + history)** ⭐
- [x] **REST API dla monitoringu** ⭐

### Non-Functional
- [x] 5000+ concurrent connections
- [x] <50ms latency p99 (proxy traffic)
- [x] **<5ms ACL check overhead** ⭐
- [x] **<2ms session tracking overhead** ⭐
- [x] <800MB memory (5k sessions)
- [x] **>1000 sessions/sec database writes** ⭐
- [x] **<100ms API response time (p99)** ⭐
- [x] **Hot reload <100ms** ⭐
- [x] Zero-downtime reload
- [x] Graceful shutdown
- [x] Extended Prometheus metrics

### Quality
- [x] >80% test coverage
- [x] **ACL tests >85% coverage** ⭐
- [x] Benchmarks passing
- [x] Zero clippy warnings
- [x] Documentation complete
- [x] **API documentation (OpenAPI)** ⭐
- [x] Security audit done

### Operations
- [x] systemd service
- [x] Docker image
- [x] Installation packages
- [x] **Grafana dashboards** ⭐
- [x] **Alerting rules** ⭐
- [x] Monitoring runbooks

## 🔄 Post-1.0 Roadmap

### v1.1 - Observability
- OpenTelemetry support
- Distributed tracing
- Better dashboards

### v1.2 - Advanced Features
- SOCKS over TLS
- Traffic shaping
- Geo-blocking

### v1.3 - Management
- Web dashboard
- REST API dla zarządzania
- User self-service portal

### v2.0 - Clustering
- Multi-node setup
- Session persistence
- Load balancing

## 💡 Tips & Best Practices

### Rust-Specific
1. **Use `cargo-watch` dla szybszego dev loop:**
   ```bash
   cargo install cargo-watch
   cargo watch -x check -x test
   ```

2. **Profile memory early:**
   ```bash
   cargo install cargo-instruments
   cargo instruments -t Allocations
   ```

3. **Catch bugs z clippy:**
   ```bash
   cargo clippy -- -W clippy::all -W clippy::pedantic
   ```

4. **Document publiczne API:**
   ```bash
   cargo doc --open
   ```

### Performance
- Use `bytes::Bytes` dla zero-copy
- Pool bufory (avoid allocations)
- Batch syscalls gdzie możliwe
- Monitor z `tokio-console`

### Testing
- Integration testy w osobnym crate'ie
- Use `#[tokio::test]` dla async testów
- Mock network z `tokio-test`
- Load test w CI (mniejsza skala)

## 📞 Support & Community

### Przed Release
- GitHub Issues dla bug tracking
- Discussions dla Q&A
- Discord/Slack dla dev chat

### Po Release
- Documentation site (mdbook?)
- Example configs repo
- Community tutorials
- Blog posts o architekturze

---

## 🔍 Przykłady Użycia - API i Monitoring

### REST API Examples

#### 1. Sprawdź aktywne sesje
```bash
curl -H "Authorization: Bearer your-secret-token-here" \
  http://localhost:8080/api/sessions/active

# Response:
[
  {
    "session_id": "550e8400-e29b-41d4-a716-446655440000",
    "user": "alice",
    "start_time": "2025-10-21T10:30:00Z",
    "duration_secs": 1234,
    "source_ip": "192.168.1.100",
    "source_port": 54321,
    "dest_ip": "93.184.216.34",
    "dest_port": 443,
    "protocol": "tcp",
    "bytes_sent": 102400,
    "bytes_received": 2048000,
    "status": "active"
  }
]
```

#### 2. Historia sesji z filtrowaniem
```bash
# Sesje użytkownika alice z ostatnich 24h
curl -H "Authorization: Bearer your-token" \
  "http://localhost:8080/api/sessions/history?user=alice&hours=24"

# Sesje do konkretnego IP
curl -H "Authorization: Bearer your-token" \
  "http://localhost:8080/api/sessions/history?dest_ip=93.184.216.34"

# Sesje odrzucone przez ACL
curl -H "Authorization: Bearer your-token" \
  "http://localhost:8080/api/sessions/history?status=rejected"
```

#### 3. Statystyki agregowane
```bash
curl -H "Authorization: Bearer your-token" \
  http://localhost:8080/api/sessions/stats

# Response:
{
  "active_sessions": 1234,
  "total_sessions": 5678,
  "total_bytes": 1099511627776,
  "total_bytes_sent": 549755813888,
  "total_bytes_received": 549755813888,
  "top_users": [
    {"user": "alice", "sessions": 234, "bytes": 10737418240},
    {"user": "bob", "sessions": 123, "bytes": 5368709120}
  ],
  "top_destinations": [
    {"ip": "93.184.216.34", "connections": 456},
    {"ip": "192.168.1.50", "connections": 234}
  ],
  "acl_stats": {
    "allowed": 4500,
    "blocked": 178
  }
}
```

#### 4. Szczegóły konkretnej sesji
```bash
curl -H "Authorization: Bearer your-token" \
  http://localhost:8080/api/sessions/550e8400-e29b-41d4-a716-446655440000
```

### Prometheus Queries

#### Top 10 użytkowników po aktywnych sesjach
```promql
topk(10, rustsocks_user_active_sessions)
```

#### Bandwidth per user (MB/s)
```promql
rate(rustsocks_user_bytes_total[5m]) / 1024 / 1024
```

#### ACL rejection rate
```promql
rate(rustsocks_acl_decisions_total{decision="block"}[5m]) / 
rate(rustsocks_acl_decisions_total[5m]) * 100
```

#### Average session duration
```promql
rate(rustsocks_session_duration_seconds_sum[5m]) / 
rate(rustsocks_session_duration_seconds_count[5m])
```

#### Top destinations by connection count
```promql
topk(10, rate(rustsocks_destination_connections_total[5m]))
```

### Grafana Dashboard Queries

**Panel 1: Active Sessions Over Time**
```promql
rustsocks_active_sessions
```

**Panel 2: Session Creation Rate**
```promql
rate(rustsocks_total_sessions[5m])
```

**Panel 3: Bandwidth (Upload/Download)**
```promql
rate(rustsocks_bytes_sent_total[1m]) / 1024 / 1024
rate(rustsocks_bytes_received_total[1m]) / 1024 / 1024
```

**Panel 4: ACL Allow vs Block**
```promql
sum by (decision) (rate(rustsocks_acl_decisions_total[5m]))
```

**Panel 5: Top Users Table**
```promql
sort_desc(sum by (user) (rustsocks_user_active_sessions))
```

**Panel 6: Session Duration Heatmap**
```promql
rate(rustsocks_session_duration_seconds_bucket[5m])
```

### Alerting Rules Example

```yaml
# prometheus-alerts.yml
groups:
  - name: rustsocks
    interval: 30s
    rules:
      - alert: HighACLRejectionRate
        expr: |
          rate(rustsocks_acl_decisions_total{decision="block"}[5m]) / 
          rate(rustsocks_acl_decisions_total[5m]) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High ACL rejection rate ({{ $value | humanizePercentage }})"
          description: "More than 10% of connections are being blocked by ACL"
      
      - alert: HighConnectionCount
        expr: rustsocks_active_sessions > 4000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High number of active sessions ({{ $value }})"
          description: "Approaching connection limit"
      
      - alert: DatabaseWriteSlow
        expr: |
          rate(rustsocks_session_database_writes_total[1m]) < 100
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Session database writes are slow"
          description: "Database write rate is below 100/sec"
      
      - alert: HighMemoryUsage
        expr: |
          process_resident_memory_bytes{job="rustsocks"} > 800 * 1024 * 1024
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "RustSocks high memory usage ({{ $value | humanize }}B)"
          description: "Memory usage exceeds 800MB"
```

### ACL Testing - Command Line

#### Test ACL rules przed deploymentem
```bash
# Dry-run ACL check
rustsocks acl-check \
  --config /etc/rustsocks/acl.toml \
  --user alice \
  --dest 192.168.1.100 \
  --port 443

# Output:
# ✅ ALLOW: Rule matched - "Allow access to company network"
# Rule details: destinations=[10.0.0.0/8], ports=[443]
```

#### Validate ACL config
```bash
rustsocks acl-validate --config /etc/rustsocks/acl.toml

# Output:
# ✅ Config valid
# Users: 5
# Groups: 2
# Total rules: 23
# Conflicting rules: 0
```

#### Hot reload ACL
```bash
# Send SIGHUP dla reload
sudo systemctl reload rustsocks

# Lub przez API
curl -X POST -H "Authorization: Bearer your-token" \
  http://localhost:8080/api/admin/reload-acl
```

### Monitoring Setup - Quick Start

#### 1. Start RustSocks
```bash
rustsocks --config /etc/rustsocks/rustsocks.toml
```

#### 2. Start Prometheus
```bash
# prometheus.yml
scrape_configs:
  - job_name: 'rustsocks'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s

prometheus --config.file=prometheus.yml
```

#### 3. Import Grafana Dashboard
```bash
# Import dashboard JSON (będzie w repo)
curl -X POST http://localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  -d @grafana-dashboard.json
```

#### 4. Query sessions via CLI
```bash
# Simple CLI tool dla admin
rustsocks-cli sessions list --active
rustsocks-cli sessions list --user alice --last 24h
rustsocks-cli sessions stats
rustsocks-cli users top --by-sessions
```

---

## 🎬 Getting Started - First Steps

### Dzień 1 - Setup
```bash
# 1. Create project
cargo new rustsocks --bin
cd rustsocks

# 2. Add dependencies
# (copy Cargo.toml z tego dokumentu)

# 3. Setup git
git init
git add .
git commit -m "Initial commit"

# 4. Create structure
mkdir -p src/{protocol,auth,server,config,metrics,acl,utils}
touch src/protocol/mod.rs
# ... (wszystkie moduły)

# 5. First compile
cargo check
```

### Dzień 1 - First Code
```rust
// src/main.rs
use tokio::net::TcpListener;
use tracing::{info, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    let listener = TcpListener::bind("127.0.0.1:1080").await?;
    info!("RustSocks listening on 127.0.0.1:1080");
    
    loop {
        let (socket, addr) = listener.accept().await?;
        info!("New connection from {}", addr);
        
        tokio::spawn(async move {
            if let Err(e) = handle_client(socket).await {
                error!("Error handling client: {}", e);
            }
        });
    }
}

async fn handle_client(socket: TcpStream) -> anyhow::Result<()> {
    // TODO: implement SOCKS5 protocol
    Ok(())
}
```

**Następny krok:** Implementacja parsera handshake'u!

---

## ✅ Daily Checklist Template

```markdown
### Day X - [Feature Name]

**Goal:** [Co chcę osiągnąć dziś]

**Tasks:**
- [ ] Task 1
- [ ] Task 2
- [ ] Write tests
- [ ] Update docs

**Blockers:** [Jeśli jakieś]

**Learnings:** [Co nowego się nauczyłem]

**Tomorrow:** [Plan na następny dzień]
```

---

**Created:** 2025-10-21  
**Version:** 1.0  
**Author:** AI-assisted development plan  
**License:** MIT

**Powodzenia! 🚀 Masz pytania o konkretny element planu - pytaj!**
