# Flux Language Specification (v1.0 draft)

Flux is a statically typed language with **built-in reactivity**: module-level bindings form a dataflow graph, and `watch` / `on` / `rfn` integrate declarative updates with imperative code.

## 1. Lexical structure

- **Comments**: `//` to end of line
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`
- **Literals**: integers, floats, booleans (`true`/`false`), double-quoted strings

## 2. Types

| Type | Description |
|------|-------------|
| `Int` | Primary integer type (alias for `Int64`; integer literals infer as `Int64`) |
| `Int32`, `Int64`, `UInt64` | Fixed-width integers |
| `Float`, `Float32`, `Float64` | Floating point (`Float` is an alias for `Float64`) |
| `Bool`, `String`, `Unit` | Primitives |
| `[T]` | Homogeneous array |
| `Option[T]` | Optional value (`Some`, `None`) |
| `Result[T, E]` | Success or error (`Ok`, `Err`) |
| `struct Name { f: T, ... }` | Product type |
| `enum Name { A, B(T), ... }` | Sum type |
| `fn(T, ...) -> U` | Function |
| `Cell<T>` / `Read<T>` | Reactive wrapper (internal; sources/derived) |

Generics: `fn id[T](x: T) -> T` — monomorphized at compile time per call site.

### Casts

`expr as Type` converts between numeric types when the conversion is defined (e.g. `x as Int32`, `n as Float64`). The checker uses a promotion lattice for mixed arithmetic; use explicit casts when you need a specific width.

```flux
import collections
let xs = range(10)          // [Int64]
let n = len(xs) as Int32    // Int32
```

## 3. Module items

```flux
import path
import path { sym1, sym2 }

struct Point { x: Int, y: Int }
enum Color { Red, Green }

trait Display { fn to_string(x: Int) -> String; }
impl Display for Int { fn to_string(self: Int) -> String { "x" } }

event Click
fn add(a: Int, b: Int) -> Int { a + b }
rfn scaled(x: Int) { x * factor }

const VERSION = 1              // compile-time frozen
let mut counter = 0            // source — reassignable in on/watch
let doubled = counter * 2      // derived — reactive read in RHS
derived total = a + b          // explicit derived sugar (optional)

