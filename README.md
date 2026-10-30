# Flux

Flux is a **reactive programming language** with a clean compiler pipeline written in Rust. Module-level bindings form a dependency graph that propagates changes automatically — like a spreadsheet, but as a general-purpose language.

## Quick start

```bash
# Run tests
cargo test

# Execute a program
cargo run -p flux_cli -- run programs/ok/basics/basics.fl

# Run the entry point declared in ./flux.toml (no file argument needed)
cargo run -p flux_cli -- run

# With Cranelift JIT (numeric hot paths): add --jit; --quiet drops the banner, --verbose adds events/effects
cargo run -p flux_cli -- run --verbose programs/demo/counter.fl

# Type-check only (import-aware)
cargo run -p flux_cli -- check programs/ok/basics/basics.fl

# Format source (stdout), or --write to format in place
cargo run -p flux_cli -- fmt programs/ok/basics/basics.fl --write

# New project (flux.toml + main.fl)
cargo run -p flux_cli -- init my_app

# LSP-style diagnostics (JSON, one-shot)
cargo run -p flux_cli -- lsp programs/ok/basics/basics.fl

# Full language server (diagnostics + hover + completion, stdio)
cargo run -p flux_cli -- lsp-server

# Interactive REPL (`:emit Event [payload]` fires events against the live session)
cargo run -p flux_cli -- repl
```

See [SPEC.md](SPEC.md) for the language specification.

## Editor setup

`flux lsp-server` speaks the Language Server Protocol over stdio and provides diagnostics (with precise ranges, refreshed on every edit), hover (binding/function/event types), and completion (keywords, snippets, and module-level bindings). To wire it up in Cursor or VS Code, point a generic LSP client extension (e.g. "Generic LSP Client") at:

```
command: cargo
args: ["run", "-p", "flux_cli", "--", "lsp-server"]
filetypes: ["flux"]
```

or build a release binary first (`cargo build --release -p flux_cli`) and point the client at `target/release/flux lsp-server` for faster startup.

## Example

```flux
event Click

let mut count = 0
let doubled = count * 2

on Click { count = count + 1 }

watch count {
    // runs when `count` changes
}
```

## Language features (v0.4+)

| Feature | Syntax |
|---------|--------|
| Mutable source | `let mut x = 5` |
| Derived binding | `let y = x * 2` or `derived y = x * 2` |
| Immutable source | `let version = 1` or `const VERSION = 1` |
| Function | `fn add(a: Int, b: Int) -> Int { a + b }` |
| Reactive function | `rfn scale(x: Int) { x * base }` |
| Events | `event Click` / `on Click { ... }` / `emit Click` |
| Watch | `watch x { ... }` |
| Snapshot | `snap(x)` in pure `fn` |
| Arrays & loops | `for i in [1, 2, 3] { ... }` |
| Struct / enum / match | `struct Point { x: Int }` / `match c { Red => 1 }` |
| Generics | `fn id[T](x: T) -> T { x }` |
| Traits & impl | `trait Display { ... }` / `impl Display for Int { ... }` / `x.method()` |
| Closures | `\|x\| x * 2`, capturing module `Source` bindings |
| Higher-order fns | `map(xs, f)`, `filter(xs, f)`, `reduce(xs, init, f)` |
| Control flow | `return`, `break`, `continue` |
| Modules | `import math { double }` |
| Stdlib | `print`, `println`, `len`, `range`, `push`, `pop`, `map`, `filter`, `reduce`, `str_*`, `read_line`, `read_file`, `write_file`, `append_file`, `file_exists`, `Option`, `Result` |

## Compiler pipeline

```
.fl → Parser → Resolver → HIR → Monomorphize → Types → IR → VM/JIT
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for details.

## Project structure

| Crate | Role |
|-------|------|
| `flux-syntax` | Pest parser + AST |
| `flux_hir` | HIR lowering, scopes, symbols |
| `flux_types` | Type inference and checking |
| `flux_ir` | Stack-machine bytecode |
| `flux-runtime` | Reactive graph + VM executor |
| `flux_jit` | Cranelift JIT backend |
| `flux_std` | Standard library builtins |
| `flux_resolve` | Module import resolution |
| `flux_fmt` | Source formatter |
| `flux_lsp` | Diagnostics API + stdio Language Server (hover, completion) |
| `flux_wasm` | WASM backend stub |
| `flux_cli` | CLI (`run`, `check`, `fmt`, `init`, `lsp`, `lsp-server`, `repl`) |

## License

MIT
