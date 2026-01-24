# Quick Start

This guide gets a basic RustSocks server running from source.

## Prerequisites

- Rust toolchain (stable)
- Node.js 18+ (only required if you want the web dashboard)

## Build and Run

```bash
cargo build --release
./target/release/rustsocks --generate-config config/rustsocks.toml
```

Edit the generated config (see [Configuration Reference](../config/rustsocks.md)), then run:

```bash
./target/release/rustsocks --config config/rustsocks.toml
```

The SOCKS5 server will bind to the configured address and port (default: `127.0.0.1:1080`).

## Enable the Dashboard (Optional)

1. Build the dashboard assets:

```bash
cd dashboard
npm install
npm run build
```

2. Enable the API + dashboard in `config/rustsocks.toml`:

```toml
[sessions]
stats_api_enabled = true
dashboard_enabled = true
swagger_enabled = true
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
base_path = "/"
```

3. Restart the server. Then open:

- Dashboard: `http://127.0.0.1:9090/`
- Swagger UI: `http://127.0.0.1:9090/swagger-ui/`

## CLI Options

RustSocks supports basic CLI overrides:

```bash
rustsocks --config config/rustsocks.toml
rustsocks --bind 0.0.0.0 --port 1080
rustsocks --generate-config config/rustsocks.toml
rustsocks --log-level debug
```

For advanced usage and system tuning, see the [Guides](../guides/web-dashboard.md).
