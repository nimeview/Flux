# Flux Language Specification (draft)

Flux is a statically typed language with **built-in reactivity**: module-level bindings form a dataflow graph, and `watch` / `on` / `rfn` integrate declarative updates with imperative code.

## 1. Lexical structure

- **Comments**: `//` to end of line
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`
- **Literals**: integers, floats, booleans (`true`/`false`), double-quoted strings
- **String escapes**: `\n` (newline), `\t`, `\r`, `\0`, `\\`, `\"`, and `\{` / `\}` for literal braces (a bare `{name}` interpolates). Any other `\x` is a parse error.

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

`expr as Type` converts between numeric types when the conversion is defined (e.g. `x as Int32`, `n as Float64`). Numeric values can also be cast to `Bool` or `Char` where valid; `Bool` and `Char` can be cast to numeric types. Unrelated casts such as `true as String` are compile errors. The checker uses a promotion lattice for mixed arithmetic; use explicit casts when you need a specific width.

Unsuffixed integer literals must fit `Int64`; a `u64` suffix permits values through `18446744073709551615u64`. The signed minimum `-9223372036854775808` is accepted as a single negative literal.

### Numeric semantics

The VM is the reference; the JIT must produce the same value or error, or fall back to the VM. `flux_cli/tests/numeric_contract.rs` checks every rule below on both engines.

| Operation | Integers (`Int8`…`Int64`, `UInt8`…`UInt64`) | Floats (`Float32`, `Float64`) |
|---|---|---|
| `+ - *` | Checked at the operand width; leaving the range is the runtime error `integer overflow` | IEEE 754 |
| `/` | Truncates toward zero; divisor `0` → `integer division by zero`; `MIN / -1` → `integer overflow` | IEEE 754 (`1.0 / 0.0` is `inf`, `0.0 / 0.0` is `NaN`) |
| `%` | Sign follows the dividend; divisor `0` → `integer division by zero`; `MIN % -1` is `0` | IEEE 754 remainder |
| unary `-` | `-MIN` → `integer overflow` | IEEE 754 |
| `== < …` | Exact (no conversion through `Float`) | IEEE 754 (`NaN != NaN`) |

- Mixed `Int`/`Float` arithmetic and comparison promote the integer to `Float64`. Operators on two integers keep the integer type, so `Int / Int` is `Int`: division by zero is a runtime error rather than a value of another type.
- Casts are checked: an integer out of the target range, a negative value cast to an unsigned type, or a `NaN`/infinite/out-of-range float cast to an integer is the runtime error `numeric cast out of range`. Float → integer casts truncate toward zero.
- A runtime error stops the current evaluation and is reported as `runtime error: <message> in <context>`, where the context names the binding, `watch`, or `on` handler. Statements after the failing one do not run. If the error happens during module initialization, `flux run` exits non-zero. Out-of-range integer *literals* are rejected at parse time.
- `range` never overflows near the `Int64` limits. A range longer than 10,000,000 elements is the runtime error `allocation limit exceeded`, not an out-of-memory abort.

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

**Block value.** A function body's value is its final expression. When the final statement is an `if … else` (including `else if` chains), the value is the final expression of the branch that runs. Both branches must have compatible types, with no numeric widening. An `if` without `else` has type `Unit`.

**Statement boundaries.** A newline does not end an expression that can continue. A line starting with `-` continues the previous line as a subtraction (`f()` followed by a line `-1` parses as `f() - 1`). Use `0 - 1`, parentheses, or put the value first.

**Derived expressions** (right-hand side of derived bindings) must be pure: no `snap()`, loops, control flow (**E008**), or side-effect builtins such as `print` / I/O (**E017**), including effects reached through user function calls. `snap()` is available in ordinary `fn` bodies for explicit snapshot reads.

## 5. Expressions

Arithmetic (`+ - * / %`), comparisons, boolean ops (`&&`, `||`, `!`), `if e { a } else { b }`, `match e { Var => expr, ... }`, calls, field access (`x.f`), closures `|x| x + 1`, `snap(expr)` in reactive contexts.

### Closures

`|x| expr` or `|x: T| expr`. Parameter types are inferred from the call site (e.g. the array element type for `map`/`filter`/`reduce` callbacks) when not explicitly annotated, falling back to `Int`. Closures may capture module-level **Source** or **Derived** bindings from their enclosing scope (e.g. `filter(nums, |x| x > threshold)` or `filter(nums, |x| x > doubled)`); capturing **Derived** bindings is supported when the closure is used in a reactive (derived) context. A closure that captures a reactive binding is itself reactive: calls that use it are re-evaluated whenever captured dependencies change.

### Trait method dispatch

`impl Trait for Type { fn method(self: Type, ...) -> U { ... } }` methods can be called with `expr.method(args)` syntax (e.g. `(42).to_string()`, `42.to_string()`). Dispatch is resolved statically: the typechecker matches the receiver's type against registered `impl` blocks, so multiple types can implement the same method name unambiguously (e.g. `Display` for both `Int` and `String`).

### Reactive semantics

| Construct | Context | Reactive reads |
|-----------|---------|----------------|
| `fn` | Ordinary function | Forbidden (use `snap`) |
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

### Block match arms

