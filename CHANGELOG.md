# Changelog

Notable changes to Flux. The versioning policy is in [`RELEASE.md`](RELEASE.md).

## [0.1.0] - 2026-11-03

The first version whose language surface is declared stable. The earlier history is in the git log.

### Language

- Functions, `if`/`while`/`for`, `match` with guards and nested patterns, and a value for every block.
- `Int8` to `Int64`, `UInt8` to `UInt64`, `Float32`, `Float64`, `Bool`, `Char`, `String`, arrays, `Map`, `Option`, `Result`, `struct`, `enum`, recursive types, type aliases, generics, closures, function types, and traits with static dispatch.
- Expected failures as values with `?`, which requires an exact error type. Runtime errors with a file, line, and column trace.
- Reactive values: `let mut`, `derived`, `event`, `on`, `watch`, `rfn`, and `snap`.
- Modules with `pub`, and local packages with `flux.lock`.
- Checked integer arithmetic and casts, with documented limits on recursion, collection size, and nesting.

### Tools

- `flux init`, `run`, `check`, `test`, `fmt`, `lock`, `repl`, and `lsp-server`.
- Standard library: `io`, `collections`, `math`, `str`, `option`, `result`, `testing`.
- A `dist` build profile with link-time optimization.

### Experimental

- `--jit`, a Cranelift backend that must match the VM or fall back to it.
- `flux wasm`, a stub that always fails.
- `flux migrate`.

### Decided just before this release

Each of these can change what an existing program does:

- A function's value comes only from its last statement. `let`, an assignment, a loop, or an `if` without `else` gives `Unit`.
- A `reduce` callback must return the exact type of the initial value.
- A letter, digit, or `_` right after a number is a parse error.
- `Ok`, `Err`, `Some`, and `None` need no import. A module's own item, variant, or local of that name takes priority.
- Inside a function, a `match` arm may `emit` and assign a source.
- `true` and `false` arms cover `Bool`.
- `abs` of an integer keeps its type: `abs(-3)` is `3`, not `3.0`, and the smallest value of a signed type is `integer overflow`.
- Number literals such as `1e3`, `1_000`, and `0xff`, and ordering of strings by code point, are new.
