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
- **Display.** A finite Float always prints with a decimal point or exponent (`5.0`, `2.5`), so the type is visible in output; `inf`, `-inf`, and `NaN` print as themselves. Integers print without one.
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

### Modules and visibility

Each `.fl` file is a module with its own namespace. Inside a module, all of its own top-level items are visible, `pub` or not, together with the names its own `import`s bring in (stdlib modules included). It never sees the importing module's names.

- **Exports.** Only `pub` items can be imported. `import m { name }` brings in the listed names; importing a non-`pub` or missing name is an error (**E014**). `import m` (no list) brings in every `pub` name; `import m as a` also makes `a.name` available.
- **Resolution.** `import a.b` is resolved relative to the importing file's directory, or through the `[dependencies]` of the package that contains the importing file (the nearest `flux.toml` above it). A package can import its own dependencies but not its dependencies' dependencies.
- **One instance.** A module imported along several paths (a diamond) is loaded and initialized once. Cyclic imports are an error (**E013**).
- **Reactivity.** Importing a `pub let mut` binding gives the importer the same reactive source: assignments in the root's `on`/`watch` update it, and derived values that read it recompute.
- **Limits.** `watch`, `on`, and top-level `if` must live in the root module (an error elsewhere). Type names (`struct`, `enum`, `trait`) are program-global for now: two modules that define the same type name are a duplicate-definition error.

## 4. Statements

`let` / `let mut`, assignment, `if`/`else`, `while`, `for x in expr`, `return expr?`, `break`, `continue`, `emit Event(args?)`, expression statements.

**Block value.** A function body's value is its final expression. When the final statement is an `if … else` (including `else if` chains), the value is the final expression of the branch that runs. Both branches must have compatible types, with no numeric widening. An `if` without `else` has type `Unit`.

**Module initialization order.** Bindings with literal initializers have their values from the start. Computed initializers (`let a = f()`, `const C = B + 1`) run in dependency order: after the bindings they read, directly or through any function they call, and otherwise in declaration order. So a binding may use a `const` declared later. Top-level `if` and assignments keep their position. A cycle among initializers is an error.

**Statement boundaries.** A newline does not end an expression that can continue: a binary operator at the end of a line, or a line starting with `+`, continues it. A line that starts with a binary `-` is a compile error rather than a silent subtraction from the line above (`f()` followed by a line `-1` used to mean `f() - 1`). To subtract, end the previous line with `-`; for a negative value on its own line, write `0 - 1` or bind it with `let`. Unary minus elsewhere (`let d = -a`, `2 * -3`) is unaffected.

**Derived expressions** (right-hand side of derived bindings) must be pure: no `snap()`, loops, control flow (**E008**), or side-effect builtins such as `print` / I/O (**E017**), including effects reached through user function calls. `snap()` is available in ordinary `fn` bodies for explicit snapshot reads.

## 5. Expressions

Arithmetic (`+ - * / %`), comparisons, boolean ops (`&&`, `||`, `!`), `if e { a } else { b }`, `match e { Var => expr, ... }`, calls, field access (`x.f`), closures `|x| x + 1`, `snap(expr)` in reactive contexts.

### Closures

`|x| expr` or `|x: T| expr`. Parameter types are inferred from the call site (e.g. the array element type for `map`/`filter`/`reduce` callbacks) when not explicitly annotated, falling back to `Int`. Closures may capture module-level **Source** or **Derived** bindings from their enclosing scope (e.g. `filter(nums, |x| x > threshold)` or `filter(nums, |x| x > doubled)`); capturing **Derived** bindings is supported when the closure is used in a reactive (derived) context. A closure that captures a reactive binding is itself reactive: calls that use it are re-evaluated whenever captured dependencies change.

### Trait method dispatch

