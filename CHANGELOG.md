# Changelog

Notable changes to Flux. The versioning policy is in [`RELEASE.md`](RELEASE.md).

## [Unreleased]

### Changes that can alter programs

- Reading a derived value in an `on` or `watch` body after changing one of its sources gives the new value; it used to give the value from before the body. Nested handlers read current values too. Watches still run once, when the body ends. Migration: none needed unless a program relied on reading the old value; keep it in a local before the write.
- A derived value counts as changed only when its new value differs by `==`: `0.0` becoming `-0.0` no longer runs its watches.
- Changes settle in batches. An `emit` or an `act fn` call opens a batch; all handlers of the event, nested `emit`s, and the `act fn`s they call join it, and watches run once, when it ends. Watches used to run after each handler, and a nested `emit` settled its own handlers' writes before the outer body went on. Migration: code that needs a watch to run between two changes emits them from separate top-level calls.
- A watch remembers the values it follows and runs only when one differs by `==` from what it saw last time. Assigning a source its current value, or changing it and back within one event, no longer runs its watches. Migration: none unless a program counted such no-op assignments; emit an event for them instead.
- In a nested element assignment such as `grid[f()][j] = v`, each index is now evaluated once; it used to be evaluated again for every level below it. Migration: none unless an index expression has side effects.
- A runtime error while settling no longer stops the remaining handlers of the same event from running, since settling now happens after all of them.

### Added

- Tuples: `(1, "a")` of type `(Int, String)`, `t.0`, `let (a, b) = t`, tuple patterns in `match` with exhaustiveness checking (`(true, _)`, `(Some(x), 0)`), and `for (k, v) in m`, which walks a map's entries in key order.
- Assigning part of a value: a field `p.x = 3`, and paths such as `todos[i].done = true` and `grid[i][j] = v`. Each index is evaluated once.
- Compound assignment `+=`, `-=`, `*=`, `/=`, and `%=` on any place.
- `p with { x: 3 }`: a copy of a struct with some fields changed.
- Warning E030 for a collection function whose result is dropped, such as `push(xs, 3)` on its own line.
- Events with several values: `event Move(dx: Int, dy: Int)`, `emit Move(1, 2)`, and `on Move(dx, dy) { }`. The older `on Tick { n -> }` still works with warning E026; `flux migrate` rewrites it to `on Tick(n) { }`.
- Warning E029 for watches that wake each other, or one that wakes itself, in a cycle, found through calls, closures, and emitted events.
- `watch total(prev) { }` binds the value the watch saw last time.
- Function kinds: `track fn` (the new spelling of `rfn`, which still works with warning E026) and `act fn`, a function that changes state; it reads current values and may take several values and return a result.
- Warnings for writes to a source outside an `on` handler, an `act fn`, or initialization: E027 for a `fn`, `track fn`, or method (an error from 0.3; the fix writes `act fn`), and E028 for a `watch`. Migration: `flux migrate --write` turns such a `fn` into an `act fn`; move a watch's write into `let` or an `on` handler.
- Raw strings `r"..."` and `r#"..."#`, with no escapes or interpolation.
- `;` between statements on one line, and a comma after the last element of any list.

### Fixed

- With `--jit`, a function that writes a source lost the write when called from code the VM runs, and a compiled function or handler could read a derived value from before a write in the same handler. Both now run on the VM in those cases.
- An event declared with more than one value, such as `event Move(x: Int, y: Int)`, silently dropped all but the first. Events now carry all their values.

### Tools

- Warning E026 marks an older form and carries its replacement. The editor offers it as a quick fix, and `flux migrate` applies it, along with the `act fn` fix of E027.

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
- A VS Code extension in `editors/vscode`.
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
