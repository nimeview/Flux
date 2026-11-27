# Flux Language Specification

Flux is a statically typed language with **built-in reactivity**: module-level bindings form a dataflow graph, and `watch` / `on` / `track fn` integrate declarative updates with imperative code.

## 1. Lexical structure

- **Comments**: `//` to end of line
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`
- **Literals**: integers, floats, booleans (`true`/`false`), double-quoted strings
- **Numbers**: integers are decimal (`1000`, `1_000`) or hexadecimal (`0xff`, `0xFF_FF`). Floats need a fraction, an exponent, or both (`1.5`, `1e3`, `2.5e-3`), and `1e3` is a Float. `_` may separate digits, one at a time. A type suffix may follow (`255u8`, `0x10u8`, `1e3f32`). A value out of range for its type is a parse error, including a float too large to represent, and so is a letter, digit, or `_` right after a number (`1e`, `12abc`). `flux fmt` keeps the spelling.
- **Raw strings**: `r"..."` holds its text exactly as written, with no escapes and no `{...}` interpolation, across lines too. To include `"`, add `#` signs: `r#"say "hi""#` ends at `"#`, and `r##"..."##` at `"##`.
- **Separators**: a newline ends a statement where the expression cannot continue. `;` may also separate items or statements on one line; it is only a separator, so a final `;` does not change a block's value. A comma may follow the last element of any list: arguments, parameters, array and struct literals, struct fields, enum variants, match arms, imports, and type arguments.
- **String escapes**: `\n` (newline), `\t`, `\r`, `\0`, `\\`, `\"`, and `\{` / `\}` for literal braces (a bare `{name}` interpolates). Any other `\x` is a parse error.

## 2. Types

| Type | Description |
|------|-------------|
| `Int` | Primary integer type (alias for `Int64`; integer literals infer as `Int64`) |
| `Int8`, `Int16`, `Int32`, `Int64`, `UInt8`, `UInt16`, `UInt32`, `UInt64` | Fixed-width integers |
| `Float`, `Float32`, `Float64` | Floating point (`Float` is an alias for `Float64`) |
| `Bool`, `String`, `Unit` | Primitives |
| `Bytes` | Raw bytes, one byte per element |
| `[T]` | Homogeneous array |
| `Option[T]` | Optional value (`Some`, `None`) |
| `Result[T, E]` | Success or error (`Ok`, `Err`) |
| `(A, B, ...)` | Tuple of two or more values |
| `struct Name { f: T, pub g: U, ... }` | Product type; fields are private unless marked `pub` before the field name; `struct Name[T, ...] { ... }` is generic |
| `enum Name { A, B(T), C { f: T }, ... }` | Sum type; a variant has no payload, one value, or named fields; `enum Name[T, ...] { ... }` is generic |
| `fn(T, ...) -> U` | Function |
| `Cell<T>` / `Read<T>` | Reactive wrapper (internal; sources/derived) |

Generics: `fn id[T](x: T) -> T` — monomorphized at compile time per call site. A generic call's result can be the argument of another (`id(id(3))`). A declaration names each type parameter once (**E004**).

**Inference from the expected type.** A generic function's type arguments come from its arguments. Any still unknown come from the type the call's place expects:

- An annotated binding: `let xs: [Int] = empty()`.
- A function's declared return type, for a tail expression or an explicit `return`.
- A parameter of known type: `count(empty())`, where `fn count(xs: [Int])` takes an array of integers.
- A struct literal field: `Cart { names: empty() }`, where `names: [String]`.
- An assignment to a variable of known type: `list = empty()`.
- The branches of an `if` or `match` whose type is known: `let o: Option[Int] = if flag { none() } else { Some(3) }`.

This also applies to type functions of generic types: `let b: Bag[Int] = Bag::new()`. Flux has no explicit type arguments at call sites; `f::[T]()` is not supported.

If nothing fixes a type argument, as in `let c = empty()`, compilation fails with a type mismatch: expected "type arguments for `empty` from its arguments or from where the call is", found "type arguments that nothing fixes". The help is "give the call's place a type, as in `let xs: [Int] = empty()`, or pass arguments of known types".

**Generic structs and enums.** `struct Pair[T] { a: T, b: T }` and `enum Tree[T] { Leaf, Node { v: T, l: Tree[T], r: Tree[T] } }` take type parameters, which their fields and payloads may use at any depth. A generic type is written with all its arguments, `Pair[Int]`; `Pair` alone or `Pair[Int, Int]` is **E005**. Type arguments are never written when a value is built: they come from the values (`Pair { a: 1, b: 2 }` is a `Pair[Int]`) and, for what the values leave unknown, from context, like `None`'s: an annotation (`let t: Tree[Int] = Leaf`), a parameter, a return type, another `if`/`match` branch, or a later array element (`[Leaf, Node(2)]`). Values must agree (`Pair { a: 1, b: "s" }` is **E005**), and `Tree[Int]` and `Tree[String]` are different types. A binding whose type still has an unknown argument (`let t = Leaf`) is **E005**, with a suggested annotation. Patterns, `with`, field reads and assignments, `==`, and display work as for other user types; values display without their arguments (`Pair { a: 1, b: 2 }`). Generic functions take and return generic types (`fn first[T](p: Pair[T]) -> T`). For generic impls, see [Methods](#methods).

**Recursive types.** A struct or enum may refer to itself or to another user type that leads back to it, directly (`enum T { Leaf, Wrap(T) }`), through another type (`enum L { Nil, Cons(Node) }` with `struct Node { value: Int, next: L }`), or through `Option` and arrays (`struct Tree { kids: [Tree] }`). User types are compared by name. A value can be nested as deep as a program builds it; dropping, comparing, and printing it do not overflow the stack. A type that has no finite value, such as `struct S { me: S }`, is **E025**.

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
| `& \| ^` | Bitwise AND, OR, XOR; result takes the wider of the two integer types | **E005** |
| unary `!` | Bitwise NOT | **E005** |
| `<< >>` | Keep the left operand's type; shifted-out bits are dropped; signed `>>` preserves the sign | **E005** |
| `== < …` | Exact (no conversion through `Float`) | IEEE 754 (`NaN != NaN`) |

