#!/usr/bin/env bash
# Full verification suite for aten-ia.
#
# Designed to run locally and in CI without a GGUF model or external network:
#   1. locked dependency metadata
#   2. formatting
#   3. clippy (library)
#   4. all unit + integration tests (single-threaded)
#   5. CLI smoke tests (--version, --self-test)
#   6. documentation checks (links, secrets, man page)
#
# Environment:
#   VERIFY_RELEASE=1   also build the release binary (slower)
#   LLAMA_LOCAL_LIBS   reuse prebuilt llama.cpp static libraries
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT/memvid-agent-core"

step() { printf '\n==> %s\n' "$*"; }

step "cargo metadata --locked"
cargo metadata --locked --format-version 1 >/dev/null

step "cargo fmt --check"
cargo fmt --all -- --check

step "cargo clippy --lib"
cargo clippy --locked --lib

step "cargo test"
cargo test --locked -- --test-threads=1

step "CLI: --version"
cargo run --locked --quiet -- --version

step "CLI: --self-test"
cargo run --locked --quiet -- --self-test

if [[ "${VERIFY_RELEASE:-0}" == "1" ]]; then
    step "cargo build --release"
    cargo build --release --locked
    step "release binary: --self-test"
    ./target/release/aten-ia --self-test
fi

step "documentation checks"
"$ROOT/scripts/ci/check-docs.sh"

printf '\nverify: OK\n'