`impl Trait for Type { fn method(self: Type, ...) -> U { ... } }` methods can be called with `expr.method(args)` syntax (e.g. `(42).to_string()`, `shape.area()`). Dispatch is static. The typechecker resolves the receiver's type (a literal, a variable, a parameter, any expression; primitives, structs, and enums) against the `impl` blocks and compiles the call as a direct call to that impl, so several types can implement the same method name.

An `impl` must match its trait (**E020**): the trait must be declared, and the impl must define exactly the trait's methods, with the same parameter count, the same types for parameters after the receiver, and the same return type. Traits have no `Self` type, so the receiver's type is not compared. A method that no impl provides for the receiver's type is **E019**.

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

### Indexing

`xs[i]` is the element of array `xs` at integer index `i`, counting from 0: `names[0]`, `grid[1][0]`, `rows[i].field`, `f(x)[0]`. An index outside `0..len(xs)`, negative ones included, is the runtime error `index out of bounds`. Indexing a non-array or using a non-integer index is a type error (**E005**). To read without the possibility of an error, use `get(xs, i)`, which returns an `Option`.

**Element assignment.** `xs[i] = v`, `grid[i][j] = v`, and `m[k] = v` assign an element of a `let mut` local (or a source in a handler). They mean `xs = set(xs, i, v)`, and nested ones `grid = set(grid, i, set(grid[i], j, v))`, so the out-of-range and type rules of `set` apply. The array or map is updated in place when nobody else holds it, and a copy someone else holds keeps its old contents. In a nested assignment the index expressions are evaluated more than once.

### Maps

`Map[K, V]` maps keys to values. The key type `K` is an integer type, `String`, `Bool`, or `Char` (anything else is **E005**). A map is empty when created with `map_new()`; its type comes from an annotation (`let mut m: Map[String, Int] = map_new()`) or from the first `set`. Entries are kept in key order (integers by value, strings by code point), which makes `map_keys`, `map_values`, iteration over them, and the display form `{"a": 1, "b": 2}` deterministic.

| Operation | Result |
|-----------|--------|
| `m[k]` | the value; a missing key is the runtime error `key not found` |
| `get(m, k)` | `Some(v)` or `None` |
| `set(m, k, v)`, `m[k] = v` | `m` with `k` set to `v` |
| `len(m)` | number of entries |
| `map_has(m, k)` | `Bool` |
| `map_remove(m, k)` | `m` without `k` |
| `map_keys(m)`, `map_values(m)` | arrays, in key order |

Maps are equal when they have the same keys with equal values. The JIT has no form for maps, so code that receives or builds one runs on the VM. A map holds at most 10,000,000 entries.

### Equality

`==` and `!=` (and `assert_eq`) compare two values of the same type when that type is equatable: numbers (any widths, by value, Float by IEEE 754), `Bool`, `Char`, `String`, `()`, and arrays, `Option`, `Result`, structs, and enums whose parts are equatable. Compound values are equal when their parts are, by the same rules: `[0.0 / 0.0] == [0.0 / 0.0]` is `false`, and `Some([1]) == Some([1])` is `true`. Comparing functions, or values of different types, is a type error (**E005**).

### `if` and `match` as operands

An `if … else` expression and a `match` are operands like any other value: `1 + if x > 0 { 1 } else { 2 }`, `10 + match x { 0 => 1, _ => 2 }`, `f(if c { a } else { b })`, `[match x { 1 => 7, _ => 8 }]`. An operator after one applies to its whole value: `match x { 1 => 3, _ => 4 } * 10`. The keywords end at a word boundary (`iffy` and `matches` are names). At the start of a statement, `if` is the `if` statement.

### Match guards

`match x { n if n > 0 => n, _ => 0 }` — patterns may include `if` guards evaluated after the pattern binds.

### Literal patterns

`match n { 0 => "zero", -1 => "minus one", _ => "other" }`, `match s { "y" => true, _ => false }`, and `true =>` / `false =>` compare the scrutinee with `==`. Allowed literals are Int (optionally negative), String without `{…}` interpolation, and Bool. A Float literal pattern is an error, because exact float equality is a trap. The literal must have the scrutinee's type. Literal arms never make a match exhaustive, so a `_` or binding arm is required.

