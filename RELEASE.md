# Flux Core release gate

A release candidate is a commit on which every item below holds. Each item is
checked by CI (`.github/workflows/ci.yml`) from a clean checkout with `--locked`.

## Supported platforms (MVP)

| Platform | Runner | Status |
|----------|--------|--------|
| Linux x86_64 | `ubuntu-latest` | Supported, gated |
| macOS (Apple silicon) | `macos-15` | Supported, gated |
| Windows | — | Not tested; not part of the MVP |

## Required (blocking)

| Check | CI job | Command |
|-------|--------|---------|
| Formatting | `fmt` | `cargo fmt --all -- --check` |
| Lint, incl. tests and benches | `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Full test suite | `test` | `cargo test --workspace --all-targets --locked` |
| JIT ↔ VM parity | `jit` | `cargo test -p flux_jit` and `-p flux_cli --test jit --test numeric_contract` (single-threaded) |
| Release build | `release` | `cargo build --workspace --release --locked` |
| Quick-start on the release binary | `release` | `cargo test -p flux_cli --release --test quickstart`, plus `flux init` / `check` / `run` |

## Visible, not yet blocking

- **Security audit** (`audit`): `cargo audit` runs on every push. The result is written to the run summary, and a failure raises a warning annotation. Policy (decided 2026-10-31): a failure **warns and does not block** a release. Review each warning before tagging a release.
- **Coverage** (`coverage`): `cargo llvm-cov` publishes a baseline summary and an `lcov` artifact. No threshold exists until the baseline is reviewed. The compiler, resolver, runtime, and CLI error paths should be raised first.

## What "stable MVP" covers

Stable: parsing and module resolution with per-file diagnostics; the type checker; the VM, which is the default and reference engine; reactive sources, derived values, events, and `watch`/`on`; the numeric contract (SPEC §2); modules with per-module namespaces and local path dependencies pinned by `flux.lock`; `flux init [--lib]`, `check`, `run`, `test`, `fmt`, `repl`, and `lsp-server`.

Experimental, not covered by stability:
- `--jit` (Cranelift): opt-in. It must match the VM or fall back to it.
- `flux wasm`: a stub that always fails; no WASM code generation exists.
- `flux lsp` (one-shot JSON): deprecated in favor of `lsp-server`.

Deferred and not claimed:
- a package registry, version dependencies, and remote fetching. They come only after their security, integrity, and offline behavior are specified; a version dependency is an error today;
- non-`String` interpolation (`"{n}"`); use `to_string(n)`;
- module-scoped type names (`struct`/`enum`/`trait` names are program-global).

## Versioning and compatibility

- **One version for the toolchain.** The language, standard library, compiler, and CLI share the workspace version (`Cargo.toml`, currently `0.1.0`). `flux repl` shows it.
- **Before 1.0**, a minor release (`0.x` → `0.(x+1)`) may change syntax, semantics, the standard library, or CLI behavior. A patch release (`0.x.y`) only fixes defects. A fix can still reject a program that relied on the defect (for example, keywords now end at a word boundary). Every user-visible change, breaking ones included, is listed in `CHANGELOG.md`.
- **1.0** freezes the stable surface listed above. After that, breaking changes require a major version.
- **Package `version`** in `flux.toml` is informational. Dependencies are resolved by path, not by version.
- **`flux.lock`** carries a format `version` (currently `1`). A lock in any other format or content counts as stale: it is rewritten, or rejected under `--locked`. Checksums hash LF-normalized text, so CRLF checkouts match.
- **Not compatibility surfaces:** the `.flux/cache` IR cache (keyed by a compiler fingerprint, so any rebuilt compiler starts fresh), the IR, JIT internals, and the diagnostic message wording. Diagnostic codes (`E000`–`E022`) are stable within a minor release.

