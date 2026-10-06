# Development scripts

The local CI scripts run the same core validation used by the repository CI and fail on the first build/lint command that is not clean. Test commands use Cargo's `--no-fail-fast` so all failing integration-test binaries are reported in a single run.

## Windows PowerShell

```powershell
.\scripts\ci-local.ps1
```

## Linux/macOS

```bash
./scripts/ci-local.sh
```

## Validation matrix

The scripts run:

1. `cargo fmt --all -- --check`
2. `cargo check --locked`
3. `cargo check --locked --all-features`
4. `cargo check --locked --no-default-features`
5. `cargo clippy --locked --all-features -- -D warnings`
6. `cargo test --locked --no-fail-fast`
7. `cargo test --locked --all-features --no-fail-fast`

`--no-fail-fast` is intentional. Without it, Cargo can stop after the first failing integration-test binary and hide independent failures in later test targets.
