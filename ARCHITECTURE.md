# Flux architecture

## Compiler pipeline

```
.fl  →  flux_syntax  →  flux_resolve  →  flux_hir  →  flux_types  →  flux_ir  →  flux_runtime
              │                │               │             │              │              │
           pest AST        import graph     scopes/HIR    mono+check     bytecode     graph + VM
              │                │               │             │              │              │
              └──────────────── flux_diagnostics (Span, miette E000–E025) ─────────────────┘
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
| `flux_diagnostics` | Spans, miette error types E000–E025 | `error.rs`, `lib.rs` |
| `flux_driver` | Unified compile pipeline + IR cache | `pipeline.rs`, `cache.rs`, `options.rs` |
| `flux_cli` | CLI binary, REPL, project init | `main.rs`, `repl.rs`, `manifest.rs` |

---

## Reactive model

Module-level bindings form a **dataflow graph**:

- **Source nodes** — writable bindings (`let mut count = 0`, `on Click { count = count + 1 }`)
- **Derived nodes** — read-only, recomputed when dependencies change (`let doubled = count * 2`)
- **Watch blocks** — side-effect code that reruns on dependency changes
- **Event handlers** — `on Event { ... }` triggered by `emit Event`
- **Reactive functions** — `rfn` reads are tracked; rfn calls participate in derived graph

The runtime (`flux-runtime`) keeps the graph in `graph.rs` and runs programs in `exec.rs`. Event dispatch and settling use node ids and reused buffers in `ReactiveState`, kept behind a `Box` so that it does not spread the fields calls use over more cache lines. A derived value with a conditional reads its dependencies as it runs (`tracking_scope`), so it depends only on the branch it took.

Statements carry their source location in `StmtBlock.locs`, beside the statements rather than between them, because only a runtime error reads it. `SourceMap` turns a location into a path, line, and column.

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
| Numeric `watch`/`on` | Stmt bodies with `if`/`while`/`for`, when eligible; a body that emits runs on the VM |
| Conditional reading a reactive node | VM, which records the branch's dependencies (`dynamic_dependencies`) |
| String const, `+`, `==`, ordering, `to_string` | Tag 5 + intern pool; host calls |
| `match` on integers and user enums | Native, with bindings as Cranelift variables |
| Structs, closures, `Option`, `Result`, maps | VM: no tagged form (`vm_fallback` set) |

Native code has no observable effects until it succeeds, so when it fails the result is discarded and the VM runs that entry again, and everything after it (`vm_only`). This makes the error text and trace the VM's, and keeps a failed handler's earlier writes.

`flux run --jit --verbose` prints `JitStats` (compiled count, fallback reasons, node names). JIT engines are cached per-thread keyed by `(IR hash, JIT_CACHE_VERSION)`.

**Tagged-value heap lifetime.** The VM works with owned `Value`s and never touches the process-wide pools in `flux_value::heap`; only the JIT boundary does. Each exchange with compiled code (derived evaluation, function call, or `watch`/`on` run, including reading back the handler's slots) runs inside a `flux_value::heap_scope()`. When the outermost scope on a thread ends, the arrays and enums allocated in it are freed and its string references are released. Strings interned outside any scope, such as compile-time constants embedded in machine code, are pinned for the process lifetime. Scoped strings are reference-counted across threads, and ids are never reused, so a stale handle reads as missing rather than as another value. `flux_cli/tests/heap_lifetime.rs` checks that repeated events keep the pools flat, that results match the VM, and that concurrent executors stay correct.

---