- An integer and a float compare (`==`, `<`, …) by their exact values: `9007199254740993 == 9007199254740992.0` is `false`, though the integer rounds to that float. `NaN` is unordered with every number, so only `!=` is `true`.
- Mixed integer and float arithmetic converts the integer to the float operand's type: `Float32` with a `Float32` operand (which may round a large integer: `16777217 + 0.0f32` is `16777216.0`), and `Float64` otherwise. Operators on two integers keep the integer type, so `Int / Int` is `Int`: division by zero is a runtime error rather than a value of another type.
- To handle integer overflow or division by zero as a value, use the [checked integer methods](#library-methods): `checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem`, `checked_neg`, and `checked_pow` return `None` instead of stopping with a runtime error.
- A shift by a negative amount or by the left operand's type width or more is the runtime error `integer overflow`. Bits shifted out are dropped rather than causing overflow; signed right shift keeps the sign (`-8 >> 1` is `-4`). The JIT runs code with bit operations on the VM.
- `<`, `<=`, `>`, `>=` also order two strings, by Unicode code point: `"a" < "b"`, and `"Z" < "a"`. A string and a number cannot be compared.
- Casts are checked: an integer out of the target range, a negative value cast to an unsigned type, or a `NaN`/infinite/out-of-range float cast to an integer is the runtime error `numeric cast out of range`. Float → integer casts truncate toward zero.
- A runtime error stops the current evaluation and is reported as `runtime error: <message> in <context>`, where the context names the binding, `watch`, or `on` handler, followed by what was running, innermost first: `  at <name> (<path>:<line>:<column>)`. The location is the failing statement in that frame, or the declaration of a derived binding; a statement directly in a `match` arm reports the `match`, and a module-level `if` has no location. A trace longer than 12 frames shows its 10 innermost and 2 outermost. Both engines report the same text: an error in compiled code replays on the VM, and the rest of the run stays there. Statements after the failing one do not run, and when settling a batch fails, no watch runs. If the error happens during module initialization, `flux run` exits non-zero. Out-of-range integer *literals* are rejected at parse time.
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
pub struct Account { pub owner: String, balance: Int }
enum Color { Red, Green }

trait Display { fn to_string(x: Int) -> String; }
impl Display for Int { fn to_string(self: Int) -> String { "x" } }

event Click
fn add(a: Int, b: Int) -> Int { a + b }
track fn scaled(x: Int) -> Int { x * factor }
act fn reset() { counter = 0 }

const VERSION = 1              // compile-time frozen
let mut counter = 0            // source — reassignable in on/watch
let doubled = counter * 2      // derived — reactive read in RHS
derived total = a + b          // explicit derived sugar (optional)

watch counter { ... }
on Click { counter = counter + 1 }
```

### Binding rules

| Form | Role | Reassign in `on`/`watch` |
|------|------|--------------------------|
| `let mut name = expr` | **Source** (writable) | Yes |
| `let name = expr` + reactive read in RHS | **Derived** (read-only) | No |
| `let name = expr` (no reactive reads) | **Source** (immutable after init) | No |
| `derived name = expr` | **Derived** | No |
| `const NAME = expr` | Compile-time constant | No |

There is no top-level assignment (`x = 0`). Reassignment uses `name = expr` only inside `on`/`watch` for `let mut` sources, or `let mut` locals inside functions.

Inside a function (`fn`, `track fn`, `act fn`):

- `let x = 1` — immutable local
- `let mut x = 1` — mutable local
- `x = expr` — assign to `let mut` local or module source from reactive context

### Modules and visibility

Each `.fl` file is a module with its own namespace. Inside a module, all of its own top-level items are visible, `pub` or not, together with the names its own `import`s bring in (stdlib modules included). It never sees the importing module's names.

- **Exports.** Only `pub` items can be imported. `import m { name }` brings in the listed names; importing a non-`pub` or missing name is an error (**E014**). `import m` (no list) brings in every `pub` name; `import m as a` also makes `a::name` available. `a.name` is the older spelling: it still works with warning E026, which `flux migrate` rewrites.
- **Resolution.** `import a.b` is resolved relative to the importing file's directory, or through the `[dependencies]` of the package that contains the importing file (the nearest `flux.toml` above it). A package can import its own dependencies but not its dependencies' dependencies.
- **One instance.** A module imported along several paths (a diamond) is loaded and initialized once. Cyclic imports are an error (**E013**).
- **Reactivity.** A module can own state: its own `let mut` sources, `derived` values, events, `on` handlers, `watch`es, and top-level `if`s, like the root. Importing a `pub let mut` binding gives the importer the same reactive source, read-only: its derived values, watches, and functions read it and follow its changes, but only the module that declares it changes it, through its own handlers, `act fn`s, and initialization. Assigning another module's source anywhere else (an `on` handler, an `act fn`, a top-level `if`) is **E032**; change it by emitting one of the module's events or calling one of its `pub act fn`s. A `pub event` is used like a function: `emit counter::Inc(1)`, or `emit Inc(1)` after `import counter { Inc }`, and an importer may handle it too, `on counter::Inc(by) { … }`; every handler of an event runs when it is emitted, whichever module declares the handler.
- **Initialization.** Modules initialize one after another: each after the modules it imports (in import order, each once), the root last. A module's initialization assigns its sources and runs its top-level `if`s, then its derived values recompute and its watches run for the first time; a watch of an earlier module runs again only if what it follows changed. `main` runs after the root's watches.
- **Types.** Type names (`struct`, `enum`, `trait`) belong to their module, like functions: two modules may each declare `Point`, and they are different types. Another module's type is used through an import, `import geo { Point }` or `import geo`, or as `geo::Point` (also in annotations such as `Option[geo::Point]`, in `impl geo::Area for Local`, and as `geo::Point::new(…)`), and only if it is `pub`; a type that is not imported is unknown (**E007**), and `geo::Hidden` for a private one is **E022**. Importing an enum makes its variants usable bare as well (`Circle(1.0)` beside `Shape::Circle(1.0)`), unless the importing file declares a variant of that name; `geo::Shape::Circle` always works. A value prints with its type's declared name (`Point { x: 1 }`), whichever module it comes from. Names starting with `__mod_` are reserved for the compiler's internal names (**E000**).
- **Fields.** A struct field is private to its module unless marked `pub`: `pub struct Account { pub owner: String, balance: Int }`. Outside that module (another file), only `pub` fields may be named in field reads (`a.owner`), field writes (`a.owner = …`), `with { … }`, destructuring `let Account { … } = a` patterns, and struct literals. A struct literal must list every field, so a type with a private field cannot be built by a literal outside its module; use a function such as `Account::open("ann")` or a method that its module provides. Naming a private field there is **E033**: "field `balance` of `Account` is private to its module", with help: "only `Account`'s module can name it; use the functions or methods that module provides, or mark the field `pub` there". Inside the module, including its functions, methods, and `impl` blocks in that file, every field is visible. A method may share a field's name (`fn balance(self) -> Int { self.balance }`). The editor's completion after `.` leaves out another module's private fields. Enum variants' named fields (`Rect { w: Float }`) have no `pub` and remain visible wherever the enum is.

## 4. Statements

`let` / `let mut`, assignment, `if`/`else`, `while`, `for x in expr`, `return expr?`, `break`, `continue`, `emit Event(args?)`, expression statements.

**`for`.** `for x in xs` runs the body once per element of an array, in order. Over a map it runs once per entry, in key order, with `x` the tuple `(key, value)`. `for (a, b) in …` takes each element apart as `let (a, b) = element` does.

**`break` and `continue`.** They are allowed only inside a `while` or `for` body in the same function, including inside an `if`, a block, or a `match` arm within that body. A loop outside a closure does not count inside it. Elsewhere, including outside a loop in `act fn main`, they are compile errors (**E000**): "`break` outside a loop: it must be inside a `while` or `for` body", or "`continue` outside a loop: it must be inside a `while` or `for` body".

**Assignment.** `x = v` assigns a `let mut` local, or a source where sources may change. A place may also be a part of one: an element `xs[i] = v` or `m[k] = v`, a struct field `p.x = v`, or a path of them, `todos[i].done = true`, `grid[i][j] = v`, `board.cells[k].owner = p`. The index expressions are evaluated once each, left to right, then `v`; then the part is replaced. The root must be assignable as for `x = v`, and a part of a source is a write to that source. An index out of range, or a map key missing on the way to a deeper part, is a runtime error and changes nothing; `m[k] = v` on its own adds `k`. Arrays, maps, and structs are updated in place when nothing else holds them; a copy someone else holds keeps its old contents.

**Compound assignment.** `x += v`, `-=`, `*=`, `/=`, and `%=` work on any place and mean `place = place op v`, with the operator's types and errors (so `s += "x"` appends to a string, and integer overflow is a runtime error that leaves the place unchanged). The indices are evaluated once, then the old value of the place is read, then `v`.

**Unused results.** A collection function (`push`, `set`, `pop`, `len`, `map`, `filter`, and the rest of the `collections` module) called as a statement whose value nobody uses warns (**E030**): they return a new value and leave their arguments as they were, so `push(xs, 3)` alone does nothing. So does a library method that only computes a value (`xs.sorted()`, `xs.len()`, `m.keys()`), unless the program declares a method or field of that name; the changing methods (`xs.push(3)`) store their result back instead. A block's last statement is checked only where its value is dropped (loop bodies, handlers, watches).

**Block value.** A function's value is its last statement's: an expression, a `return`, or an `if … else` (including `else if` chains), whose value is the final expression of the branch that runs. Both branches must have compatible types, with no numeric widening. A final `match` whose arms all leave with `return` takes the returned values' type, as does an `if … else` whose branches all return (see [Block match arms](#block-match-arms)). Any other last statement (`let`, an assignment, `while`, `for`, an `if` without `else`) gives `Unit`. So `fn main() { let x = 7 }` exits with status 0, and `fn f() -> Int { 1\n let x = 2 }` is a type error.

**Module initialization order.** Bindings with literal initializers have their values from the start. Computed initializers (`let a = f()`, `const C = B + 1`) run in dependency order: after the bindings they read, directly or through any function they call, and otherwise in declaration order. So a binding may use a `const` declared later. Top-level `if` and assignments keep their position. A cycle among initializers is an error.

**Statement boundaries.** A newline does not end an expression that can continue: a binary operator at the end of a line, or a line starting with `+`, continues it. A line that starts with a binary `-` is a compile error rather than a silent subtraction from the line above (`f()` followed by a line `-1` used to mean `f() - 1`). To subtract, end the previous line with `-`; for a negative value on its own line, write `0 - 1` or bind it with `let`. Unary minus elsewhere (`let d = -a`, `2 * -3`) is unaffected.

**Derived expressions** (right-hand side of derived bindings) must be pure: no `snap()`, loops, control flow, or calls to an `act fn` (**E008**), or side-effect builtins such as `print` / I/O (**E017**), including those reached through a function or method it calls, a function it passes to a call, or a function it calls through something holding it: a binding (which holds what its initializer and every assignment give it), a field, a tuple, a local, a closure's captured local, an event's value received by a handler, or a call's result (which holds what the function returns). A struct or tuple counts all its parts. Only holding a function is not running it, so `const out = println` is allowed. A function that reads a value with `snap()`, directly or through a call, cannot run there either (**E008**): the derived value would not follow that value. `snap()` is available in ordinary `fn` bodies for explicit snapshot reads.

## 5. Expressions

Arithmetic (`+ - * / %`), integer bit operations (`&`, `|`, `^`, `<<`, `>>`, unary `!`), comparisons, boolean ops (`&&`, `||`, `!`), `if e { a } else { b }`, `match e { Var => expr, ... }`, calls, field access (`x.f`), closures `|x| x + 1`, `snap(expr)` in reactive contexts.

**Operator precedence.** From lowest to highest: `||`, `&&`, equality (`==`, `!=`), ordering (`<`, `<=`, `>`, `>=`), `|`, `^`, `&`, shifts (`<<`, `>>`), `+` / `-`, `*` / `/` / `%`. Thus `1 + 2 << 3` is `24`, and `6 & 3 == 2` is `true`. Unary `!x` is bitwise NOT for an integer and logical NOT for a `Bool`. Non-integer operands to a bit operation are **E005** ("integer types for a bit operation"). `||`, `&&`, and closures `|x|` keep their meaning. There are no compound bit assignments (`&=`, `|=`, `^=`, `<<=`, `>>=`).

### Closures

`|x| expr` or `|x: T| expr`. Unannotated parameter types are inferred during type checking from the actual array element type for `map`/`filter`/`reduce` callbacks, the initial accumulator value for `reduce`, a called function’s function-typed parameter, or a binding’s function type annotation. An explicit parameter annotation takes precedence. A parameter with no usable context falls back to `Int`. Closures may capture module-level **Source** or **Derived** bindings from their enclosing scope (e.g. `filter(nums, |x| x > threshold)` or `filter(nums, |x| x > doubled)`); capturing **Derived** bindings is supported when the closure is used in a reactive (derived) context. A closure that captures a reactive binding is itself reactive: calls that use it are re-evaluated whenever captured dependencies change.

A closure may also use the locals and parameters of the code around it (`map(xs, |x| x + k)`). They are captured **by value when the closure is created**: a later assignment to the local does not change what the closure sees, and assigning to a captured local inside the closure is **E009**. Module bindings are not copied: they are read when the closure runs, as above.

**Function values.** A function type is written `fn(A, B) -> C`; without `->` the function returns `Unit`. A declared function without `mut` parameters and a closure are values of such a type: they can be passed, returned, and stored in bindings, arrays, and struct fields, and called (`let add2 = adder(2)` then `add2(5)`; `adder(3)(5)`; `(op.f)(3)` for the function in field `f`). `op.f(3)` is a method call; when `op`'s type has no method `f` but a field `f` holding a function, it calls that function with warning E026, and `flux migrate` writes the parentheses. Two function types match when their parameters and results do. Functions have no `==` and no text form. The same holds for any value containing a function, at any depth: an array, `Option`, `Result`, tuple, map, struct, or enum. `==`, `!=`, array `contains`, `to_string`, string interpolation, and `print` / `println` reject such values (**E005**). Calling anything else is **E023**. A closure passed directly to a function takes its unannotated parameter types from that function's declared `fn(...)` parameter. A closure's value is its body's value, so `return` inside a closure is a compile error. A callee is evaluated before its arguments. A derived value depends on what a function value it uses reads, and a function with side effects cannot reach a derived value as a value either (**E017**).

### Paths

`a::b` names something known at compile time: an item of a module imported as `a` (`math::sqrt`, `io::println`), a type function (`Point::new`), or a variant (`Shape::Circle(2)`, also in patterns; `Circle(2)` stays valid). A path has two parts and no spaces around `::`. `.` is for values: fields and methods.

### `mut` parameters

`fn add_tax(mut prices: [Int], pct: Int) { … }` makes `prices` a mutable local starting as the argument; its final value is written back to the caller. The call writes `mut` before that argument: `add_tax(mut list, 10)`. The argument must be a place rooted in a mutable variable (`let mut`): the variable itself, a field (`mut cart.items`), or an element (`mut grid[i]`), including paths of them. Each index in a place is evaluated once, before the call. Several parameters may be `mut`, as in `swap(mut x, mut y)`. They work across modules (`lib::grow(mut a, 3)`) and in generic functions.

A call with `mut` arguments must be a statement or the value of a `let`. A function with `mut` parameters returns a value only if it declares `-> T`; then `let first = take_first(mut list)` binds that value. Without a declared result it returns nothing, even if its last statement is an expression. Such a function cannot be used as a value. Methods cannot have `mut` parameters other than `mut self`.

These are compile errors (**E000**, except an immutable argument's existing **E009**):

- Calling without `mut` before each `mut` parameter's argument: "this function has `mut` parameters: call it on its own line or as a `let` value, with `mut` before each of those arguments".
- Writing `mut` before an argument whose parameter is not `mut`.
- Using the call inside a larger expression: "a call with `mut` arguments must be a statement, or the value of a `let`".
- Binding the value of a function that declares no result.
- Passing a `mut` argument that is not a place, or an immutable variable ("cannot assign to immutable local").
- Using the function as a value: "`grow` has `mut` parameters and cannot be used as a value".

### Methods

`impl Type { … }` gives a type its own functions. A method takes `self` first and is called on a value: `p.len()`. A **type function** takes no `self` and is called by path: `Point::new(1, 2)`. Inside an `impl`, `Self` is the type: in signatures, in `Self { … }`, and in `Self::f(…)`. A generic type's impl has its type's parameters: `impl[T] Pair[T] { fn swap(self) -> Pair[T] { … } }` gives every `Pair[…]` the method, and `impl[T] Pair[T] { fn new(a: T, b: T) -> Self { … } }` the type function `Pair::new`. The impl's parameters are the type's arguments, in order (`impl[K, V] Entry[K, V]`); an impl for one instance (`impl Pair[Int]`) or for a generic type without its parameters (`impl Pair`) is an error (**E020**, **E005**). Each instance a program uses gets its own copy of the methods, like a generic function's. Two methods with one name for one type, across all `impl`s and modules, are an error (**E020**).

A method can also declare type parameters of its own after its name, with optional bounds, like a function. For example, `fn map[U](self, f: fn(T) -> U) -> Box[U]` inside `impl[T] Box[T]`, or `fn scaled[N: Ord](self, by: N, f: fn(Int, N) -> Int) -> Point` inside `impl Point`. These parameters are inferred at each call from the arguments and the expected type. Each distinct instantiation is specialized. A method with its own type parameters cannot take `mut self`; the error is "method `set` has type parameters and cannot be `mut self`" for a method named `set`.

A **`mut self` method** changes the value it is called on and returns nothing: `fn grow(mut self, by: Int) { self.size += by }`. It is called on its own line on a place (a `let mut` local, a source where sources may change, or a field or element of one): `c.grow(2)`, `board.cells[i].grow(1)`. That call means `place = <the changed copy>`: the place's indices are evaluated once, then the arguments, then the method runs on a copy, and only when it returns is the copy stored back, so a method that fails leaves the receiver unchanged. Calling a `mut self` method anywhere else (inside an expression, on a temporary) is a compile error, and so is declaring one with a return type.

**Library types** (`Int` and the other number types, `Bool`, `Char`, `String`) may get methods too: `impl String { fn shout(self) -> String { self + "!" } }`. Such a method is visible in its own file and in files that import that file, nowhere else.

`impl Trait for Type { … }` implements a trait; its methods are called the same way. Dispatch is static: the type checker resolves the receiver's type (a literal, a variable, any expression; primitives, structs, and enums) and compiles a direct call, so several types can have methods with one name. In a trait, `Self` is the implementing type (`fn grow(self, by: Int) -> Self;`). An `impl` must match its trait (**E020**): the trait must be declared, and the impl must define exactly the trait's methods, with the same parameter count, the same types for parameters after the receiver, and the same return type, `Self` standing for the type. A method that no impl provides for the receiver's type is **E019**. `impl[T] Area for Pair[T]` implements a trait for every instance, and `impl[T: Display] Display for Pair[T]` for the instances whose argument has `Display`.

**Trait bounds.** A generic function's or generic impl's type parameter may name traits it needs: `fn largest[T: Ord](xs: [T]) -> T`, `fn f[T: Ord + Display, U](…)`, `impl[T: Display] Display for Pair[T]`. Each call checks them for the type it gives the parameter; a type without one of the traits is **E031**, at the call. A bound names a trait the program declares (or imports), or a library trait: **`Eq`** (values `==` can compare: every type without a function in it), **`Ord`** (the number types and `String`, which `<` and its kin order), or **`Display`** (the library types; a user type has it through `impl Display for T { fn to_string(self) -> String { … } }`, whose `to_string` then replaces the default text for that type's `to_string()` calls; string interpolation still shows the value's structure). A type has a declared trait when it has all its methods. A program's own trait named `Eq`, `Ord`, or `Display` is the one its bounds mean. `Eq` and `Ord` cannot be implemented. Bounds are optional: a generic function may use `<` or call a method on `T` without one, and each specialization is checked as before; a bound moves the error to the call and names the missing trait. A bound naming no trait is **E007**.

### Reactive semantics

| Construct | Context | Reactive reads |
|-----------|---------|----------------|
| `fn` | Ordinary function | Forbidden (use `snap`) |
| `track fn` (older spelling `rfn`, warning E026) | Tracked function | Tracked |
| `act fn` | Action: reads current values, may change sources | Read, not tracked |
| `watch` / `on` | Side-effect blocks | Tracked |
| Module derived | `let y = f(x)` or `derived y = f(x)` | Tracked |
| Closure (captures a source or derived) | Any | Tracked |

**Calling actions.** A derived value, a `const` initializer, or a `track fn` may not run an `act fn` (**E008**). This includes direct calls (`derived y = read()`), calls through a `track fn` or plain `fn`, and calls through a function value (`apply(read)`). An action's reads are not tracked, so a derived value computed through one could go stale even if the action only reads. Write `track fn` instead of `act fn` for a function that computes from sources. An action that writes was already rejected in a derived value as a side effect (**E017**). `act fn main` and `on` handlers may call any `act fn`; an `act fn` may call another.

`watch` and `on` are only valid at module level; using them inside a function body is a compile error (**E011**).

**Who changes sources.** A source changes in an `on` handler, in an `act fn`, or while the module initializes. A `fn`, `track fn`, or `impl` method that assigns a source is an error, **E027** (the fix turns a `fn` into an `act fn`; a method can change state by calling an `act fn`). `main` is no exception: `act fn main` changes state as one batch. A `watch` that assigns a source still runs but warns, **E028** (compute the value with `let`, or `emit` an event whose `on` handler changes it). Each function, handler, or watch reports a source once. A closure counts as the code it is written in.

**Writes and settling.** An assignment takes effect on the source at once. A derived value that depends on a changed source is marked stale, and reading it recomputes it first, so code always reads current values; a stale value nobody reads is not computed until the batch ends, and its errors appear only then. Changes are grouped in **batches**. An `emit` or a call to an `act fn` opens one when none is open; everything it runs joins it, including every handler of the event, nested `emit`s and their handlers, and the `act fn`s they call. When the batch ends, the derived values that are still stale recompute, then the watches that depend on what changed run; what they write, and the events they emit, settle in the same batch. Each watch runs at most once per batch, so a cycle of watches ends: a change that reaches a watch that has already run does not run it again. A derived value counts as changed only when its new value differs from the old one by `==`, so `0.0` and `-0.0` are no change. A watch runs only when what it follows differs from what it last saw (below). A write outside any batch (in a plain `fn` called from `main`, E027) settles at once. If a batch fails, the writes made before the failure stay and nothing else runs.

**Watches.** `watch x { }` follows `x`; `watch { }` follows every reactive value its body names, on any branch. A watch runs once when the program starts and remembers the values it follows. After that it runs only when one of them differs by `==` from what it remembers, compared right before it runs, since an earlier watch may have changed it back: a value that goes `0 -> 1 -> 0` within a batch wakes nothing, and assigning a source its current value is no change. `watch x(prev) { }` binds `prev` to the value of `x` the watch saw last time, of the same type as `x`; on the first run it is the current value. Only a watch with a target takes `prev`. Watches that can wake each other in a cycle, or a watch that can wake itself, warn (**E029**), once per cycle: a watch wakes another when it changes a source the other follows, through its own assignments, the functions, methods, closures, and function values it calls, and the handlers of the events it emits. A watch follows what it names and what the derived values and tracked functions among those read; what an `act fn` reads is not followed.

**Events.** `event Name` or `event Name(a: A, b: B, ...)` declares an event that carries values, in order; two values with the same name are **E004**. `emit Name(x, y)` gives exactly one value per declared value, of its type. `on Name(a, b) { }` names them for the handler's body, one name per value; `on Name { }` names none. The older `on Name { v -> }` names the one value of an event that has one; it warns (**E026**), and the quick fix and `flux migrate` rewrite it to `on Name(v) { }`. Naming values in both forms at once is an error.

**`emit`.** Handlers run at the `emit`, in declaration order, before the next statement. Their writes settle when the batch ends: at the end of the outermost `emit` or `act fn` call. Nested `emit`s count toward the call depth limit, so an unbounded chain is the runtime error `call depth limit exceeded`.

### Components

**Declaration.** `live fn Name(params) { members }` declares a component. It may be `pub live fn`. Members are `let`, `let mut`, `derived`, `event`, `on`, `watch`, and functions (`fn`, `act fn`, `track fn`). Each member may be `pub`. A member names one value; destructuring patterns are not allowed. A member function cannot have type parameters, `mut` parameters, or `mut self`. A component parameter cannot be `mut`: "parameter `x` of a live fn cannot be `mut`: an instance keeps its own copy".

**Instances.** Calling `Name(args)` makes a new instance. Each instance has its own copies of every member: sources, derived values, events, handlers, and watches. Inside the body, members and parameters use their bare names. A parameter is fixed for the instance's life. Reading it is not a reactive read.

**Construction.** Making an instance is an action, like calling an `act fn`. It is allowed in module initialization, `on` handlers, `act fn`s, and functions. A derived value, `const`, or `track fn` that makes one is **E008**. A module-level `let` whose value makes an instance runs once, when the module starts. It reads its inputs then and is not a derived value. For example, `let label = Label(a, "a")` reads the module binding `a` once.

**Types and handles.** The component's name is its instances' type. It can appear in `[Counter]`, `fn total(xs: [Counter]) -> Int`, and `live fn Label(c: Counter, name: String)`. An instance is a handle. Copying it into a binding (`let b = a`), an array, or a field, or passing it, gives the same instance. `==` and `!=` compare identity. Interpolation shows `Counter#3`: the component's name and the instance's number. Numbers count every instance the program made.

**Member access.** Outside the component, code uses only `pub` members. It reads `a.n`, emits `emit a.Inc(1)`, and calls `a.reset()`. `emit` names an instance by a chain of names, as in `emit car.engine.Faster()`. To emit to an element, bind it first: `let item = list.items[i]`, then `emit item.Toggle()`. A non-`pub` member is **E033**: "field `hidden` of `C` is private to its module", with help "mark it `pub` in `live fn C` to use it outside". Assigning a member from outside, as in `a.n = 5` or `xs[0].n += 1`, is **E032**: "`a` belongs to an instance of `C`; only the instance's own code changes it". The help is "emit one of its `pub` events, or call one of its `pub act fn`s". Reading an event or function as a value, as in `let f = c.reset`, is an error.

**Member reads.** A derived value or `track fn` that reads `a.double` follows that instance's member. It updates when the member changes. If the binding holding the instance (`let mut current`) or the array of instances changes, it follows the new instances. Reads through a called function work too: `derived sum = total(all)` follows the `c.double` reads in `fn total(xs: [Counter])`. A plain function may read `a.n` to get its current value. A watch without a target cannot read an instance's member. The help is "give it a name and follow that: `derived n = x.n`, then `watch n { … }`". A named watch on a derived value that reads members works.

**Handlers and watches.** A component's `on` for its own event runs only for the instance it is emitted to. An `on Tick` for an event declared outside runs for every instance. Module handlers run first. Instance handlers follow in instance creation order, then declaration order within each instance. An instance made while an event is being handled does not receive that event. Module watches run first, then instance watches in creation order. A new instance's watches run once at the end of the batch in which it was made, like module watches at startup. `act fn main` is one batch.

**Lifetime.** An instance lives while a value holds it: a variable, a field, an array element, or another instance's member. Global event handlers and watches do not keep it alive. What an instance's `watch` remembers to detect changes does not keep instances alive. A watch with `prev` keeps the values it hands back as `prev`. When its last handle is dropped, its handlers and watches stop no later than the end of the outermost batch. A call that makes an instance and drops the result as a statement warns (**E030**). The help is "an instance lives while something holds it: write `let x = Counter(…)`".

**Holding instances.** A component's `let mut` and `derived` members may hold instances. A list component can have `pub let mut items: [Item] = []` and `pub derived left: Int = items.filter(|i| !i.done).len()`. It can add an item with `on Push(text) { items.push(Item(text)) }`. It can remove one with `items = items.filter(|x| x != gone)`. An instance removed from the list and held nowhere else is dropped and stops reacting.

An instance may never come to hold itself, directly or through the instances it holds. A tree node adopting its own ancestor would do this. Storing such a value in a member fails with the runtime error "an instance cannot hold itself, directly or through the instances it holds". This applies to assignment, `push`, and a derived value's new result. The error is reported where the store happens, such as "at on Adopt". The value is not stored. This includes a component's derived value that would hold a module array containing that same instance.

Recursive components support trees whose children change. A node can have `pub let mut children: [Node] = []` and `pub derived size: Int = 1 + children.map(|c| c.size).reduce(0, |x, y| x + y)`. Such a member needs a type annotation because its type depends on itself. Chains of tens of thousands of instances are computed without exhausting the stack. This also holds when read from the far end inside a batch.

**Cycles and initialization.** A cycle among a component's own members read by their bare names is a compile error (**E010**). A cycle through module bindings or other components is also **E010**, as in `C.d → total → C.d`. Members appear as `C.a` in cycle reports. The same cycle is reported every time. If a value comes to read itself at run time through instances, the runtime error is "a derived value depends on itself". It never returns an old value. During module initialization, an instance's derived value may read a module binding that is not set yet. The runtime error is "a binding was read before its initial value was set".

**Modules and execution.** `pub live fn` is exported and imported like a function: `import counter { Counter, Label }`. The imported name is also the type. Component code runs on the VM. With `--jit`, the results are the same.

### Type aliases

`type UserId = Int` introduces a name synonymous with an existing type for annotations and checking.

### Const bindings

`const N = 42` at module level declares an immutable binding, computed once when the program starts and never changed. Its initializer may call functions, but nothing with a side effect: printing, reading input or files, or a function that does (**E017**). It may not run an `act fn`, directly, through another function, or through a function value (**E008**). Reading a `const` is not a reactive read, so plain functions may use it.

### Indexing

`xs[i]` is the element of array `xs` at integer index `i`, counting from 0: `names[0]`, `grid[1][0]`, `rows[i].field`, `f(x)[0]`. An index outside `0..len(xs)`, negative ones included, is the runtime error `index out of bounds`. Indexing a non-array or using a non-integer index is a type error (**E005**). To read without the possibility of an error, use `get(xs, i)`, which returns an `Option`.

**Element assignment.** `xs[i] = v` and `m[k] = v` mean `xs = set(xs, i, v)`, so the out-of-range and type rules of `set` apply; deeper places are described under **Assignment** in §4.

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

`==` and `!=` (and `assert_eq`) compare two values of the same type when that type is equatable: numbers (any widths, and integers with floats, by exact value; Float by IEEE 754), `Bool`, `Char`, `String`, `Bytes`, `()`, and arrays, `Option`, `Result`, tuples, maps, structs, and enums whose parts are equatable. Compound values are equal when their parts are, by the same rules: `[0.0 / 0.0] == [0.0 / 0.0]` is `false`, and `Some([1]) == Some([1])` is `true`. Comparing functions, values containing a function at any depth, or values of different types is a type error (**E005**). Array `contains` uses the same rule: `[id].contains(id)` is **E005** when `id` is a function.

### `if` and `match` as operands

An `if … else` expression and a `match` are operands like any other value: `1 + if x > 0 { 1 } else { 2 }`, `10 + match x { 0 => 1, _ => 2 }`, `f(if c { a } else { b })`, `[match x { 1 => 7, _ => 8 }]`. An operator after one applies to its whole value: `match x { 1 => 3, _ => 4 } * 10`. The keywords end at a word boundary (`iffy` and `matches` are names). At the start of a statement, `if` is the `if` statement.

### Match guards

`match x { n if n > 0 => n, _ => 0 }` — patterns may include `if` guards evaluated after the pattern binds.

### Literal patterns

`match n { 0 => "zero", -1 => "minus one", _ => "other" }`, `match s { "y" => true, _ => false }`, and `true =>` / `false =>` compare the scrutinee with `==`. Allowed literals are Int (optionally negative), String without `{…}` interpolation, and Bool. A Float literal pattern is an error, because exact float equality is a trap. The literal must have the scrutinee's type. Literal arms do not make a match exhaustive, so a `_` or binding arm is required. The exception is `Bool`: unguarded `true` and `false` arms cover it.

### Tuple patterns

A `match` arm may take a tuple apart: `(0, name) => …`, `(Some(x), _) => …`, `(true, false) => …`. Each position is any pattern (a literal, a variant, a binding, `_`, or a nested tuple), and the arm matches when every position does; a guard runs after all of them. A tuple pattern also fits inside a variant: `Some((a, 0))`. The arms of a `match` must cover every value: positions whose type has finitely many cases (`Bool`, enums, `Option`, `Result`, tuples of them) are checked case by case, so `(true, _)`, `(false, true)`, and `(false, false)` together cover `(Bool, Bool)`; a position of any other type (numbers, strings) is covered only by a binding or `_`.

### Nested patterns

A struct variant `C { f: T }` is built like a struct, `C { f: 1 }` or `E::C { f: 1 }`, with every field exactly once, and matched with `C { f }`, naming only its fields; a struct variant is not matched as `C(x)`, as its fields have no type of their own to bind as one value. Braces build only struct variants. In a pattern they also take apart a variant whose one value is a struct (`D { x }` for `D(Point)`). A bare `C { … }` names the variant only when no struct or other name `C` is in scope; `E::C { … }` always names it. It prints as `E::C { f: 1 }`.

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

An arm may be a block: `Err(e) => { println(e)\n  0 }`. Its statements run in order, after the pattern binds, and the block's **final expression** is the arm's value. Inside a function, a block that ends in `if … else` (including `else if` chains) takes the value of the branch that runs, as a function body does (see **Block value**): each branch's final expression, with statements before it allowed, branches that agree without numeric widening, and a branch that leaves taking no part in the type. A block that ends in another statement (an assignment, an `if` without `else`, a loop; a `match` used for its effects) has the value `()`. Mixing such an arm with arms that have a value is a type mismatch. Allowed everywhere: `let`, assignment to a `let mut` local, and expression statements.

Inside a function, an arm block may also contain `if`/`while`/`for` and `return`/`break`/`continue`. They act on the enclosing function or loop, as they would outside the `match`. An arm may also *be* one of these (`Err(e) => return Err(e)`), or a block may end with one instead of a value. Such an arm leaves and has no value: it does not take part in the `match`'s type. When a `match` is the last statement of a function body or block and every arm leaves with `return`, its type is the returned values' type. The same rule applies to an `if … else` whose branches all return. A `match` whose arms all leave otherwise has type `Unit`. Without a return annotation, the function's return type is inferred from those values. A `return` in an arm is checked against the function's declared return type; returning a `String` from an arm of a `-> Int` function is **E005**. Derived values, `watch`, and `on` keep expression-only arms (control flow there is a compile error), and `emit` or assignments to module bindings are compile errors in their arm blocks. Inside a function, an arm block may also `emit` and assign a source; a derived binding or `const` is rejected as anywhere else. An assignment to an immutable local is a compile error in any arm block.

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

`{…}` inside a string literal holds any expression: `"Hello, {name}!"`, `"x={p.x}"`, `"sum={a + b}"`, `"{label(7)}"`, even a nested string `"{"<{n}>"}"`. The value is shown as `to_string` (and `print`) would show it: a string's own text without quotes, and any other value in its display form (`[1, 2]`, `Some(3)`, `P { x: 1 }`, `{"a": 1}`). A function, or a value containing one at any depth, has no text form. Interpolation, `to_string`, and `print` / `println` reject such values (**E005**). `\{` and `\}` write literal braces, and an empty `{}` is a parse error. No import is needed.

### Destructuring let

`let Point { x, y } = pt` binds struct fields from a value. `let (a, b) = t` binds the values of a tuple, and tuples nest: `let (name, (x, y)) = entry`. `_` binds nothing. The value is evaluated once (inside a function; at module level each name is its own binding).

### Tuples

`(a, b)` is a tuple of two or more values, of the type `(A, B)`; `(x)` is only grouping, and `(x,)` is an error. `t.0`, `t.1`, … read a value, and an index the type does not have is a compile error; `t.0 = v` and `t.0 += v` assign one in a `let mut` tuple. Tuples compare with `==` value by value, display as `(1, "a")`, and may be passed, returned, stored in arrays, captured, and used as generic arguments. They cannot be map keys.

### Record update

`p with { x: 3, y: 4 }` is a copy of the struct `p` with those fields changed and the rest kept: `p` is evaluated once, then the field values in order. Each name must be a field of `p`'s type, at most once, and each value must have the field's type; the result has `p`'s type. `with` binds like a postfix operator: `a + p with { x: 1 }` is `a + (p with { x: 1 })`. It is a keyword only after an expression, so `with` stays usable as a name.

## 6. Standard library

Apart from `Ok`, `Err`, `Some`, and `None`, standard-library symbols must be imported. Import the module you need:

```flux
import io
import collections
import math
import str
import option
import result
```

Selective imports: `import io { println, read_line }`. Stdlib modules are built in and are not listed in `flux.toml` dependencies. A local file with the same name (e.g. `./math.fl`) shadows the stdlib module.

| Module | Symbols |
|--------|---------|
| `io` | `print`, `println`, `eprint`, `eprintln`, `args`, `read_line`, `read_file`, `write_file`, `read_bytes`, `write_bytes`, `append_file`, `file_exists`, `read_stdin` |
| `time` | `now`, `after`, `every`, `cancel`, `stop` (§11) |
| `env` | `env_var`, `current_dir` |
| `fs` | `list_dir`, `is_dir`, `create_dir`, `remove_file`, `remove_dir` |
| `path` | `path_join`, `file_name`, `extension`, `parent` |
| `process` | `exit`, `run` |
| `collections` | `len`, `range`, `bytes`, `push`, `set`, `pop`, `get`, `map`, `filter`, `reduce`, `map_new`, `map_has`, `map_remove`, `map_keys`, `map_values` |
| `math` | `abs`, `min`, `max`, `clamp`, `sqrt`, `floor`, `ceil`, `pow`, `assert`, `assert_eq` |
| `testing` | `assert`, `assert_eq` (the same functions as in `math`) |
| `str` | `str_len`, `str_split`, `str_chars`, `str_join`, `str_contains`, `str_trim`, `str_starts_with`, `str_ends_with`, `str_replace`, `str_to_lower`, `str_to_upper`, `to_string`, `parse_int`, `parse_float` |
| `option` | `Some`, `None` |
| `result` | `Ok`, `Err` |

### Library methods

Values of library types have methods, which need no import: `text.split(",")` is `str_split(text, ",")`. The free functions stay valid and are the same implementation. A method of the value's own type (an `impl`, or an extension the file can see) takes precedence.

| Receiver | Methods |
|---|---|
| `String` | `len`, `split`, `chars`, `contains`, `trim`, `starts_with`, `ends_with`, `replace`, `to_lower`, `to_upper`, `parse_int`, `parse_float`, `to_string`, `to_bytes() -> Bytes`, `split_once(sep) -> Option[(String, String)]`, `lines() -> [String]`, `words() -> [String]` |
| `Bytes` | `len() -> Int`, `slice(start, end) -> Bytes`, `to_text() -> Result[String, String]`; indexing `b[i]` gives `UInt8` |
| `[T]` | `len`, `get`, `map`, `filter`, `reduce`, `join` (for `[String]`), `contains(v) -> Bool`, `find(f) -> Option[T]`, `any(f)`, `all(f)`, `enumerate() -> [(Int, T)]`, `zip(ys) -> [(T, U)]`, `slice(a, b)` (elements `a` up to `b`; out of range is a runtime error), `sorted()`, `sorted_by(key)`, `reversed()`, `first()`, `last()` (`Option[T]`) |
| `Map[K, V]` | `len`, `get`, `has`, `keys`, `values`, `entries() -> [(K, V)]`, `get_or(k, default)` |
| `Option[T]` | `unwrap_or(default)`, `map(f)`, `and_then(f)` (`f` returns an `Option`), `ok_or(err)`, `is_some()`, `is_none()` |
| `Result[T, E]` | `unwrap_or(default)`, `map(f)`, `and_then(f)` (`f` returns a `Result` with error type `E`), `map_err(f)`, `is_ok()`, `is_err()` |
| numbers | `abs`, `sqrt`, `floor`, `ceil`, `pow`, `min`, `max`, `clamp`, `to_string`; integers only: `checked_add(m)`, `checked_sub(m)`, `checked_mul(m)`, `checked_div(m)`, `checked_rem(m)`, `checked_neg()`, `checked_pow(exponent)` |

**Bytes.** `Bytes` holds raw bytes, one byte each; `==` compares their contents, and a value displays as `Bytes[72, 105]`. `b.len()` counts bytes and `b[i]` reads a `UInt8`; an out-of-range index is a runtime error. `b.slice(start, end)` returns `Bytes`. `b.to_text()` decodes UTF-8 into `Result[String, String]`, with the error "not UTF-8 text at byte N". `s.to_bytes()` encodes a `String` as UTF-8 bytes. Construct bytes with `collections`' `bytes(values: [Int]) -> Bytes`; each value must be `0..255`, otherwise the runtime error `numeric cast out of range`. File I/O uses `io`'s `read_bytes(path) -> Result[Bytes, String]` and `write_bytes(path, b) -> Result[Unit, String]`, side effects like `read_file`.

**Checked integer methods.** Every integer type (`Int`, `Int8`…`Int64`, `UInt8`…`UInt64`) has the `checked_` methods above. Each returns `Option[N]`, where `N` is the receiver's integer type: `Some(result)` in that type on success, or `None` when the plain operation would stop with integer overflow or division by zero. An argument may be any integer type, but a value that does not fit the receiver's type gives `None` (`120i8.checked_add(300)`), as does a negative exponent for `checked_pow`. For a signed minimum, `checked_rem(-1)` gives `Some(0)`, like `%`.

A float receiver is **E005** ("`checked_` operations are for integer types"); a float argument is also **E005**. These are methods only, with no free functions or import needed. They are pure and may be used in derived values and `const` initializers. There are no `wrapping_` or `saturating_` forms.

`sorted` and `sorted_by` sort numbers by value and strings by code point (`sorted_by(|p| p.age)` sorts by the key the function gives, keeping equal keys in order); other element types are a compile error. `a + b` joins two arrays of the same type.

**Changing methods.** `xs.push(v)`, `xs.pop()`, `xs.set(i, v)`, `xs.sort()`, `xs.sort_by(key)`, `xs.reverse()`, `m.set(k, v)`, and `m.remove(k)` change the value they are called on, like a `mut self` method: they are called on their own line on a place, return nothing, and store the result back only when they succeed. Calling one inside an expression is a compile error. The free functions `push(xs, v)`, `pop(xs)`, `set(xs, i, v)`, and `map_remove(m, k)` still return a new value and change nothing.

`Ok`, `Err`, `Some`, and `None` need no import. A module that declares the same name itself, as an item or an enum variant, uses its own; so does a local. Importing them from `option` and `result` remains valid.

| Symbol | Type | Behavior |
|--------|------|----------|
| `print` | `fn(String?) -> Unit` | Write to stdout |
| `println` | `fn(String?) -> Unit` | Write line |
| `len` | `fn([T] \| String) -> Int` | Length |
| `range` | `fn(Int) -> [Int]` | `0..n-1` as array |
| `range` | `fn(Int, Int) -> [Int]` | `start..end-1` (step 1) |
| `range` | `fn(Int, Int, Int) -> [Int]` | `start..end-1` with explicit step |
| `bytes` | `fn([Int]) -> Bytes` | Raw bytes; each value must be `0..255`, otherwise the runtime error `numeric cast out of range` |
| `push` | `fn([T], T) -> [T]` | Append element (returns new array) |
| `set` | `fn([T], Int, T) -> [T]` | Replace element `i` (returns new array); `i` outside `0..len` is the runtime error `index out of bounds` |
| `pop` | `fn([T]) -> [T]` | Remove last element |
| `get` | `fn([T], Int) -> Option[T]` | `Some(xs[i])` when `0 <= i < len(xs)`, otherwise `None` (never an error); `i` may be any integer type |
| `map` | `fn([T], fn(T) -> U) -> [U]` | Transform each element |
| `filter` | `fn([T], fn(T) -> Bool) -> [T]` | Keep elements matching predicate |
| `reduce` | `fn([T], U, fn(U, T) -> U) -> U` | Fold to a single accumulator |
| `min`, `max`, `clamp` | math | Integer arguments promote like arithmetic operands and the result keeps their type; a value that does not fit the promoted width (`min(-1, 0u64)`) is `integer overflow` |
| `abs` | math | An integer keeps its type (`abs(-3)` is `3`), and the signed minimum is `integer overflow`; a float gives `Float` |
| `sqrt`, `floor`, `ceil`, `pow` | math | Return `Float`, also for integer arguments (`pow(2, 3)` is `8.0`) |
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
| `read_line` | `fn() -> Result[String, String]` | One line from stdin (no trailing newline); `Err("end of input")` when there are no more lines |
| `read_file` | `fn(String) -> Result[String, String]` | Read UTF-8 file contents |
| `write_file` | `fn(String, String) -> Result[Unit, String]` | Write UTF-8 (create/truncate) |
| `read_bytes` | `fn(String) -> Result[Bytes, String]` | Read raw file bytes; a side effect like `read_file` |
| `write_bytes` | `fn(String, Bytes) -> Result[Unit, String]` | Write raw file bytes; a side effect like `write_file` |
| `append_file` | `fn(String, String) -> Result[Unit, String]` | Append UTF-8 to file |
| `file_exists` | `fn(String) -> Bool` | Whether path exists |
| `read_stdin` | `fn() -> Result[String, String]` | All of stdin, until its end |
| `env_var` | `fn(String) -> Option[String]` | An environment variable, if set |
| `current_dir` | `fn() -> Result[String, String]` | The working directory |
| `list_dir` | `fn(String) -> Result[[String], String]` | Entry names of a directory, sorted |
| `is_dir` | `fn(String) -> Bool` | Whether the path is a directory |
| `create_dir` | `fn(String) -> Result[Unit, String]` | Create a directory and missing parents; an existing one is fine |
| `remove_file` / `remove_dir` | `fn(String) -> Result[Unit, String]` | Remove a file, or an empty directory |
| `path_join` | `fn(String, String) -> String` | `part` inside `base`; an absolute `part` replaces it |
| `file_name` / `extension` / `parent` | `fn(String) -> Option[String]` | Parts of a path; `None` when there is none |
| `exit` | `fn(Int) -> Unit` | Stop the program at once with a status `0..=255` (else a runtime error); nothing after it runs, no watch included |
| `run` | `fn(String, [String]) -> Result[(Int, String, String), String]` | Run a program without a shell, wait, and return its exit code (`-1` if a signal ended it), stdout, and stderr; `Err` if it cannot start |

Expected failures of these functions are `Err` with a message naming the file or program. Except the `path` functions, they are side effects (**E017** in a derived value or `const`).
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
| Generic specializations in a program | 1,024, with types nested at most 32 levels | compile error (E005) |
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
| E008 | Control flow or `snap` in a derived value; an `act fn` or `extern fn` called by a derived value, `const` initializer, or `track fn`, directly or through other calls or function values |
| E009 | Assign to immutable local or const |
| E010 | Cyclic dependency in reactive graph |
| E011 | `watch`/`on` outside module level |
| E012 | Import path outside the project |
| E013 | Cyclic import |
| E014 | Importing a name a module does not have |
| E015 | Warning: unused binding |
| E026 | Warning: an older form that still works; it names its replacement, which the editor's quick fix and `flux migrate` apply |
| E027 | A `fn`, `track fn`, or method changes a source (§ Reactive semantics). For a `fn`, the quick fix and `flux migrate` write `act fn` |
| E028 | Warning: a `watch` changes a source |
| E029 | Warning: watches that wake each other, or one that wakes itself, in a cycle |
| E030 | Warning: a collection function's result is not used (`push(xs, 3)` alone) |
| E031 | A generic call's type argument lacks a trait that its parameter's bound names (§ Methods) |
| E032 | A module changes a source that another module declares (§ Modules and visibility) |
| E033 | Naming a struct field private to another module (§ Modules and visibility) |
| E034 | A module or library function is not available on the target (§11) |
| E016 | `let` without `mut` cannot be reassigned (use `let mut`) |
| E017 | Side effect (`print`, I/O) in a derived expression or a `const` initializer |
| E018 | Runtime error (overflow, division by zero, failed assertion, …) |
| E019 | No method with that name for the receiver's type |
| E020 | `impl` does not match its trait (unknown trait, missing, extra, or mismatched method) |
| E021 | Invalid `?` (see §5, "The `?` operator") |
| E022 | Importing a name the module declares without `pub` |
| E023 | Calling something that is not a function |
| E024 | A type alias that refers to itself |
| E025 | A type with no possible values (`struct S { me: S }`) |

A UTF-8 byte-order mark at the start of a file is ignored, like whitespace.

## 8. Tooling

| Command | Description |
|---------|-------------|
| `flux run [--jit] [--verbose] file.fl` | Compile and execute |
| `flux repl [--jit]` | Interactive shell |
| `flux check [--target cli\|web] file.fl` | Type-check only; the target defaults to `cli` (§11) |
| `flux test [filter] [--jit]` | Run every `fn test_*()` in the project (see below) |
| `flux lock [--check]` | Create or update `flux.lock` for path dependencies; `--check` only reports whether it is up to date (exit 1 if not) |
| `--locked` (on `run`, `check`, `test`) | Require `flux.lock` to match the dependency tree; never write it (§9) |
| `flux fmt file.fl [--write] [--check]` | Format source (`--check` fails if reformat needed) |
| `flux migrate [--write] path` | Experimental: rewrite older forms. It applies every fix the compiler offers, repeatedly, as fixes can uncover others: the replacement of each E026 warning and `act fn` for each E027 warning and the old `let` / `let mut` binding rewrite. It also adds `pub` to every field of every `pub struct` to preserve visibility from before 0.4; other structs and enum variants are left as they are. Use `--write` to apply the changes, then remove `pub` where a field should be private |
| `flux init [path] [--lib]` | Create a project: an app (`main.fl`) or, with `--lib`, a library (`lib.fl` + `lib_test.fl`). Refuses a directory that already has `flux.toml` and never overwrites a source file |
| `flux lsp file.fl` | JSON diagnostics (one-shot; prefer `lsp-server`) |
| `flux lsp-server` | Stdio LSP: diagnostics, hover, completion, go-to-def, format, document symbols, rename |
| `flux wasm file.fl` | Experimental stub: always fails, writes nothing |

**`flux test`.** Every `.fl` file under the project root (the nearest `flux.toml`, or the current directory) that defines `fn test_*()` is compiled as its own program. Hidden directories and `target/` are skipped. Each test function takes no parameters and runs in a fresh executor after module initialization; the file's `main` is not run. Timers do not run. A test passes if it returns `Unit` or `true`. It fails if it returns `false` or any other value, raises any runtime error (including a failed `assert(cond)` or `assert_eq(a, b)`, which report `assertion failed[: a != b]`), or if its file does not compile. Each test is reported as `test <file>::<name> ... ok | FAILED: <reason>`, then a `test result` summary line; the exit status is 1 if anything failed. `filter` keeps tests whose `file::name` contains it. With no tests, it prints `no tests found` and exits 0.

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
- Cranelift JIT compiles numeric derived nodes, pure functions (including loops, early `return`, recursion, and calls to functions defined earlier), and eligible `watch`/`on` bodies with a `while` or `for` loop, using tag-aware int/float arithmetic. A handler or watch whose body has no loop runs on the VM even with `--jit`. `match` on integers and on user enums (tuple payloads), string `+` and ordering, `to_string`, and interpolation are compiled. A derived value whose conditional branch reads a reactive node, directly or through an `rfn`, runs on the VM, which records the dependencies of the branch it took (`dynamic_dependencies`). Structs, struct-variant fields, `break`/`continue`, `emit`, calls through function values, and other ineligible IR fall back to the VM. So does any call whose inputs include a struct, a closure, `Option`, `Result`, or a map, which have no tagged form
- `flux run --jit --verbose` prints JIT compile/fallback statistics
- WASM backend: a stub that always fails; there is no code generation

## 11. Host model

### Time and the event loop

Import the time functions with `import time { now, after, every, cancel, stop }`.

| Function | Behavior |
|----------|----------|
| `now() -> Int` | Milliseconds since 1970-01-01 UTC |
| `after(ms: Int, Event(values…)) -> Int` | Send the event once, `ms` milliseconds later; return the timer's number |
| `every(ms: Int, Event(values…)) -> Int` | Send the event every `ms` milliseconds; return the timer's number |
| `cancel(timer: Int)` | Stop a timer |
| `stop()` | End the event loop; no timer fires after the current batch |

A negative delay counts as 0. The interval for `every` is at least 1 millisecond. Write the event as it is emitted: `after(500, Tick())` or `after(10, Say("first"))`. Anything else is a type error: "an event to send, as in `after(500, Tick())`".

All five functions are side effects. They cannot run in a derived value or `const` (**E017**), as in "side effect `after` is not allowed in a derived expression".

After `main` returns, `flux run` keeps the program running while a timer is set, until `stop()`. Without `main`, the event loop starts after the modules start. Each firing is its own batch. Timers due at the same moment fire in the order they were set. The exit status is still `main`'s. A runtime error in a fired event stops the program with status 1.

A component's timer for one of its own events sends it to that instance. For example, `after(5, Blink())` inside its handler sends `Blink` to that instance. The timer does not keep the instance alive. It is skipped if the instance was dropped.

`flux test` does not run timers.

### Host functions: `extern fn`

`extern fn name(params) -> T` declares a function provided by the host that runs the program. `pub extern fn` exports it. Its types are checked like a function's. A parameter cannot be `mut`.

A call is an action, like an `act fn` call. Calling it in a derived value, `const`, or `track fn` is **E008**.

`flux check` accepts these declarations. `flux run` provides no host functions. A program that declares one does not start there: "this host does not provide `ask`, declared with `extern fn`".

### Embedding in Rust

`flux_runtime::Host::new().function("name", |args| Ok(value))` provides an `extern fn`. Start the program with `flux_driver::compile_file_with_host(path, options, host)` or `flux_runtime::Executor::with_host(program, mode, host)`. Both fail if a declared `extern fn` is missing. A host function returning `Err(message)` causes the runtime error "a host function failed", with "`name`: message".

`executor.event_sender()` gives an `EventSender`. Its `send(event, values)` queues an event from another thread. `executor.run_event_loop()` delivers queued events in the order they came, each as one batch. It also runs due timers. It keeps running while a timer is set or a sender exists, until `stop()`.

`executor.stopped()`, `next_timer_in()`, and `run_due_timers()` let a host drive time itself.

### Targets

`flux check --target cli|web` selects a target. The default is `cli`. Each target has its part of the library.

`web` has no `fs`, `process`, or `env` module. Its `io` module has no `args`, `read_line`, `read_stdin`, `read_file`, `write_file`, `append_file`, `file_exists`, `read_bytes`, or `write_bytes`.

Importing a module the target lacks is **E034**: "`fs` is not available on target `web`". Naming an unavailable function in an import is also **E034**. With the whole module imported, as in `import io`, using such a function is **E034** where it is used.

An unknown target is an error: "unknown target `mars`; the targets are `cli` and `web`". `web` can only be checked. Nothing builds or runs for it yet.
