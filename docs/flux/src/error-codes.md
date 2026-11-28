# Error codes

`flux explain E0xx` prints one section of this chapter. Examples marked `error=` fail to compile. A `fragment` shows a warning or runtime error.

## E000

A parse error means Flux cannot read the source as a program. For `let x = 1 +` it says "expected an expression, found `}`". Other messages say "expected an operator, or `.`, `(`, `[`, or `?`", or "the `(` on line 2 is not closed". A missing arm comma says "expected `,` before this match arm"; a missing parameter or field comma says "expected `,` after this type: parameters and fields are separated by commas". `{}` is not a value: use `Map::new()` with a type annotation for an empty map. For `emit items[i].Ev()`, bind the element first and emit through its name. These are grammar and implementation limits.

```flux,error=E000
fn main() {
    let x = 1 +
}
```

Finish the expression: `let x = 1 + 2`.

## E001

"undefined name `totl`" means no visible declaration has that spelling. Flux may suggest "a name with a similar spelling exists: `total`". For a library name, it gives the import, such as "`map_new` is in the `collections` library: add `import collections { map_new }` at the top of the file", and also suggests `Map::new()`. A name must resolve before the program can run.

```flux,error=E001
fn main() {
    let total = 5
    let doubled = totl * 2
}
```

Write `let doubled = total * 2`.

## E002

"cannot read reactive `count` in a pure function" means a plain `fn` reads changing module state. Pure calls are not tracked as reactive reads. When a module `let` holds an instance, the help says a plain function reads an instance only when given one. Use `track fn`, `act fn` (`act fn main` for `main`), `snap` for an explicit snapshot, or pass the value from an action.

```flux,error=E002
let mut count = 0

fn read() -> Int {
    count
}

fn main() {}
```

For a tracked result, change the declaration to `track fn read() -> Int { count }`.

## E003

"cannot assign to derived binding `double`" means the binding is computed from its inputs. A direct write would put the result at odds with those inputs.

```flux,error=E003
let mut n = 1
derived double = n * 2
event Reset

on Reset {
    double = 0
}

fn main() {}
```

Change the source instead: `n = 0`.

## E004

"duplicate definition of `x`" means one scope declares the same name twice. Each name must identify one declaration there.

```flux,error=E004
let x = 1
let x = 2

fn main() {}
```

Rename or remove one binding, for example `let y = 2`.

## E005

E005 reports a type mismatch or wrong argument count. A call may say "`area` takes 2 arguments but 1 was given", with help "`area` is called as `area(Int, Int)`". Arithmetic on `Option` or `Result` suggests `.unwrap_or(default)`, `match`, or `?`. Text plus a number suggests interpolation, such as `"n = {n}"`. These rules give an operation known input types. Messages use Flux names (`Int`, `Float`, `Option[Int]`, `Result[Int, String]`, `Map[String, Int]`), without internal reactive wrappers.

```flux,error=E005
fn area(w: Int, h: Int) -> Int {
    w * h
}

fn main() {
    let a = area(3)
}
```

Pass both arguments: `let a = area(3, 4)`.

## E006

"unknown event `count`" means `on` or `emit` names a binding that is not an event. The runtime needs an event declaration to find its handlers.

```flux,error=E006
let mut count = 0

on count {
}

fn main() {}
```

Declare `event Tick` at module level, then write `on Tick { }`.

## E007

"unknown type `Piont`" means the type is not declared or imported. Flux may say "a type with a similar name exists: `Point`". The checker needs a known type for an annotation.

```flux,error=E007
struct Point {
    x: Int
}

fn read(p: Piont) -> Int {
    1
}

fn main() {}
```

Write `fn read(p: Point) -> Int`.

## E008

E008 means an untracked read or action reaches a pure result. Messages name the case: "a derived value cannot call the `act fn` `f`", "cannot call the host function `ask`", or "cannot make a `C` instance". Call an action or host function from an `on` handler or `act fn`; make an instance there or in a module-level `let`. A derived value cannot use `snap()` because it would stop following the source. Other messages include "`snap()` cannot be used here", "`const C` reads a value that can change", and "`read` reads with `snap()`, so a derived value cannot call it".

```flux,error=E008
let mut n = 1

act fn read() -> Int {
    n
}

derived d = read()
fn main() {}
```

If `read` only computes from sources, write `track fn read() -> Int { n }`.

## E009

E009 rejects assignment to a fixed name. A local needs `let mut`. A `live fn` parameter is fixed for the instance's life. A closure holds a copy of a captured local, so changing it would lose the write: "a closure cannot change `total`: it holds a copy of it". Return the new value from the closure or use a `for` loop.

```flux,error=E009
fn main() {
    let x = 1
    x = 2
}
```