An arm may be a block: `Err(e) => { println(e)\n  0 }`. Its statements run in order, after the pattern binds, and the block's **final expression** is the arm's value. A block that does not end in an expression is a parse error. Allowed statements: `let`, assignment to a `let mut` local, and expression statements. `if`/`while`/`for`, `return`/`break`/`continue`, `emit`, and assignments to module bindings or immutable locals are compile errors in an arm block for now; move such logic into a function.

### String interpolation

`"Hello, {name}!"` desugars to string concatenation.

### Destructuring let

`let Point { x, y } = pt` binds struct fields from a value.

## 6. Standard library

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
| `io` | `print`, `println`, `eprint`, `eprintln`, `args`, `read_line`, `read_file`, `write_file`, `append_file`, `file_exists` |
| `collections` | `len`, `range`, `push`, `pop`, `get`, `map`, `filter`, `reduce` |
| `math` | `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow`, `assert`, `assert_eq` |
| `str` | `str_len`, `str_split`, `str_join`, `str_contains`, `str_trim`, `str_starts_with`, `str_ends_with`, `str_replace`, `str_to_lower`, `str_to_upper`, `to_string`, `parse_int`, `parse_float` |
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
| `get` | `fn([T], Int) -> Option[T]` | `Some(xs[i])` when `0 <= i < len(xs)`, otherwise `None` (never an error); `i` may be any integer type |
| `map` | `fn([T], fn(T) -> U) -> [U]` | Transform each element |
| `filter` | `fn([T], fn(T) -> Bool) -> [T]` | Keep elements matching predicate |
| `reduce` | `fn([T], U, fn(U, T) -> U) -> U` | Fold to a single accumulator |
| `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow` | math | Numeric intrinsics (`Int`/`Float` overloads where applicable) |
| `str_len` | `fn(String) -> Int` | Length in Unicode scalar values |
| `str_split` | `fn(String, String) -> [String]` | Split on separator |
| `str_join` | `fn([String], String) -> String` | Join with separator |
| `str_contains` | `fn(String, String) -> Bool` | Substring check |
| `str_trim` | `fn(String) -> String` | Trim whitespace |
| `to_string` | `fn(T) -> String` | Text form of any value; a `String` is returned unchanged (no quotes), others use their display form (`42`, `2.5`, `true`, `[1, 2]`) |
| `parse_int` | `fn(String) -> Result[Int, String]` | Optional `+`/`-` then ASCII digits, within `Int64`; no whitespace trimming; otherwise `Err` naming the input |
| `parse_float` | `fn(String) -> Result[Float, String]` | Decimal or exponent notation (`-2.5`, `1e3`); `inf`/`NaN` and non-finite results are `Err`; no trimming |
| `eprint` / `eprintln` | `fn(String?) -> Unit` | Like `print`/`println`, but to stderr |
| `args` | `fn() -> [String]` | Program arguments given after `--` (`flux run app.fl -- a b`); empty otherwise |
| `read_line` | `fn() -> Result[String, String]` | One line from stdin (no trailing newline) |
| `read_file` | `fn(String) -> Result[String, String]` | Read UTF-8 file contents |
| `write_file` | `fn(String, String) -> Result[Unit, String]` | Write UTF-8 (create/truncate) |
| `append_file` | `fn(String, String) -> Result[Unit, String]` | Append UTF-8 to file |
| `file_exists` | `fn(String) -> Bool` | Whether path exists |
| `Some` / `None` | Option constructors | |
| `Ok` / `Err` | Result constructors | |

**Constructor types.** `Some(x)` is `Option[T]` where `T` is the type of `x`; `None` (written bare or as `None()`) is `Option[_]`; `Ok(x)` is `Result[T, _]`; `Err(e)` is `Result[_, E]`. An unknown part `_` takes its type from context: an annotation, a parameter, a return type, or the other branches of an `if`/`match` (`if c { Some(n) } else { None }` is `Option[Int]`). Known parts must still agree (`Some("s")` is not an `Option[Int]`). In an unannotated binding, a part that is still unknown keeps the historical default: `Option[Unit]`, `Unit` for `Ok`'s side, `String` for `Err`'s side. A bare `[]` binding is still an error.

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
jit = false   # optional; set true to use the experimental JIT for this project
```

## 10. Execution

Pipeline: **parse → resolve imports → HIR → monomorphize → typecheck → IR → VM/JIT**.

- Default: stack VM with reactive graph
- **Entry point.** If the root module defines `fn main()` or `fn main() -> Int` (no parameters; any other signature is a compile error), `flux run` calls it after module initialization. The program then owns stdout: no banner and no binding dump are printed. The exit status is `main`'s `Int` result (it must be 0..=255; another value is an error with status 1), or 0 for a `Unit` `main`. A runtime error in `main` is reported on stderr with status 1. Without `main`, `flux run` prints the module's reactive bindings as before.
- `const` bindings are compile-time constants: a pure `fn` (including `main`) may read them. Reading `let` / `let mut` module bindings from a pure `fn` is still a reactive read (E002).
- JIT opt-in: `--jit`, environment `FLUX_JIT=1`, or `[run] jit = true` in `flux.toml`
- Cranelift JIT compiles numeric derived nodes, pure functions, and eligible `watch`/`on` bodies with tag-aware int/float arithmetic; `LazyIf` branch deps collected statically; VM fallback for strings, pattern matching, and ineligible IR
- `flux run --jit --verbose` prints JIT compile/fallback statistics
- WASM backend: stub module header (future: full codegen)
