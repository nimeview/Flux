# Changelog

Notable changes to Flux. The versioning policy is in [`RELEASE.md`](RELEASE.md).

## [Unreleased]

### Added

- The `time` module: `now`, `after`, `every`, `cancel`, and `stop`. `flux run` keeps running while a timer is set, until `stop()`. Each firing is one batch. A component's timer for its own event sends it to that instance without keeping it alive. `flux test` does not run timers.
- `extern fn` and `pub extern fn` declare functions provided by the host. Calls are actions (E008 in a derived value, `const`, or `track fn`). `flux check` accepts them; `flux run` provides no host functions and cannot start a program that declares one.
- Rust hosts can provide functions with `Host::function` and start programs with `compile_file_with_host` or `Executor::with_host`. `EventSender::send` queues events from another thread. `run_event_loop` delivers them in order, each as one batch, and runs due timers. `stopped`, `next_timer_in`, and `run_due_timers` let a host drive time itself.
- `flux check --target cli|web`, defaulting to `cli`. `web` has no `fs`, `process`, or `env`, and omits command-line and file functions from `io`. An unavailable module or function is E034. `web` can only be checked; nothing builds or runs for it yet.
- Component `let mut` and `derived` members may hold instances. Lists and trees may change their children. An instance removed from a list and held nowhere else is dropped and stops reacting. An assignment, `push`, or derived result that would make an instance hold itself fails at run time. This includes holding itself through other instances. The error is "an instance cannot hold itself, directly or through the instances it holds". It is reported where the store happens, and the value is not stored. What an instance's `watch` remembers to detect changes does not keep instances alive. A watch with `prev` keeps the values it hands back as `prev`.
- Components declared with `live fn`, or `pub live fn` for export. Each call makes an instance with its own sources, derived values, events, handlers, and watches. Public members can be read, events emitted, and functions called through the instance. Member reads in derived values follow the instances they read. Instances are handles and live while a value holds them. Component code runs on the VM, with the same results under `--jit`.
- Recursive components, such as trees and lists. A derived value may read the same component's members through instances held in parameters or `let` members. Those instances were made first, so this is not a cycle. Such a member needs a type annotation because its type depends on itself. A value that comes to read itself at run time through instances fails with "a derived value depends on itself". It never returns an old value. Chains of tens of thousands of instances are computed without exhausting the stack, also when read from the far end inside a batch.
- Generic calls infer missing type arguments from the expected type: an annotated binding, a declared return type (tail or `return`), a parameter of known type, a struct literal field, an assignment, or `if`/`match` branches whose type is known. Type functions of generic types work too (`let b: Bag[Int] = Bag::new()`). Call sites have no explicit type arguments. A call whose type arguments remain unknown reports a type mismatch with help to give its place a type or pass arguments of known types.
- Methods can declare their own type parameters, with optional bounds: `fn map[U](self, f: fn(T) -> U) -> Box[U]` in `impl[T] Box[T]`, or `fn scaled[N: Ord](self, by: N, f: fn(Int, N) -> Int) -> Point` in `impl Point`. Calls infer them from arguments and the expected type; each distinct instantiation is specialized. A method with its own type parameters cannot take `mut self`.
- Integer bit operations: `&`, `|`, `^`, `<<`, `>>`, and bitwise `!`; `!` on `Bool` stays logical NOT. Shifts keep the left operand's type, drop shifted-out bits, and preserve the sign for signed right shifts; a negative shift or one at least the type's width is `integer overflow`. The JIT runs code with bit operations on the VM.
- `Bytes` for raw bytes: `io`'s `read_bytes` and `write_bytes`, `collections`' `bytes([Int])`, indexing as `UInt8`, `len`, `slice`, UTF-8 decoding with `to_text`, and `String.to_bytes()`.
- `mut` function parameters, written at both declaration and call (`fn grow(mut xs: [Int], n: Int)`, `grow(mut xs, 3)`): a mutable local starts as the argument and its final value is written back to the caller's variable, field, or element. Several are allowed, including in generic functions and across modules; a call is a statement or a `let` value, with a result only when declared with `-> T`.
- Private struct fields: a field is private to its module unless marked `pub` before its name. Naming another module's private field is the new error E033; completion after `.` leaves those fields out. Enum variants' named fields stay visible wherever the enum is.
- Checked integer methods on every integer type: `checked_add(m)`, `checked_sub(m)`, `checked_mul(m)`, `checked_div(m)`, `checked_rem(m)`, `checked_neg()`, and `checked_pow(exponent)`. They return `Option` of the receiver's type: `Some(result)` on success, or `None` for overflow, division by zero, an integer argument that does not fit that type, or a negative exponent. They need no import and are pure, including in derived values and `const` initializers.

