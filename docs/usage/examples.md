# Common Usage Scenarios

This page collects practical configuration snippets and command examples.

## 1) Local Development Proxy

```toml
[server]
bind_address = "127.0.0.1"
bind_port = 1080

[auth]
client_method = "none"
socks_method = "none"
```

Run:

```bash
cargo build --release
./target/release/rustsocks --config config/rustsocks.toml
```

## 2) Username/Password Authentication

```toml
[auth]
socks_method = "userpass"

[[auth.users]]
username = "alice"
password = "secret123"
```

Test:

```bash
curl -x socks5://alice:secret123@127.0.0.1:1080 http://example.com
```

## 3) Enable ACLs

```toml
[acl]
enabled = true
config_file = "config/acl.toml"
watch = true
```

Example `acl.toml` rule:

```toml
[[users]]
username = "alice"

[[users.rules]]
action = "allow"
destinations = ["*.dev.company.com"]
ports = ["443"]
protocols = ["tcp"]
priority = 200
```

## 4) Enable Sessions + Dashboard

```toml
[sessions]
enabled = true
storage = "sqlite"
database_url = "sqlite://sessions.db"
stats_api_enabled = true
dashboard_enabled = true
stats_api_port = 9090
```

**Note**: Database-backed sessions (`sqlite`, `mariadb`, `mysql`) require the `database` feature at build time.

## 5) Add Dashboard Authentication

```toml
[sessions.dashboard_auth]
enabled = true

[[sessions.dashboard_auth.users]]
username = "admin"
password = "strong-secret"
```

## 6) QoS Rate Limiting

```toml
[qos]
enabled = true
algorithm = "htb"

[qos.htb]
global_bandwidth_bytes_per_sec = 125000000
max_bandwidth_bytes_per_sec = 12500000
```

## 7) TLS for Client Connections

```toml
[server.tls]
enabled = true
certificate_path = "config/server.crt"
private_key_path = "config/server.key"
require_client_auth = false
```

## 8) Base Path for Reverse Proxy

```toml
[sessions]
base_path = "/rustsocks"
```

Dashboard and API are now under `/rustsocks/`.

## 9) Prometheus Metrics

```toml
[metrics]
enabled = true
storage = "sqlite"
retention_hours = 24
```

Metrics endpoint (requires the stats API server): `http://127.0.0.1:9090/metrics`