Write `let mut x = 1`. For a changing component parameter, use a member such as `let mut x_now = x`.

## E010

"cyclic dependency in reactive graph" means a derived value depends on itself through one or more bindings. None of them can be computed first.

```flux,error=E010
derived a = b + 1
derived b = a + 1

fn main() {}
```

Break the cycle with a source: `let mut a = 0`, then keep `derived b = a + 1`.

## E011

"`watch` is only allowed at module level" means a watch or handler was put in a function. These declarations belong to a module so they can be registered once.

```flux,error=E011
fn main() {
    watch {
    }
}
```

Move `watch { }` above `fn main`.

## E012

"import path escapes project root" means a relative import climbs outside the project. Source modules must be inside the project root.

```flux,error=E012
import ../outside as bad

fn main() {}
```

Put the file inside the project and use a path within the root, such as `import inside as good`.

## E013

"cyclic import" means modules depend on each other while loading. Neither can finish before the other.

```flux,file=cycle_a.fl
import cycle_b

pub fn a() -> Int {
    1
}
```

```flux,file=cycle_b.fl
import cycle_a

pub fn b() -> Int {
    2
}
```

```flux,error=E013
import cycle_a

fn main() {}
```

Remove one import, or move a shared declaration to a third module.

## E014

"unknown export `printn` in module `io`" means the module has no public name with that spelling. Help may say "`io` has a name with a similar spelling: `println`", or list its exports. An import list must name actual exports.

```flux,error=E014
import io { printn }

fn main() {}
```

Write `import io { println }`.

## E015

"unused local `x`" is a warning for a `let` whose value is never read. It also covers unused module bindings. This often points to leftover work. Parameters are not reported; names starting with `_` are skipped.

```flux,fragment
fn main() {
    let x = 1
}
```

Remove the binding, or write `let _x = 1` to keep it unused.

## E016

"binding `count` is assigned; declare it with `let mut`" means a module `let` is used as a changing source. A computed `let` that reads reactive state instead needs `derived`, so it updates with its inputs.

```flux,error=E016
let count = 0
event Inc

on Inc {
    count = 1
}

fn main() {}
```

Write `let mut count = 0`.

## E017

"side effect `println` is not allowed in a derived expression" means computing a reactive value would print or perform I/O. Derived values may recompute as dependencies change; repeating an effect then would make evaluation order observable.

```flux,error=E017
import io { println }

derived d = println("read")

fn main() {}
```

Keep the value pure, such as `derived d = "read"`, and print in a `watch` or `on` handler.

## E018

E018 is a runtime error, such as integer division by zero. That operation cannot produce a Flux value. Its trace shows where evaluation failed.

```flux,fragment
fn main() {
    1 / 0
}
```

Check the divisor first, for example `if divisor != 0 { let x = 1 / divisor }`.

## E019

"no method `lenght` for type `[Int]`" means the receiver lacks that method. Help may say "there is a method with a similar name: `len`". For a program struct or enum it suggests an `impl`, such as `impl P { fn triple(self) … }`. For a library type it lists methods: "`String` has: chars, contains, …". Method lookup uses the receiver's type.

```flux,error=E019
fn main() {
    let xs = [1, 2]
    let n = xs.lenght()
}
```

Write `let n = xs.len()`.

## E020

E020 means an `impl` does not match its trait or a type has a conflicting method. Trait dispatch needs the declared method names and signatures.

```flux,error=E020
trait Size {
    fn size(self: Int) -> Int;
}

impl Size for Int {
    fn other(self: Int) -> Int {
        self
    }
}

fn main() {}
```

Rename the method to `fn size(self: Int) -> Int`.

## E021

E021 means `?` cannot pass a failure through the enclosing function. The function must return `Result[T, E]` for a `Result[U, E]` with the same error type, or `Option[T]` for an `Option[U]`.

```flux,error=E021
import str { parse_int }

fn number(s: String) -> Int {
    parse_int(s)?
}

fn main() {}
```

Handle the result with `match`, or change the signature to `fn number(s: String) -> Result[Int, String]`.

## E022

"`secret` in module `hidden` is private" means the module declares it without `pub`. Modules expose only explicit public names.

```flux,file=hidden.fl
fn secret() -> Int {
    1
}
```

```flux,error=E022
import hidden { secret }

fn main() {}
```

In `hidden.fl`, write `pub fn secret() -> Int`, or use a public function.

## E023

"`n` is not a function" means a call's callee has no function type. Only functions and closures can be called.

```flux,error=E023
fn main() {
    let n = 2
    n(3)
}
```

Use the value directly, for example `let answer = n + 3`.

## E024

"type alias `Loop` refers to itself" means the alias never resolves to a type. An alias is only another name; it cannot define a recursive shape.

