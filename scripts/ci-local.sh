#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

if [[ ! -f Cargo.toml ]]; then
    echo "Cargo.toml not found in repository root: $REPO_ROOT" >&2
    exit 1
fi

printf 'Repository: %s\n' "$REPO_ROOT"
printf 'Cargo: %s\n' "$(cargo --version)"
printf 'Rustc: %s\n' "$(rustc --version)"

if [[ "${1:-}" == "--clean" ]]; then
    printf '\n==> Cleaning Cargo build artifacts\n'
    cargo clean
fi

run() {
    printf '\n==> %s\n' "$*"
    "$@"
}

run cargo fmt --all -- --check

run cargo check --locked
run cargo check --locked --all-features
run cargo check --locked --no-default-features

run cargo clippy --locked -- -D warnings
run cargo clippy --locked --all-features -- -D warnings
run cargo clippy --locked --no-default-features -- -D warnings

run cargo test --locked --no-fail-fast
run cargo test --locked --all-features --no-fail-fast
run cargo test --locked --no-default-features --no-fail-fast

echo
echo "All Rust CI checks passed."
