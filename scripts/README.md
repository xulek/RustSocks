# Scripts Directory

This directory contains utility scripts for development and CI/CD.

## Available Scripts

### `ci-local.sh`

Local CI verification script that runs all checks performed by GitHub Actions.

**Usage:**
```bash
./scripts/ci-local.sh
```

**Checks performed:**
1. ✅ Code formatting (`cargo fmt --all -- --check`)
2. ✅ Clippy lints (`cargo clippy --all-features -- -D warnings`)
3. ✅ Build verification (`cargo build --locked --all-targets --features database`)
4. ✅ Unit tests (`cargo test --locked --all-targets --features database -- --skip performance`)
5. ✅ Security audit (`cargo audit`)

**Known allowed vulnerabilities:**
- `RUSTSEC-2023-0071` (rsa) - Marvin Attack
- `RUSTSEC-2025-0040` (users) - PAM dependency, no alternative available
- `RUSTSEC-2024-0370` (proc-macro-error) - Compile-time only, via utoipa
- `RUSTSEC-2023-0040` (users) - Unmaintained, PAM dependency
- `RUSTSEC-2023-0059` (users) - Unsound, PAM dependency

**Exit codes:**
- `0` - All checks passed
- `1` - At least one check failed

Run this script before pushing code to ensure CI will pass.
