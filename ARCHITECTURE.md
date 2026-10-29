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
| **Functions** | `fn` (pure), return inference, generics `fn id[T](x: T) -> T`, closures `\|x\| expr` |
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

## Test suite (~113 tests, 0 failures)

| Location | Count | Focus |
|----------|-------|-------|
| `flux_cli/tests/programs.rs` | 41 | All ok/fail `.fl` programs |
| `flux_cli/tests/snapshots.rs` | 13 | Output vs `.expected` files |
| `flux_cli/tests/stress.rs` | 13 | Large graphs (100–200 nodes), rapid updates |
| `flux_cli/tests/jit.rs` | 19 | VM vs JIT parity (strings, enums, numeric) |
| `flux_cli/tests/demo.rs` | 6 | Demo program behavior |
| `flux_cli/tests/integration.rs` | 6 | Pipeline integration |
| `flux-syntax/tests/parse.rs` | 7 | Parser |
| `flux_hir/src/tests.rs` | 4 | HIR lowering |
| Other crates | ~17 | fmt, wasm, resolve, repl, exec, cache, regression |

**Programs:** 39+ ok, 8 fail (negative), 6 demo.

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
| User enum match | Tag 6 + host store; `MakeUserEnum`, `VariantTagEq`, payload, `BindLocal` |
| `for` over arrays, structs | VM fallback (`vm_fallback` set) |

`flux run --jit --verbose` prints `JitStats` (compiled count, fallback reasons, node names). JIT engines are cached per-thread keyed by `(IR hash, JIT_CACHE_VERSION)`.

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