### Nested patterns

A variant's payload may itself be a pattern: `Some(Num(n))`, `Some(LParen)`, `Ok(0)`, `Some(Some(_))`, to any depth. A name inside is a payload-free variant if one with that name exists (including `None`), and otherwise a binding. Bindings from every level are visible in the guard and the arm. Nested arms count toward exhaustiveness when, together, they cover the payload's type: `Some(A)`, `Some(B)`, `None` is exhaustive for `Option[AB]` with `enum AB { A, B }`. A nested arm with a guard does not count, just like a top-level one. `None` in a pattern is always `Option`'s variant, never a binding.

```flux
match get(toks, i) {
    Some(Num(n)) => n,
    Some(LParen) => parse_group(),
    Some(_) => fail(),
    None => end()
}
```

### Block match arms

An arm may be a block: `Err(e) => { println(e)\n  0 }`. Its statements run in order, after the pattern binds, and the block's **final expression** is the arm's value. A block that ends in a statement instead (an assignment, `if`, a loop; a `match` used for its effects) has the value `()`. Mixing such an arm with arms that have a value is a type mismatch. Allowed everywhere: `let`, assignment to a `let mut` local, and expression statements.

Inside a function, an arm block may also contain `if`/`while`/`for` and `return`/`break`/`continue`. They act on the enclosing function or loop, as they would outside the `match`. An arm may also *be* one of these (`Err(e) => return Err(e)`), or a block may end with one instead of a value. Such an arm leaves and has no value: it does not take part in the `match`'s type, and a `match` whose arms all leave has type `Unit`. A `return` in an arm is checked against the function's declared return type. Derived values, `watch`, and `on` keep expression-only arms (control flow there is a compile error), and `emit` or assignments to module bindings or immutable locals are compile errors in any arm block.

```flux
fn add(a: String, b: String) -> Result[Int, String] {
    let x = match parse_int(a) {
        Ok(v) => v,
        Err(e) => return Err(e)
    }
    let y = match parse_int(b) {
        Ok(v) => v,
        Err(e) => return Err(e)
    }
    Ok(x + y)
}
```

Keywords end at a word boundary: `returned`, `breaker`, and `continued` are ordinary names.

### The `?` operator

`expr?` unwraps an expected failure or passes it on. In a function declared `-> Result[T, E]`, `expr` must be a `Result[U, E]`: `Ok(v)?` is `v`, and `Err(e)?` returns `Err(e)` from the function. In a function declared `-> Option[T]`, `expr` must be an `Option[U]`: `Some(v)?` is `v`, and `None?` returns `None`. The error type must be exactly the function's `E`, since `?` performs no conversion. Any other use is **E021**: in a function without a declared `Result`/`Option` return type, on a value of the other kind, with a different error type, or outside a function (derived values, `watch`, `on`, module bindings). `?` binds tighter than every operator: `parse_int(a)? + 1`, `get_r(x)?.field`.

```flux
fn add(a: String, b: String) -> Result[Int, String] {
    Ok(parse_int(a)? + parse_int(b)?)
}
```

### String interpolation