```flux,error=E024
type Loop = Loop

fn main() {}
```

Use a struct or enum with a stopping case, such as `enum Loop { End, Next(Loop) }`.

## E025

"no value of type `Node` can exist" means every `Node` would have to contain another `Node`, with no base case. Construction could never finish.

```flux,error=E025
struct Node {
    next: Node
}

fn main() {}
```

Use `next: Option[Node]`, or an enum with an end variant.

## E026

E026 warns that an older spelling still works. The warning names its replacement; `flux migrate` or an editor quick fix can apply it. The older form exists for migration.

```flux,fragment
rfn value() -> Int {
    1
}
```

Write `track fn value() -> Int`.

## E027

"`bump` changes `n`, but only `on` handlers and `act fn` change state" means a plain `fn` or `track fn` writes a source. Derived computation may call such a function; a write during recomputation would change state unexpectedly.

```flux,error=E027
let mut n = 0

fn bump() {
    n += 1
}

fn main() {}
```

Write `act fn bump() { n += 1 }`, or move the write to an `on` handler.

## E028

"this watch changes `n`" is a warning. A watch reacts to changes; its write may wake more watches and make their order matter.

```flux,fragment
let mut n = 0

watch n {
    n = 1
}
```

Compute the value with `let`, or `emit` an event whose `on` handler changes `n`.

## E029

E029 warns that watches wake each other, or one wakes itself, in a cycle. Each watch runs at most once per batch. The last change therefore cannot wake a watch that already ran.

```flux,fragment
let mut n = 0

watch n {
    n += 1
}
```

Compute the value with `let`, or move the change to an `on` handler.

## E030

"the result of `push` is not used" is a warning. Free collection functions return a new value, so dropping it loses the change. For `push`, `pop`, `set`, and `map_remove`, help suggests a changing method on a `let mut` value or keeping the result.

```flux,fragment
import collections { push }

fn main() {
    let xs = [1]
    push(xs, 2)
    let _done = 0
}
```

Write `let mut xs = [1]` and `xs.push(2)` on its own line, or keep `let more = push(xs, 2)`.

## E031

E031 says a generic call's type lacks a trait named by a bound. The generic body may use that trait's operations, so the type must provide them.

```flux,error=E031
struct P {
    x: Int
}

fn largest[T: Ord](a: T, b: T) -> T {
    if a > b { a } else { b }
}

fn main() {
    let p = P { x: 1 }
    let q = largest(p, p)
}
```

Pass an `Ord` type, such as `largest(1, 2)`, or implement `Ord` for `P`.

## E032

E032 means code changes state it does not own. An importing module cannot assign another module's source. Code outside a component cannot assign an instance member either. The owner must make the write so its updates settle in the right batch.

```flux,error=E032
live fn Counter() {
    pub let mut n = 0
}

act fn main() {
    let c = Counter()
    c.n = 1
}
```

Give the component `pub event Set(v: Int)` and `on Set(v) { n = v }`, then write `emit c.Set(1)`.

## E033

"field `value` of `Box` is private to its module" means another module names a field without `pub`. Private fields let the defining module control construction and changes.

```flux,file=private_box.fl
pub struct Box {
    value: Int
}

pub fn make() -> Box {
    Box { value: 1 }
}
```

```flux,error=E033
import private_box { make }

fn main() {
    let b = make()
    let x = b.value
}
```

Use a public method that reads the field, or write `pub value: Int` in `private_box.fl`.

## E034

"`fs` is not available on target `web`" means the selected target has no such module or function. Target libraries differ because some host services are unavailable.

```flux,fragment
import fs { list_dir }

fn main() {}
```

`flux check --target web main.fl` reports E034 for this program. Check it for `cli` instead, or leave out `fs` on `web`.

## E035

E035 warns that a local file has a library module's name. `import collections` always means the library, so a nearby `collections.fl` is not imported. The warning exposes the name collision.

```flux,fragment
// In main.fl, beside a local collections.fl:
import collections

fn main() {}
```

Rename the local file and import it by its new name.

## E036

"this `match` does not cover `Shape::Triangle`" means some value has no matching arm. Missing named variants are listed with `or` before the last. The help says "add an arm for `Shape::Triangle`, or `_ => …` for every case not listed". For other types the message says "does not cover every `(Bool, Bool)` value" or "every `Int` value". A match needs a result for every possible input.

```flux,error=E036
enum Shape {
    Circle,
    Triangle
}

fn sides(s: Shape) -> Int {
    match s {
        Shape::Circle => 0
    }
}

fn main() {}
```

Write `Shape::Circle => 0, Shape::Triangle => 3`, or add `_ => 3` after a comma.
