# Closures, generics, and traits

## Closures

A closure is written `|parameter| expression`. The standard library's `map`, `filter`, and `reduce` take one:

```flux
import collections { filter, map, reduce }

import io { println }

struct Item {
    name: String,
    price: Int
}

fn main() {
    let items = [Item { name: "pen", price: 3 }, Item { name: "lamp", price: 150 }]
    let names = map(items, |i| i.name)
    let costly = filter(items, |i| i.price > 10)
    let total = reduce(items, 0, |sum, i| sum + i.price)
    println("{names} {len_of(costly)} {total}")
}

fn len_of(xs: [Item]) -> Int {
    reduce(xs, 0, |n, _x| n + 1)
}
```

```output
["pen", "lamp"] 1 153
```

Parameter types come from where the closure is used: the array's element type, `reduce`'s starting value, the type of a function's parameter, or an annotated binding. With no such context, as in `let f = |i| i.price`, a parameter is an `Int`, and the error tells you to write the type: `|i: Item| i.price`.

`reduce`'s closure must return the same type as the starting value, exactly, so summing floats starts from `0.0`, not `0`.

A closure can use local variables around it. It captures them by value when it is created, so a later change to the variable is not seen:

```flux
import io { println }

fn main() {
    let mut k = 1
    let add = |x: Int| x + k
    k = 100
    println("{add(1)} {k}")
}
```

```output
2 100
```

## Function values

Functions and closures are values with types like `fn(Int) -> Int`. They can be passed, returned, and stored:

```flux
import io { println }

fn adder(n: Int) -> fn(Int) -> Int {
    |x| x + n
}

fn twice(f: fn(Int) -> Int, x: Int) -> Int {
    f(f(x))
}

fn main() {
    let add2 = adder(2)
    println("{add2(5)} {twice(add2, 1)} {twice(|x| x * 3, 2)}")
}
```

```output
7 5 18
```

A closure's value is its body's value, so `return` is not allowed inside one.

