#!/usr/bin/env bash
# The local gate, in one command.
#
#   scripts/gate.sh          format, Clippy, and every workspace test (~2 min)
#   scripts/gate.sh full     also 1500 generated VM/JIT programs on a release
#                            build and the VS Code extension tests (~3 min)
#
# CI repeats the default gate on Linux and macOS, plus the release build.
# If the tests become slow again, check `ls target/debug/deps | wc -l`: debug
# info in many `.o` files there made every test binary take ~20 s to start
# (see `[profile.dev]` in Cargo.toml).
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$1"; }
started=$(date +%s)

step "format"
cargo fmt --all -- --check

step "clippy"
cargo clippy -q --workspace --all-targets --locked -- -D warnings

step "tests"
cargo test -q --workspace --all-targets --locked

if [[ "${1:-}" == "full" ]]; then
    step "1500 generated programs, VM against JIT (release)"
    FLUX_DIFF_SEEDS=1500 cargo test -q --release -p flux_cli --test differential \
        jit_matches_vm_on_generated_reactive_programs

    step "VS Code extension"
    (cd editors/vscode && npm test)
fi

printf '\ngate passed in %ss\n' "$(( $(date +%s) - started ))"
