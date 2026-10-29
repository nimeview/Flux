# Flux program fixtures

Integration tests under `flux_cli/tests/` compile and run files from this tree.

## Layout

| Directory | Purpose |
|-----------|---------|
| `ok/` | Programs that must compile (and usually run) |
| `fail/` | Programs that must be rejected with diagnostics |
| `demo/`, `apps/` | Larger examples (separate test suites) |

## Header comments

**Expectations** (documentation for humans and some targeted tests):

```flux
// expect: a=5, b=2
// expect: compile ok — short description
// expect: fail E003 — assign to derived inside watch
```

**Test mode** (controls how the regression suite treats the file):

```flux
// test: run          — default: full compile + executor (implicit)
// test: check-only   — typecheck only; no IR/executor (stdin, blocking init I/O)
// test: stub         — valid module used only as an import; skip fail-suite roots
```

Place `// test:` directives in the first few lines of the file (before code).

## Running tests

```bash
cargo test --workspace --no-fail-fast
```

Use `--no-fail-fast` locally so one failing test does not hide others.

## I/O programs

- `ok/io/read_line.fl` — `// test: check-only` in regression; runtime behavior is tested via `integration.rs` with a stdin pipe.
- Other `ok/io/*` — file I/O in `watch` at module init; safe for executor tests.
