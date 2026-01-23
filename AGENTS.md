# Repository Guidelines

## Project Structure & Module Organization
- `src/` contains the Rust implementation (core modules: `protocol/`, `auth/`, `acl/`, `session/`, `server/`, `config/`, `api/`).
- `tests/` holds integration and system tests (e.g., `udp_associate.rs`, `pool_*`, `acl_*`).
- `docs/` contains technical and user guides; `config/` and `docs/examples/` include example TOML configs.
- `dashboard/` contains the web dashboard assets; `migrations/` has database migrations.

## Build, Test, and Development Commands
- `cargo build` — compile debug build.
- `cargo build --release` — optimized release build.
- `cargo test --all-features` — run full test suite with optional features enabled.
- `cargo test <name>` — run a specific test (e.g., `cargo test udp_packets_respect_acl_rules`).
- `cargo clippy --all-features -- -D warnings` — lint; warnings are treated as errors.
- `cargo fmt --check` — verify formatting; use `cargo fmt` to apply.
- `./target/release/rustsocks --config config/rustsocks.toml` — run server with config.
- `./target/release/rustsocks --generate-config config/rustsocks.toml` — generate example config.

## Coding Style & Naming Conventions
- Rust style follows `rustfmt` defaults (4-space indentation).
- Prefer idiomatic Rust APIs (e.g., `io::Error::other`).
- Keep function parameter lists small; use context structs when needed.
- File and module names are lowercase with underscores (e.g., `session_manager_edge_cases.rs`).

## Testing Guidelines
- Frameworks: `cargo test` with unit tests and integration tests.
- Integration tests live in `tests/` and should be named by behavior or component (`acl_*`, `pool_*`).
- Add tests for new behavior and edge cases; ensure they pass with `--all-features`.

## Commit & Pull Request Guidelines
- No strict commit convention observed; use clear, imperative messages (e.g., “Fix QoS timing tests”).
- PRs should include: summary, rationale, test commands run, and any config/doc updates.
- If changing config schema, update example TOML files and relevant docs under `docs/`.

## Security & Configuration Tips
- Avoid committing secrets; use `sessions.api_token` and dashboard auth in config files.
- When enabling database-backed sessions, set `sessions.database_url` and apply migrations.