`{…}` inside a string literal holds any expression: `"Hello, {name}!"`, `"x={p.x}"`, `"sum={a + b}"`, `"{label(7)}"`, even a nested string `"{"<{n}>"}"`. The value is shown as `to_string` (and `print`) would show it: a string's own text without quotes, and any other value in its display form (`[1, 2]`, `Some(3)`, `P { x: 1 }`, `{"a": 1}`). A function has no text form, so interpolating one is a type error (**E005**). `\{` and `\}` write literal braces, and an empty `{}` is a parse error. No import is needed.

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
| `collections` | `len`, `range`, `push`, `set`, `pop`, `get`, `map`, `filter`, `reduce`, `map_new`, `map_has`, `map_remove`, `map_keys`, `map_values` |
| `math` | `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow`, `assert`, `assert_eq` |
| `testing` | `assert`, `assert_eq` (the same functions as in `math`) |
| `str` | `str_len`, `str_split`, `str_chars`, `str_join`, `str_contains`, `str_trim`, `str_starts_with`, `str_ends_with`, `str_replace`, `str_to_lower`, `str_to_upper`, `to_string`, `parse_int`, `parse_float` |
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
| `set` | `fn([T], Int, T) -> [T]` | Replace element `i` (returns new array); `i` outside `0..len` is the runtime error `index out of bounds` |
| `pop` | `fn([T]) -> [T]` | Remove last element |
| `get` | `fn([T], Int) -> Option[T]` | `Some(xs[i])` when `0 <= i < len(xs)`, otherwise `None` (never an error); `i` may be any integer type |
| `map` | `fn([T], fn(T) -> U) -> [U]` | Transform each element |
| `filter` | `fn([T], fn(T) -> Bool) -> [T]` | Keep elements matching predicate |
| `reduce` | `fn([T], U, fn(U, T) -> U) -> U` | Fold to a single accumulator |
| `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow` | math | Numeric intrinsics (`Int`/`Float` overloads where applicable). Integer arguments of `min`/`max`/`clamp` promote like arithmetic operands; a value that does not fit the promoted width (`min(-1, 0u64)`) is `integer overflow` |
| `str_len` | `fn(String) -> Int` | Length in Unicode scalar values |
| `str_split` | `fn(String, String) -> [String]` | Split on separator (an empty separator also yields `""` at both ends) |
| `str_chars` | `fn(String) -> [String]` | One string per character (Unicode scalar value) |
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

### Limits

These limits keep extreme programs a language error rather than a native stack overflow (a process abort). They are the same on the VM and the JIT.

| Limit | Value | Past it |
|-------|-------|---------|
| Nested function calls (recursion depth) | 10,000 active calls | runtime error `call depth limit exceeded (10000 nested calls)` (E018) |
| Bracket nesting (`(`, `[`, `{`, string `{…}`) | 256 levels | parse error (E000) |
| Consecutive unary `-`/`!` | 256 | parse error (E000) |
| Syntax tree depth (e.g. one operator chain `1 + 1 + …`) | 1,000 levels | parse error (E000); split it with `let` |
| Collection length (`range`, `push`, …) | 10,000,000 elements | runtime error `allocation limit exceeded` |
| String built by `+` or interpolation | 100,000,000 bytes | runtime error `allocation limit exceeded` |

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
| E018 | Runtime error (overflow, division by zero, failed assertion, …) |
| E019 | No method with that name for the receiver's type |
| E020 | `impl` does not match its trait (unknown trait, missing, extra, or mismatched method) |
| E021 | Invalid `?` (see §5, "The `?` operator") |
| E022 | Importing a name the module declares without `pub` |

A UTF-8 byte-order mark at the start of a file is ignored, like whitespace.

## 8. Tooling

| Command | Description |
|---------|-------------|
| `flux run [--jit] [--verbose] file.fl` | Compile and execute |
| `flux repl [--jit]` | Interactive shell |
| `flux check file.fl` | Type-check only |
| `flux test [filter] [--jit]` | Run every `fn test_*()` in the project (see below) |
| `flux lock [--check]` | Create or update `flux.lock` for path dependencies; `--check` only reports whether it is up to date (exit 1 if not) |
| `--locked` (on `run`, `check`, `test`) | Require `flux.lock` to match the dependency tree; never write it (§9) |
| `flux fmt file.fl [--write] [--check]` | Format source (`--check` fails if reformat needed) |
| `flux migrate [--write] path` | Migrate v1 top-level bindings to v2 `let` / `let mut` |
| `flux init [path] [--lib]` | Create a project: an app (`main.fl`) or, with `--lib`, a library (`lib.fl` + `lib_test.fl`). Refuses a directory that already has `flux.toml` and never overwrites a source file |
| `flux lsp file.fl` | JSON diagnostics |
| `flux lsp-server` | Stdio LSP: diagnostics, hover, completion, go-to-def, format, document symbols, rename |
| `flux wasm file.fl` | Experimental stub: always fails, writes nothing |
| `flux repl` | Interactive shell (v2 syntax help) |

