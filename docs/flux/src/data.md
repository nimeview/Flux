# Data

## Arrays

An array holds values of one type. `xs[i]` reads an element, and an index out of range is a runtime error. `get(xs, i)` returns an `Option` instead, so it cannot fail.

```flux
import collections { get, len, push }

import io { println }

fn main() {
    let mut xs = [10, 20, 30]
    xs = push(xs, 40)
    xs[0] = 11
    println("{xs} {len(xs)} {xs[1]} {get(xs, 9)}")
}
```

```output
[11, 20, 30, 40] 4 20 None
```

Arrays behave as values. Assigning one to another variable gives you an independent copy, as far as you can tell, and Flux only actually copies when both are still in use:

```flux
import io { println }

fn main() {
    let mut a = [1, 2]
    let b = a
    a[0] = 9
    println("{a} {b}")
}
```

```output
[9, 2] [1, 2]
```

## Maps

`Map[K, V]` maps keys to values. Keys are integers, strings, `Bool`, or `Char`. Entries stay in key order, so printing and iterating are predictable.

```flux
import collections { get, len, map_has, map_keys, map_new, map_remove }

import io { println }

fn main() {
    let mut ages: Map[String, Int] = map_new()
    ages["ann"] = 30
    ages["bob"] = 25
    println("{ages} {len(ages)} {map_has(ages, "ann")}")
    println("{ages["bob"]} {get(ages, "zed")}")
    ages = map_remove(ages, "ann")
    println("{map_keys(ages)}")
}
```

```output
{"ann": 30, "bob": 25} 2 true
25 None
["bob"]
```

Reading a missing key with `ages["zed"]` is the runtime error `key not found`.

## Structs

```flux
import io { println }

struct Point {
    x: Int,
    y: Int
}

fn moved(p: Point, dx: Int) -> Point {
    Point { x: p.x + dx, y: p.y }
}

fn main() {
    let a = Point { x: 1, y: 2 }
    let b = moved(a, 10)
    let Point { x, y } = b
    println("{a} {x} {y} {a == b}")
}
```

```output
Point { x: 1, y: 2 } 11 2 false
```

`let Point { x, y } = b` takes the fields apart. `==` compares structs field by field, and works the same way for arrays, `Option`, `Result`, and enums.

## Enums and match

An enum is a value that is one of several variants, each with an optional payload. `match` handles them, and the compiler checks that you covered every variant:

```flux
import io { println }

enum Shape {
    Circle(Float),
    Square(Float),
    Empty
}

fn area(s: Shape) -> Float {
    match s { Circle(r) => 3.0 * r * r, Square(side) => side * side, Empty => 0.0 }
}

fn main() {
    println("{area(Circle(2.0))} {area(Square(3.0))} {area(Empty)}")
}
```

```output
12.0 9.0 0.0
```

Leaving a variant out is a compile error:

```flux,error=E005
enum Light {
    Red,
    Green
}

fn stop(l: Light) -> Bool {
    match l { Red => true }
}

fn main() {
    let _a = stop(Green)
}
```

### Patterns

`match` works on more than enums. A pattern can be a literal, a name that binds the value, or `_`, and it can have an `if` guard. Literals need a catch-all at the end, except that `true` and `false` together cover a `Bool`. Patterns nest as deep as you need:

```flux
import io { println }

enum Token {
    Num(Int),
    Plus
}

fn size(n: Int) -> String {
    match n { 0 => "none", n if n < 0 => "negative", _ => "some" }
}

fn flag(b: Bool) -> Int {
    match b { true => 1, false => 0 }
}

fn describe(t: Option[Token]) -> String {
    match t { Some(Num(n)) => "number {n}", Some(Plus) => "plus", None => "nothing" }
}

fn main() {
    println("{size(0)} {size(-1)} {size(5)} {flag(true)}")
    println("{describe(Some(Num(4)))} {describe(None)}")
}
```

```output
none negative some 1
number 4 nothing
```

### Blocks in arms

An arm can be a block, whose last expression is its value. Separate arms with commas, including after a block. Inside a function, an arm can do whatever the function body can: run a loop, `return`, `emit` an event, or update a reactive source.

```flux
import io { println }

fn check(n: Int) -> Int {
    let doubled = match n {
        0 => return -1,
        _ => {
            println("got {n}")
            n * 2
        }
    }
    doubled + 1
}

fn main() {
    println("{check(0)} {check(4)}")
}
```

```output
got 4
-1 9
```

## Recursive types

A struct or enum can contain itself, as long as a finite value exists:

```flux
import io { println }

enum List {
    Nil,
    Cons(Node)
}

struct Node {
    value: Int,
    next: List
}

fn sum(l: List) -> Int {
    match l { Nil => 0, Cons(n) => n.value + sum(n.next) }
}

fn main() {
    let list = Cons(Node { value: 1, next: Cons(Node { value: 2, next: Nil }) })
    println("{sum(list)}")
}
```

```output
3
```

`struct S { me: S }` has no finite value, so it is rejected.

## Type aliases

```flux
import io { println }

type UserId = Int

fn next(id: UserId) -> UserId {
    id + 1
}

fn main() {
    println("{next(1)}")
}
```

```output
2
```