### Changes that can alter programs

- Before 0.4, every struct field was visible everywhere; fields without `pub` are now private to their module. This applies to field reads, writes, `with`, destructuring `let` patterns, and struct literals. A literal must list every field, so outside its module a type with a private field must be built through the functions or methods its module provides. Migration: `flux migrate --write` adds `pub` to every field of every `pub struct` so programs compile as before; other structs and enum variants are left as they are. Then remove `pub` where a field should be private.
- A derived value, a `const` initializer, or a `track fn` may no longer run an `act fn` (E008), directly, through another function, or through a function value. An action's reads are not tracked, so a derived value computed through one could go stale even when the action only read sources. Migration: write `track fn` instead of `act fn` for a function that a derived value, `const` initializer, or `track fn` uses to compute from sources.

### Fixed

- A derived cycle among module values is reported the same way on every run (it could start at a different binding).

## [0.3.1] - 2026-11-26

### Changes that can alter programs

- `break` and `continue` outside a `while` or `for` body in the same function are now compile errors (E000). A loop outside a closure does not count inside it. Previously, a `-> Int` function with `break` could return `()`. Migration: move it into a loop or use `return` to leave the function.
- Functions and values holding a function at any depth are rejected by `==`, `!=`, array `contains`, `to_string`, interpolation, and `println` (E005). Array `contains` used to compare functions, and nesting more than 16 levels deep bypassed the equality and text checks. Migration: compare or show the data the function works on.

### Fixed

- A function or block ending in a `match` whose arms all `return` now takes the returned values' type, like an `if … else` whose branches all return. Such a `-> Int` function was rejected with "expected Int64, found Unit". Without an annotation, the return type is inferred; a return of the wrong type is still E005.
- In the REPL, `:emit Event v1 v2 …` passes all values in declaration order, separated by spaces; a quoted string is one value. It checks the count and rejects undeclared events with a message. `:emit Move 1 2` used to crash the REPL.
- A runtime error in a REPL handler run by `:emit` is printed; it used to be silent. The session continues with the sources' values as they were when the error happened.
- Recompiling a REPL session for a new line keeps sources' current values by name when their types match. Adding a declaration used to reset every source to its initial value. A watch may run once more if a copied value differs from its initial value.
- End of input (Ctrl-D, or the end of a piped script) ends the REPL session; it used to loop forever.
- `flux check --locked main.fl` and `flux run --locked main.fl` use the same project root as `./main.fl`. A bare file name used an empty root and could report a stale lock ("`geometry` added, `geometry` removed") when `./main.fl` worked.

## [0.3.0] - 2026-11-26

### Added

