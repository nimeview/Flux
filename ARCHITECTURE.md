# Flux — Architecture (v0.6)

## Compiler pipeline

```
.fl  →  flux_syntax  →  flux_resolve  →  flux_hir  →  flux_types  →  flux_ir  →  flux_runtime
              │                │               │             │              │              │
           pest AST        import graph     scopes/HIR    mono+check     bytecode     graph + VM
              │                │               │             │              │              │
              └──────────────── flux_diagnostics (Span, miette E000–E017) ─────────────────┘
                                                                                       │
                                                                               flux_jit (--jit)
```

**`flux_driver`** is the single compile orchestrator used by CLI, LSP, REPL and tests:
parse → HIR → monomorphize → typecheck → IR → executor, with optional IR cache validation (`<project>/.flux/cache/`).

**IR cache contract.** The key is a SHA-256 over every resolved module's path and contents, so editing a dependency changes it. The file name is also scoped by `COMPILER_FINGERPRINT`, a hash `flux_driver/build.rs` computes over the sources of every crate that shapes the IR, so a rebuilt compiler never reads another build's entries. A blob holds magic, layout version, the fingerprint, a SHA-256 of the payload, and the payload. A wrong magic, version, fingerprint, or checksum, a truncated blob, a decode failure, or decoded IR that fails `flux_runtime::validate_program` all count as a miss: the entry is recompiled from source and overwritten. Write failures are ignored, because the freshly compiled IR is used either way. `--no-cache` neither reads nor writes entries. `flux_cli/tests/cache_recovery.rs` covers each case.

**`flux_cli`** wraps `flux_driver` for user-facing commands.

---

## Crate map

| Crate | Role | Key files |
|-------|------|-----------|
| `flux-syntax` | Pest parser + AST | `flux.pest`, `parser.rs`, `ast.rs` |
| `flux_hir` | HIR lowering, scopes, symbols | `lower.rs`, `nodes.rs`, `symbol.rs` |
| `flux_types` | Type inference, monomorphization, checking | `check.rs`, `mono.rs`, `types.rs`, `builtins.rs` |
| `flux_ir` | Stack-machine bytecode | `lower.rs`, `lib.rs` |
| `flux-runtime` | Reactive dependency graph + VM executor | `graph.rs`, `exec.rs` |
| `flux_jit` | Cranelift JIT backend (numeric hot paths) | `compiler/`, `host.rs` |
| `flux_std` | Standard library builtins + I/O | `lib.rs`, `io.rs` |
| `flux_resolve` | Multi-file import resolution | `lib.rs` |
| `flux_fmt` | Source formatter (AST → source) | `lib.rs` |
| `flux_lsp` | LSP server (diagnostics, hover, completion, format, symbols, rename) | `server.rs`, `analysis.rs` |
| `flux_wasm` | WASM backend (stub) | `lib.rs` |
| `flux_diagnostics` | Spans, miette error types E000–E017 | `error.rs`, `lib.rs` |
| `flux_driver` | Unified compile pipeline + IR cache | `pipeline.rs`, `cache.rs`, `options.rs` |
| `flux_cli` | CLI binary, REPL, project init | `main.rs`, `repl.rs`, `manifest.rs` |

---

## Language features (implemented)

| Category | Features |
|----------|----------|
| **Bindings** | `let mut` (source), `let`/`derived` (derived or immutable source), `const`, reassignment in `on`/`watch` |
| **Reactivity** | `watch`, `on`, `emit`, `rfn`, derived dependency graph, watch dep filtering |
| **Functions** | `fn` (snapshot reads), return inference, generics `fn id[T](x: T) -> T`, closures `\|x\| expr` |
| **Types** | `Int`, `Float`, `Bool`, `String`, `Unit`, `[T]`, `Option[T]`, `Result[T,E]` |
| **ADT** | `struct`, `enum` (with payload variants), `match` (simple + payload arms) |
| **OOP** | `trait`, `impl Trait for Type` |
| **Control flow** | `if/else` (expr + stmt), `while`, `for x in expr`, `return`, `break`, `continue` |
| **Modules** | `import path { sym }` — multi-file resolution via `flux_resolve` |
| **Safety** | `snap()` in pure `fn`, E002 on reactive read in pure context |
| **Operators** | Arithmetic, comparison, `&&`/`\|\|` (short-circuit), `!`, string `+` |

---

## CLI commands

| Command | Description |
|---------|-------------|
| `flux run [--jit] file.fl` | Compile and execute (VM default; JIT via `--jit`, `FLUX_JIT=1`, or `flux.toml [run] jit`) |
| `flux repl [--jit]` | Interactive shell |
| `flux check [file.fl]` | Type-check only (defaults to `flux.toml` entry) |
| `flux fmt file.fl [--write] [--check]` | Format source code (`--check` exits 1 if output would differ) |
| `flux migrate [--write] path` | Migrate v1 binding syntax to v2 (`let` / `let mut`) |
| `flux init [path]` | Create `flux.toml` + `main.fl` |
| `flux lsp file.fl` | One-shot JSON diagnostics |
| `flux lsp-server` | Full stdio LSP (diagnostics, hover, completion, go-to-def, format, document symbols, rename) |
| `flux wasm file.fl` | WASM stub backend |
| `flux repl` | Interactive shell (`:help`, `:type`, `:types`, `:clear`, `:show`) |

