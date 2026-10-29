# Changelog

All notable changes to Flux are documented here.

## [Unreleased] — v0.4

### Added
- **JIT Phase 1–4**: tag-aware int/float slots, `JitStats` + `flux run --jit --verbose`, `If`/`While`/`For`/`emit` in JIT handlers, static `LazyIf` dep tracking, per-thread JIT compile cache, `flux repl --jit`, `flux.toml [run] jit`, `FLUX_JIT=1`
- **JIT strings + enums**: string const/concat/equality (tag 5), user enum match with payload (tag 6); dedicated CI job on macOS + Linux (`cargo test -p flux_jit`, `cargo test -p flux_cli --test jit`)
- **`flux_value` unification**: `TaggedVal`, tag constants, heap stores, and `value_to_tagged` / `tagged_to_value` live in `flux_value`; JIT host stores use `Mutex` (thread-safe across workers)
- **Syntax v2**: unified `let` / `let mut` / `const` / `derived` module bindings (replaces bare `x = 0` and `set`)
- Stdlib I/O: `read_line`, `read_file`, `write_file`, `append_file`, `file_exists`
- Error **E016** (`let` without `mut` reassigned) and **E017** (side effects in derived)
- Dependency-aware compile cache (invalidates when any imported `.fl` changes)
- LSP snippets for `let mut` and `derived`

### Changed (breaking)
- Removed top-level `binding` (`x = 0`) and `set` statement syntax
- Module reassignment only via `let mut` sources in `on`/`watch`
- All bundled programs migrated to v2 syntax

### Fixed
- Compile cache stale hits when dependency modules change
- Full file compilation now regenerates or reuses IR after an analysis-cache hit
- Standard-library imports no longer hide local modules with the same name
- JIT Cranelift `bitcast` memflags (float/int slots compile correctly); `/` matches VM float promotion

---

## [0.3] — tooling + higher-order

### Added
- Higher-order functions: `map`, `filter`, `reduce` with closure arguments
- Trait method dispatch (`expr.method()`) verified end-to-end
- `flux run` reads `flux.toml` entry point when invoked without a file argument
- `flux check` reads `flux.toml` entry point when invoked without a file argument
- `flux lsp-server` — stdio LSP with diagnostics, hover, and basic completion
- `flux_driver` — unified compile pipeline for CLI, LSP, and REPL
- Compile cache read + validate (IR metadata keyed by source hash)
- `TypeCheckContext` — explicit per-pass type registries (no `thread_local`)
- GitHub Actions CI (`cargo test`, clippy, rustfmt)

### Fixed
- `map` was a pass-through stub — now applies fn to each element
- `filter` now accepts a predicate closure instead of truthy filter
- `reduce` now accepts an accumulator function instead of int-only fold
- Closure parameters hardcoded to `Int` — now inferred from call context
- Compile cache deadlock when validating cached IR

### Planned for v1.0 (not yet implemented)
- Match guards, destructuring `let`, string interpolation
- `const` module bindings, `type` aliases
- Qualified module names (`import math as m`)
- `flux_value` unified runtime value type
- LSP go-to-definition, member completion after `.`
- Package path dependencies in `flux.toml`

---

## [0.2.0] — JIT + stdlib + tooling

### Added
- Cranelift JIT backend (`flux run --jit`) for numeric hot paths
- Standard library: `print`, `println`, `len`, `range`, `push`, `pop`, `map` (stub), `filter`, `reduce`, `str_len`, `str_split`, `str_contains`, `str_trim`, `Some`, `None`, `Ok`, `Err`
- Source formatter (`flux fmt`)
- LSP JSON diagnostics API (`flux lsp`)
- `flux init` — creates `flux.toml` + `main.fl`
- Compile cache (SHA256 → `<project>/.flux/cache/*.ir`)
- Struct and enum literals with field access and pattern matching
- Generics (`fn id[T](x: T) -> T`) — monomorphized at compile time
- Traits and impl blocks (`trait Display`, `impl Display for Int`)
- Closures (`|x| x + 1`)
- Multi-file module imports (`import path { sym }`)
- WASM backend stub (`flux wasm`)

---

## [0.1.0] — Language core + VM

### Added
- Reactive binding model: source bindings, derived bindings, dependency graph
- `watch` blocks with dependency filtering
- `on Event` / `emit Event` — event-driven handlers
- `rfn` reactive functions with static dependency tracking
- `snap()` in pure `fn` contexts
- Pure functions `fn`, return type inference
- Arithmetic, comparison, boolean operators, short-circuit `&&`/`||`
- `if/else` (expression and statement), `while`, `for x in expr`
- `return`, `break`, `continue`
- `let`/`set` local bindings
- Array literals and `for i in [arr]` iteration
- Module-level `if` for conditional initialization
- String concatenation with `+`
- Stack VM executor
- REPL with `:type`, `:types`, `:help`, `:clear`, `:show`
- `flux run` and `flux check` CLI commands
- Error codes E000–E009 with miette diagnostics
- ~20 integration test programs (`programs/ok/`, `programs/fail/`)