- Reactive components: an imported module can have `on` handlers, `watch`es, and top-level `if`s, and owns its sources. Events are used like functions: `emit counter::Inc(1)`, `emit Inc(1)` after importing it, and `on counter::Inc(by) { }` in an importer. Modules initialize dependencies first, each running its watches at the end of its own initialization.
- Trait bounds: `fn largest[T: Ord](xs: [T]) -> T`, `[T: Ord + Display]`. A call whose type lacks a bound's trait is the new error E031. The library traits `Eq`, `Ord`, and `Display` can be named in bounds; a user type gets `Display` with `impl Display for P { fn to_string(self) -> String }`.
- Impls for generic types: `impl[T] Pair[T] { … }`, `impl[T: Display] Display for Pair[T] { … }`, with type functions such as `Pair::new`.
- Generic structs and enums: `struct Pair[T] { a: T, b: T }`, `enum Tree[T] { Leaf, Node(T) }`, written `Pair[Int]`. Type arguments come from the values and from context (an annotation, a parameter, a return type), never written at construction; `let t = Leaf` alone is an error that suggests an annotation.
- Type names belong to their module: two modules may each declare `Point`. Another module's type is `geo::Point` or imported by name, and an imported enum's variants can be written bare.
- Modules for command-line programs: `env` (`env_var`, `current_dir`), `fs` (`list_dir`, `is_dir`, `create_dir`, `remove_file`, `remove_dir`), `path` (`path_join`, `file_name`, `extension`, `parent`), and `process` (`exit`, `run`), and `read_stdin` in `io`. Expected failures are `Err` with a message, like `read_file`.

### Changes that can alter programs

- Another module's `pub let mut` source is read-only: assigning it is the new error E032 (it used to be allowed in the root's handlers). Migration: give the module an event that makes the change (`pub event Reset` with `on Reset { count = 0 }`) and emit it, or a `pub act fn reset()` and call it.
- Modules initialize in dependency order (a module after the modules it imports); the order was by first import, so a module could initialize before one it imports.
- A type from another module is visible only through an import or as `geo::Point`, and only if it is `pub`; it used to be visible everywhere by its bare name. Migration: add the type to the import (`import geo { origin, Point }`) or write `geo::Point`, and mark it `pub`. Names starting with `__mod_` are reserved for the compiler.
- An integer and a float compare by exact value: `9007199254740993 == 9007199254740992.0` is now `false`; the integer used to be rounded to a float first.
- A `fn`, `track fn`, or method that changes a source is an error (E027), as announced in 0.2; it was a warning. This includes `fn main`. Migration: `flux migrate --write` turns such a `fn` into an `act fn` (for `main`, the whole body becomes one batch, so watches run once at its end); a method can call an `act fn`.

### Fixed

- A generic call as the argument of another, `id(id(3))`, was an error ("generic type arguments could not be unified").
- An array literal took its element type from its first element only, so `[None, Some(1)]` was an error; later elements now fix what the first left unknown.
- A type parameter named twice, `fn f[T, T]`, was accepted; it is an error (E004).
- At startup, a watch could miss its first run when a derived value it follows began, during initialization, to depend on another one.
- A block `match` arm ending in an `if … else` chain had the value `()` instead of the value of the branch that runs, so the program failed to type-check; it now behaves as at the end of a function body.
- When a callback of `map`, `filter`, or `reduce` failed, the remaining elements were still passed to it, so its printing and other effects went on after the error.
- A method named like a changing library method (`push`, `pop`, `set`, `sort`, `reverse`, `remove`) on a type that defines it without changing its receiver lost its value when written last in a block: `fn f(s: S) -> Int { s.push(1) }` was a type error.
- After an error in a function body, the function's parameters stayed in scope and hid the same names in the functions after it, so their errors appeared only after the first was fixed.

## [0.2.1] - 2026-11-24

### Changes that can alter programs