---

## Reactive model

Module-level bindings form a **dataflow graph**:

- **Source nodes** — writable bindings (`let mut count = 0`, `on Click { count = count + 1 }`)
- **Derived nodes** — read-only, recomputed when dependencies change (`let doubled = count * 2`)
- **Watch blocks** — side-effect code that reruns on dependency changes
- **Event handlers** — `on Event { ... }` triggered by `emit Event`
- **Reactive functions** — `rfn` reads are tracked; rfn calls participate in derived graph

The runtime (`flux-runtime`) maintains the graph and propagates updates after each source mutation.
JIT (`flux_jit`) compiles numeric derived nodes, pure `fn` bodies, and numeric `watch`/`on` handlers via Cranelift; dynamic `LazyIf` uses JIT for evaluation with a VM pass for dependency tracking; strings/emit fall back to VM.

---

## Diagnostics

| Code | Meaning |
|------|---------|
| E000 | Parse error |
| E001 | Undefined name |
| E002 | Reactive read in pure `fn` (use `snap()`) |
| E003 | Assign to derived binding |
| E004 | Duplicate definition |
| E005 | Type mismatch |
| E006 | Unknown event |
| E007 | Unknown type |
| E008 | Control flow in derived expression |
| E009 | Assign to immutable local or const |
| E010 | Cyclic reactive dependency |
| E011 | Reactive effect (`watch`/`on`) inside function |
| E016 | `let` without `mut` reassigned (use `let mut`) |
| E017 | Side effect in derived expression (print, I/O) |

---

## Test suite

`cargo test --workspace` exercises parser, HIR, type checker, IR, VM, JIT, CLI, LSP, and module resolution. `flux_cli/tests/programs.rs` checks 65 accepted and 20 rejected `.fl` fixtures; `flux_cli/tests/core_contract.rs` checks cross-stage type and symbol behavior. The integration suite also drives reactive sample applications and compares VM/JIT behavior.

---

## JIT backend (`flux_jit`)

VM remains the default executor. `flux run --jit` (or `FLUX_JIT=1` / `[run] jit = true` in `flux.toml`) enables a hybrid Cranelift path:

| IR | JIT strategy |
|----|----------------|
| Numeric derived nodes | Native code; tag-aware int/float slots |
| Pure numeric `fn` | Compiled; params in host slot pairs |
| Numeric `watch`/`on` | Stmt bodies with `if`/`while`/`for`/`emit` when eligible |
| `LazyIf` deps | Static branch `Load` deps (no dual VM eval) |
| String const, `+`, `==` | Tag 5 + intern pool; host `flux_jit_string_concat` / `flux_jit_value_eq` |
| User enum match | VM fallback until branch-local binding parity is implemented |
| `for` over arrays, structs | VM fallback (`vm_fallback` set) |

`flux run --jit --verbose` prints `JitStats` (compiled count, fallback reasons, node names). JIT engines are cached per-thread keyed by `(IR hash, JIT_CACHE_VERSION)`.

**Tagged-value heap lifetime.** The VM works with owned `Value`s and never touches the process-wide pools in `flux_value::heap`; only the JIT boundary does. Each exchange with compiled code (derived evaluation, function call, or `watch`/`on` run, including reading back the handler's slots) runs inside a `flux_value::heap_scope()`. When the outermost scope on a thread ends, the arrays and enums allocated in it are freed and its string references are released. Strings interned outside any scope, such as compile-time constants embedded in machine code, are pinned for the process lifetime. Scoped strings are reference-counted across threads, and ids are never reused, so a stale handle reads as missing rather than as another value. `flux_cli/tests/heap_lifetime.rs` checks that repeated events keep the pools flat, that results match the VM, and that concurrent executors stay correct.

---

## Roadmap

### v0.1 — closed ✅
Full compiler pipeline, VM, reactive graph, events, watch, rfn, struct/enum/match, generics (syntax + monomorphize), traits (syntax + basic dispatch), closures, modules, tests.

### v0.2 — closed ✅
Cranelift JIT backend, stdlib builtins (print, len, range, push, pop, filter, reduce, str\_*), formatter, LSP diagnostics API, `flux init`, compile cache.

### v0.3 — in progress
Higher-order functions with closures (map/filter/reduce with fn args), generics E2E coverage, trait method dispatch verified, `flux.toml` integrated into `run`/`check`.

### v1.0 — planned
Full LSP server (hover, completion, go-to-def), WASM codegen, pattern matching extensions (guards, destructuring), string interpolation, method syntax, package manager, crates.io publish.

---

## Running the project

```bash
# Run tests
cargo test

# Execute a program (VM)
cargo run -p flux_cli -- run programs/ok/basics/basics.fl

# Execute with Cranelift JIT
cargo run -p flux_cli -- run --jit programs/demo/counter.fl

# Interactive REPL
cargo run -p flux_cli -- repl

# Type-check only
cargo run -p flux_cli -- check programs/ok/basics/basics.fl

# Format source
cargo run -p flux_cli -- fmt programs/ok/basics/basics.fl
```