watch counter { ... }
on Click { counter = counter + 1 }
```

### Binding rules (v2)

| Form | Role | Reassign in `on`/`watch` |
|------|------|--------------------------|
| `let mut name = expr` | **Source** (writable) | Yes |
| `let name = expr` + reactive read in RHS | **Derived** (read-only) | No |
| `let name = expr` (no reactive reads) | **Source** (immutable after init) | No |
| `derived name = expr` | **Derived** | No |
| `const NAME = expr` | Compile-time constant | No |

Top-level bare assignment (`x = 0`) and `set` statements are removed. Reassignment uses `name = expr` only inside `on`/`watch` for `let mut` sources, or `let mut` locals inside functions.

Inside `fn` / `rfn`:

- `let x = 1` — immutable local
- `let mut x = 1` — mutable local (replaces `set`)
- `x = expr` — assign to `let mut` local or module source from reactive context

## 4. Statements

`let` / `let mut`, assignment, `if`/`else`, `while`, `for x in expr`, `return expr?`, `break`, `continue`, `emit Event(args?)`, expression statements.

**Derived expressions** (right-hand side of derived bindings) must be pure: no `snap()`, loops, control flow (**E008**), or side-effect builtins such as `print` / I/O (**E017**).

## 5. Expressions

Arithmetic (`+ - * /`), comparisons, boolean ops (`&&`, `||`, `!`), `if e { a } else { b }`, `match e { Var => expr, ... }`, calls, field access (`x.f`), closures `|x| x + 1`, `snap(expr)` in reactive contexts.

### Closures

`|x| expr` or `|x: T| expr`. Parameter types are inferred from the call site (e.g. the array element type for `map`/`filter`/`reduce` callbacks) when not explicitly annotated, falling back to `Int`. Closures may capture module-level **Source** or **Derived** bindings from their enclosing scope (e.g. `filter(nums, |x| x > threshold)` or `filter(nums, |x| x > doubled)`); capturing **Derived** bindings is supported when the closure is used in a reactive (derived) context. A closure that captures a reactive binding is itself reactive: calls that use it are re-evaluated whenever captured dependencies change.

### Trait method dispatch

`impl Trait for Type { fn method(self: Type, ...) -> U { ... } }` methods can be called with `expr.method(args)` syntax (e.g. `(42).to_string()`, `42.to_string()`). Dispatch is resolved statically: the typechecker matches the receiver's type against registered `impl` blocks, so multiple types can implement the same method name unambiguously (e.g. `Display` for both `Int` and `String`).

### Reactive semantics

| Construct | Context | Reactive reads |
|-----------|---------|----------------|
| `fn` | Pure | Forbidden (use `snap`) |
| `rfn` | Reactive function | Tracked |
| `watch` / `on` | Side-effect blocks | Tracked |
| Module derived | `let y = f(x)` or `derived y = f(x)` | Tracked |
| Closure (captures a source or derived) | Any | Tracked |

`watch` and `on` are only valid at module level; using them inside a `fn` or `rfn` body is a compile error (**E011**).

### Type aliases

`type UserId = Int` introduces a name synonymous with an existing type for annotations and checking.

### Const bindings

`const N = 42` at module level declares an immutable source binding whose value is fixed at compile time.

### Match guards

`match x { n if n > 0 => n, _ => 0 }` — patterns may include `if` guards evaluated after the pattern binds.

### String interpolation

`"Hello, {name}!"` desugars to string concatenation.

### Destructuring let

`let Point { x, y } = pt` binds struct fields from a value.

## 6. Standard library (stable v1.0)

Standard-library symbols are **not** in the global prelude. Import the module you need:

```flux
import io
import collections
import math
import str
import option
import result
```

Selective imports: `import io { println, read_line }`. Stdlib modules live under the project `stdlib/` directory and are resolved automatically — they are **not** listed in `flux.toml` dependencies. A local file with the same name (e.g. `./math.fl`) shadows the stdlib module.

| Module | Symbols |
|--------|---------|
| `io` | `print`, `println`, `read_line`, `read_file`, `write_file`, `append_file`, `file_exists` |
| `collections` | `len`, `range`, `push`, `pop`, `map`, `filter`, `reduce` |
| `math` | `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow`, `assert`, `assert_eq` |
| `str` | `str_len`, `str_split`, `str_join`, `str_contains`, `str_trim`, `str_starts_with`, `str_ends_with`, `str_replace`, `str_to_lower`, `str_to_upper` |
| `option` | `Some`, `None` |
| `result` | `Ok`, `Err` |

| Symbol | Type | Behavior |
|--------|------|----------|
| `print` | `fn(String?) -> Unit` | Write to stdout |
| `println` | `fn(String?) -> Unit` | Write line |
| `len` | `fn([T] \| String) -> Int` | Length |
| `range` | `fn(Int) -> [Int]` | `0..n-1` as array |
| `range` | `fn(Int, Int) -> [Int]` | `start..end-1` (step 1) |
| `range` | `fn(Int, Int, Int) -> [Int]` | `start..end-1` with explicit step |
| `push` | `fn([T], T) -> [T]` | Append element (returns new array) |
| `pop` | `fn([T]) -> [T]` | Remove last element |
| `map` | `fn([T], fn(T) -> U) -> [U]` | Transform each element |
| `filter` | `fn([T], fn(T) -> Bool) -> [T]` | Keep elements matching predicate |
| `reduce` | `fn([T], U, fn(U, T) -> U) -> U` | Fold to a single accumulator |
| `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow` | math | Numeric intrinsics (`Int`/`Float` overloads where applicable) |
| `str_len` | `fn(String) -> Int` | Length in characters |
| `str_split` | `fn(String, String) -> [String]` | Split on separator |
| `str_join` | `fn([String], String) -> String` | Join with separator |
| `str_contains` | `fn(String, String) -> Bool` | Substring check |
| `str_trim` | `fn(String) -> String` | Trim whitespace |
| `read_line` | `fn() -> Result[String, String]` | One line from stdin (no trailing newline) |
| `read_file` | `fn(String) -> Result[String, String]` | Read UTF-8 file contents |
| `write_file` | `fn(String, String) -> Result[Unit, String]` | Write UTF-8 (create/truncate) |
| `append_file` | `fn(String, String) -> Result[Unit, String]` | Append UTF-8 to file |
| `file_exists` | `fn(String) -> Bool` | Whether path exists |
| `Some` / `None` | Option constructors | |
| `Ok` / `Err` | Result constructors | |

`map`/`filter`/`reduce` are parametric over the array's element type `T` (and accumulator `U` for `reduce`); the typechecker infers `T`/`U` from the argument expressions at each call site rather than from a single fixed signature.

## 7. Diagnostics

| Code | Meaning |
|------|---------|
| E000 | Parse error |
| E001 | Undefined name |
| E002 | Reactive read in pure `fn` |
| E003 | Assign to derived |
| E004 | Duplicate definition |
| E005 | Type mismatch |
| E006 | Unknown event |
| E007 | Unknown type |
| E008 | Control flow in derived |
| E009 | Assign to immutable local or const |
| E010 | Cyclic dependency in reactive graph |
| E011 | `watch`/`on` outside module level |
| E016 | `let` without `mut` cannot be reassigned (use `let mut`) |
| E017 | Side effect (`print`, I/O) in derived expression |

## 8. Tooling

| Command | Description |
|---------|-------------|
| `flux run [--jit] [--verbose] file.fl` | Compile and execute |
| `flux repl [--jit]` | Interactive shell |
| `flux check file.fl` | Type-check only |
| `flux fmt file.fl [--write] [--check]` | Format source (`--check` fails if reformat needed) |
| `flux migrate [--write] path` | Migrate v1 top-level bindings to v2 `let` / `let mut` |
| `flux init [path]` | Create `flux.toml` + entry file |
| `flux lsp file.fl` | JSON diagnostics |
| `flux lsp-server` | Stdio LSP: diagnostics, hover, completion, go-to-def, format, document symbols, rename |
| `flux wasm file.fl` | WASM stub backend |
| `flux repl` | Interactive shell (v2 syntax help) |

## 9. Package manifest (`flux.toml`)

```toml
[package]
name = "my_app"
version = "0.1.0"
entry = "main.fl"

[run]
jit = false   # optional; enable JIT by default for this project
```

## 10. Execution

Pipeline: **parse → resolve imports → HIR → monomorphize → typecheck → IR → VM/JIT**.

- Default: stack VM with reactive graph
- JIT opt-in: `--jit`, environment `FLUX_JIT=1`, or `[run] jit = true` in `flux.toml`
- Cranelift JIT compiles numeric derived nodes, pure functions, and eligible `watch`/`on` bodies with tag-aware int/float arithmetic; `LazyIf` branch deps collected statically; VM fallback for strings, enums, and ineligible IR
- `flux run --jit --verbose` prints JIT compile/fallback statistics
- WASM backend: stub module header (future: full codegen)
