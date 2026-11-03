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

## Traits and methods

A trait names a set of methods. An `impl` provides them for a type, and `value.method()` calls them. Which method runs is decided at compile time from the value's type:

```flux
import io { println }

trait Area {
    fn area(self: Int) -> Float;
}

struct Square {
    side: Float
}

struct Circle {
    radius: Float
}

impl Area for Square {
    fn area(self: Square) -> Float {
        self.side * self.side
    }
}

impl Area for Circle {
    fn area(self: Circle) -> Float {
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

An `impl` must define exactly the trait's methods with matching signatures, apart from the receiver. Traits have no `Self` type, and there is no way yet to write a function that accepts "any type with this trait".