A function with [`mut` parameters](./basics.md#mutable-parameters) cannot be used as a value; call it directly with `mut` before each of those arguments.

Functions have no equality and no text form. The same holds for any array, `Option`, `Result`, tuple, map, struct, or enum containing a function, at any depth. `==`, `!=`, array `contains`, `to_string()`, interpolation, and `println` reject them (E005). For example, `[id].contains(id)` is an error when `id` is a function. Compare or show the data the function works on instead.

## Generics

Square brackets declare type parameters. Flux generates a specialized copy of the function for each set of types it is used with, so generic code has no runtime cost:

```flux
import io { println }

fn first[T](xs: [T], fallback: T) -> T {
    if xs == [] {
        fallback
    } else {
        xs[0]
    }
}

fn apply[T, U](x: T, f: fn(T) -> U) -> U {
    f(x)
}

fn main() {
    println("{first([1, 2], 0)} {first(["a"], "z")}")
    println("{apply(3, |n| n * 2)} {apply("a", |s| s + "!")}")
}
```

```output
1 a
6 a!
```

Structs and enums take type parameters too. The values decide the type arguments, so you never write them when building a value; where the values leave one open (`Leaf` has no `T`), the type comes from context, such as an annotation or a return type:

```flux
import io { println }

struct Pair[T] {
    a: T,
    b: T
}

enum Tree[T] {
    Leaf,
    Node { v: T, l: Tree[T], r: Tree[T] }
}

fn swap[T](p: Pair[T]) -> Pair[T] {
    Pair { a: p.b, b: p.a }
}

fn sum(t: Tree[Int]) -> Int {
    match t { Leaf => 0, Node { v, l, r } => v + sum(l) + sum(r) }
}

fn main() {
    let p = Pair { a: 1, b: 2 }
    let names = Pair { a: "x", b: "y" }
    let t: Tree[Int] = Node { v: 1, l: Leaf, r: Node { v: 2, l: Leaf, r: Leaf } }
    println("{swap(p)} {names.a} {sum(t)}")
}
```

```output
Pair { a: 2, b: 1 } x 3
```

`Pair[Int]` and `Pair[String]` are different types, and `Pair { a: 1, b: "s" }` is an error. `let t = Leaf` is an error too, since nothing says what `T` is; write `let t: Tree[Int] = Leaf`.

## Methods

`impl Type { }` gives a type its own functions. A method takes `self` and is called on a value; a type function takes no `self` and is called with `::`. Inside the `impl`, `Self` means the type:

```flux
import io { println }

struct Counter {
    count: Int,
    step: Int
}

impl Counter {
    fn new(step: Int) -> Self {
        Self { count: 0, step: step }
    }
    fn tick(mut self) {
        self.count += self.step
    }
    fn report(self) -> String {
        "count {self.count}"
    }
}

fn main() {
    let mut c = Counter::new(5)
    c.tick()
    c.tick()
    println(c.report())
}
```

```output
count 10
```

`mut self` makes a method that changes the value it is called on. Call it on its own line, on a `let mut` variable or a field or element of one (`board.counters[i].tick()`). The method works on a copy, and the copy replaces the value only when the method finishes, so a method that fails changes nothing. It cannot return a value. Methods cannot have other `mut` parameters.

You can add methods to `String`, `Int`, and the other built-in types the same way (`impl String { fn shout(self) -> String { self + "!" } }`). Such a method is visible in its own file and in files that import that file.

A function stored in a field is called with parentheses around the field: `(op.f)(3)`. Without them, `op.f(3)` is a method call.

## Traits and methods

A trait names a set of methods. An `impl` provides them for a type, and `value.method()` calls them. Which method runs is decided at compile time from the value's type:

```flux
import io { println }

trait Area {
    fn area(self) -> Float;
}

struct Square {
    side: Float
}

struct Circle {
    radius: Float
}

impl Area for Square {
    fn area(self) -> Float {
        self.side * self.side
    }
}

impl Area for Circle {
    fn area(self) -> Float {
        3.0 * self.radius * self.radius
    }
}

fn main() {
    let s = Square { side: 2.0 }
    let c = Circle { radius: 1.0 }
    println("{s.area()} {c.area()}")
}
```

```output
4.0 3.0
```

An `impl` must define exactly the trait's methods with matching signatures. In a trait, `Self` is the type that implements it (`fn grow(self, by: Int) -> Self;`).

## Bounds and generic impls

A type parameter can say which traits it needs, so a function accepts "any type with this trait". Besides the traits a program declares, there are three library traits: `Eq` (values `==` can compare), `Ord` (numbers and `String`), and `Display` (library types, and a type with `impl Display`). A generic type gets methods with `impl[T]`, for all its instances at once:

```flux
import io { println }

struct Point {
    x: Int,
    y: Int
}

struct Pair[T] {
    a: T,
    b: T
}

impl Display for Point {
    fn to_string(self) -> String {
        "({self.x}, {self.y})"
    }
}

impl[T] Pair[T] {
    fn swap(self) -> Pair[T] {
        Pair { a: self.b, b: self.a }
    }
}

impl[T: Display] Display for Pair[T] {
    fn to_string(self) -> String {
        "<{self.a.to_string()}, {self.b.to_string()}>"
    }
}

fn largest[T: Ord](xs: [T]) -> T {
    let mut best = xs[0]
    for x in xs {
        if x > best {
            best = x
        }
    }
    best
}

fn show[T: Display](x: T) -> String {
    x.to_string()
}

fn main() {
    let p = Pair { a: Point { x: 1, y: 2 }, b: Point { x: 3, y: 4 } }
    println("{largest([3, 9, 2])} {show(p.swap())} {show(7)}")
}
```

```output
9 <(3, 4), (1, 2)> 7
```

`largest([Point { x: 1, y: 2 }])` is an error that says `Point` does not have `Ord`. Bounds are optional: without one, `T` can still be compared or have methods called, and a mistake shows up inside the function for the type that caused it. String interpolation shows a value's structure; call `to_string()` for the `Display` text.