**`flux test`.** Every `.fl` file under the project root (the nearest `flux.toml`, or the current directory) that defines `fn test_*()` is compiled as its own program. Hidden directories and `target/` are skipped. Each test function takes no parameters and runs in a fresh executor after module initialization; the file's `main` is not run. A test passes if it returns `Unit` or `true`. It fails if it returns `false`, raises any runtime error (including a failed `assert(cond)` or `assert_eq(a, b)`, which report `assertion failed[: a != b]`), or if its file does not compile. Each test is reported as `test <file>::<name> ... ok | FAILED: <reason>`, then a `test result` summary line; the exit status is 1 if anything failed. `filter` keeps tests whose `file::name` contains it. With no tests, it prints `no tests found` and exits 0.

## 9. Package manifest (`flux.toml`)

```toml
[package]
name = "my_app"
version = "0.1.0"
entry = "main.fl"

[run]
jit = false   # optional; set true to use the experimental JIT for this project
```

Dependencies are local path packages, each a directory with its own `flux.toml`:

```toml
[dependencies]
geometry = { path = "../geometry" }   # then: import geometry { area }
```

**`flux.lock`.** For a project that declares dependencies, `flux check`, `run`, and `test` keep a `flux.lock` next to `flux.toml`. It lists every package in the transitive path-dependency tree, sorted by path, with its `[package] name`, its directory relative to the project, and a `sha256` over its `flux.toml` and all of its `.fl` files (hidden directories and `target/` excluded). Commit it. Without `--locked`, a stale lock is rewritten. With `--locked`, the lock is never written, and a missing or different lock is an error naming each `added`/`changed`/`removed` package; nothing runs. A project without dependencies gets no lock. Editors (LSP) and the REPL never write it.

A registry/version dependency (`name = "1.2.0"`) is an error, because no registry exists yet. So are a path that is not a directory and invalid TOML; all three are reported against the offending `flux.toml` line.

## 10. Execution

Pipeline: **parse → resolve imports → HIR → monomorphize → typecheck → IR → VM/JIT**.

- Default: stack VM with reactive graph
- **Entry point.** If the root module defines `fn main()` or `fn main() -> Int` (no parameters; any other signature is a compile error), `flux run` calls it after module initialization. The program then owns stdout: no banner and no binding dump are printed. The exit status is `main`'s `Int` result (it must be 0..=255; another value is an error with status 1), or 0 for a `Unit` `main`. A runtime error in `main` is reported on stderr with status 1. Without `main`, `flux run` prints the module's reactive bindings as before.
- `const` bindings are compile-time constants: a pure `fn` (including `main`) may read them. Reading `let` / `let mut` module bindings from a pure `fn` is still a reactive read (E002).
- JIT opt-in: `--jit`, environment `FLUX_JIT=1`, or `[run] jit = true` in `flux.toml`
- Cranelift JIT compiles numeric derived nodes, pure functions (including loops, early `return`, recursion, and calls to functions defined earlier), and eligible `watch`/`on` bodies with tag-aware int/float arithmetic. `LazyIf` branch deps are collected statically. `match` on integers and on user enums (tuple payloads) is compiled. Strings, struct-variant fields, `break`/`continue`, and other ineligible IR fall back to the VM. So does any call whose inputs include a struct, `Option`, or `Result`, which have no tagged form
- `flux run --jit --verbose` prints JIT compile/fallback statistics
- WASM backend: stub module header (future: full codegen)