- `reduce` takes exactly three arguments, `reduce(xs, init, f)`. An undocumented two-argument form summed the elements as integers, so `reduce([1.5, 2.5], 0.0)` gave `3` and a large sum overflowed silently. Migration: pass the function, `xs.reduce(0, |a, x| a + x)`.
- A library function that has no single function type, such as `sqrt`, can no longer be used as a value: `let f = sqrt` is an error, as `f()` used to return `()`. Migration: wrap it, `|x| sqrt(x)`.
- A derived value may not reach a side effect through a function passed as a value (`xs.map(println)`), a method, or a binding or tuple holding a function, including one assigned later, nor read a value with `snap` through a function it calls (E017, E008). Such values printed during initialization or went stale. Migration: move the effect to a `watch`; make a function that reads a source a `track fn` instead of using `snap`.
- A `const` initializer may not have side effects such as reading a file (E017). It is computed once when the program starts. Migration: read the file in `main` or an `act fn`.
- `read_line()` returns `Err("end of input")` when there are no more lines; it returned `Ok("")`, the same as an empty line.
- `flux test` fails a test that returns a value other than nothing, `true`, or `false`; it used to pass it.

### Fixed

- `op.f(x)`, the older way to call the function in field `f`, could run a method `f` of another type when exactly one type had one. It now calls the field's function, like `(op.f)(x)`.
- Go to definition on a local that shadows a module item now goes to the local.
- A trait method written without its closing `;` reports that, instead of listing types.
- SPEC: an integer mixed with a `Float32` becomes `Float32` (the implementation's behavior); SPEC said `Float64`. The book's reactive limits now describe how a failing batch behaves.

## [0.2.0] - 2026-11-24

### Changes that can alter programs

- Reading a derived value in an `on` or `watch` body after changing one of its sources gives the new value; it used to give the value from before the body. Nested handlers read current values too. Watches still run once, when the body ends. Migration: none needed unless a program relied on reading the old value; keep it in a local before the write.
- A derived value counts as changed only when its new value differs by `==`: `0.0` becoming `-0.0` no longer runs its watches.
- Changes settle in batches. An `emit` or an `act fn` call opens a batch; all handlers of the event, nested `emit`s, and the `act fn`s they call join it, and watches run once, when it ends. Watches used to run after each handler, and a nested `emit` settled its own handlers' writes before the outer body went on. Migration: code that needs a watch to run between two changes emits them from separate top-level calls.
- A watch remembers the values it follows and runs only when one differs by `==` from what it saw last time. Assigning a source its current value, or changing it and back within one event, no longer runs its watches. Migration: none unless a program counted such no-op assignments; emit an event for them instead.
- In a nested element assignment such as `grid[f()][j] = v`, each index is now evaluated once; it used to be evaluated again for every level below it. Migration: none unless an index expression has side effects.
- A runtime error while settling no longer stops the remaining handlers of the same event from running, since settling now happens after all of them.

### Added

- Struct variants can be built: `Rect { w: 2, h: 3 }` or `Shape::Rect { … }`; they print as `Shape::Rect { h: 3, w: 2 }`.
- Hover and signature help for library methods in the editor.
- Library methods on strings, arrays, maps, `Option`, `Result`, and numbers, without an import: `text.split(",")`, `xs.sorted()`, `o.unwrap_or(0)`, and new ones: `contains`, `find`, `any`, `all`, `enumerate`, `zip`, `slice`, `sorted`, `sorted_by`, `reversed`, `first`, `last`, `get_or`, `entries`, `split_once`, `lines`, `words`, `map`/`and_then`/`map_err`/`ok_or`/`is_some`/`is_none`/`is_ok`/`is_err`, and `a + b` for arrays. `xs.push(v)`, `xs.pop()`, `xs.set(i, v)`, `xs.sort()`, `xs.reverse()`, and `m.remove(k)` change the value in place; the free functions keep returning a new value. A method that only computes a value warns (E030) when its result is dropped.
- Methods: `impl Type { }` with `self`, `mut self` (called on a place, written back only when it succeeds), type functions called as `Point::new`, and `Self`; traits may use `Self`. Methods may be added to `Int`, `String`, and the other library types, visible where their file is imported.
- Paths `::` for module items (`math::sqrt`), type functions, and variants (`Shape::Circle`). `m.name` for a module still works with warning E026, and `flux migrate` rewrites it.
- `(op.f)(3)` calls the function in field `f`; `op.f(3)` for that still works with warning E026.
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
