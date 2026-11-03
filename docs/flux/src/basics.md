# The basics

## Values and variables

```flux
import io { println }

fn main() {
    let name = "Flux"
    let mut count = 1
    count = count + 1
    println("{name} has {count} engines")
}
```

```output
Flux has 2 engines
```

Inside a function, `let` declares a variable you cannot reassign, and `let mut` one you can. Types are inferred, and you can write them when you want to:

```flux
import io { println }

fn main() {
    let ratio: Float = 0.5
    let label: String = "half"
    println("{label} is {ratio}")
}
```

```output
half is 0.5
```

At the top level of a file, `let` means something different: it declares a reactive value. Use `const` for a plain constant there, and see [Reactive programming](./reactive.md) for the rest.

```flux
import io { println }

const LIMIT = 10

fn over(n: Int) -> Bool {
    n > LIMIT
}

fn main() {
    println("{over(3)} {over(30)}")
}
```

```output
false true
```

## Types

| Type | Values |
|---|---|
| `Int` | 64-bit signed integers, the same as `Int64` |
| `Int8` to `Int64`, `UInt8` to `UInt64` | integers of a fixed width |
| `Float` | 64-bit floating point, the same as `Float64`; also `Float32` |
| `Bool` | `true`, `false` |
| `String` | UTF-8 text |
| `Unit` | "no value", written `()` |

Numbers can be written in several ways. A literal with an exponent is always a `Float`:

```flux
import io { println }

fn main() {
    println("{1_000} {0xff} {1.5} {1e3} {2.5e-3} {255 as UInt8}")
}
```

```output
1000 255 1.5 1000.0 0.0025 255
```

## Arithmetic

Integer arithmetic is checked. Overflow and division by zero are runtime errors, not silent wraparound:

```flux,exit=1
import io { println }

fn divide(a: Int, b: Int) -> Int {
    a / b
}

fn main() {
    println("{divide(7, 2)}")
    println("{divide(1, 0)}")
    println("never printed")
}
```

```output
3
```

```stderr
error: runtime error: integer division by zero in `main`
  at divide (<dir>/main.fl:4:5)
  at main (<dir>/main.fl:9:5)
```

The trace reads from the failure outward: which function failed, at which line and column, and who called it.

Mixing an `Int` and a `Float` gives a `Float`. To convert explicitly, use `as`. A conversion that does not fit is a runtime error too:

```flux
import io { println }

fn main() {
    println("{1 + 0.5} {7 as Float} {2.9 as Int} {200 as UInt8}")
}
```

```output
1.5 7.0 2 200
```

## Strings

Any string can include expressions in braces. For a literal brace, write `\{`:

```flux
import io { println }

fn main() {
    let items = 3
    println("{items} items, {items * 2} in total")
    println("\{not interpolated\}")
    println("a" + "b")
    println("{"apple" < "pear"}")
}
```

```output
3 items, 6 in total
{not interpolated}
ab
true
```

`<` and the other comparisons order strings by Unicode code point. That is not alphabetical order: every capital letter sorts before every lowercase one, so `"Z" < "a"`.

## Functions

The value of a function is its last expression, so `return` is only needed to leave early:

```flux
import io { println }

fn sign(n: Int) -> String {
    if n < 0 {
        return "negative"
    }
    if n == 0 {
        "zero"
    } else {
        "positive"
    }
}

fn main() {
    println("{sign(-5)} {sign(0)} {sign(5)}")
}
```

```output
negative zero positive
```

Only the very last statement gives a value, and only if it is an expression, a `return`, or an `if` with an `else`. Anything else there, such as a `let`, gives `Unit`:

```flux,error=E005
fn five() -> Int {
    let x = 5
}

fn main() {
    let _a = five()
}
```

## Control flow

`if` is also an expression. `while` and `for` loop, and `break` and `continue` work as you would expect:

```flux
import collections { range }

import io { println }

fn main() {
    let mut total = 0
    for i in range(10) {
        if i % 2 == 0 {
            continue
        }
        if i > 7 {
            break
        }
        total = total + i
    }
    let size = if total > 10 { "big" } else { "small" }
    println("{total} {size}")
    let mut n = 3
    while n > 0 {
        n = n - 1
    }
    println("{n}")
}
```

```output
16 big
0
```

A line starting with `-` is an error, rather than a subtraction from the line above. End the previous line with the operator if you mean to continue it:

```flux
import io { println }

fn main() {
    let a = 10 - 3
    println("{a}")
}
```

```output
7
```
