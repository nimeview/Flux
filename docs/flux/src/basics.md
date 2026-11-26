# The basics

## Values and variables

```flux
import io { println }

fn main() {
    let name = "Flux"
    let mut count = 1
    count = count + 1
    count += 1
    println("{name} has {count} engines")
}
```

```output
Flux has 3 engines
```

Inside a function, `let` declares a variable you cannot reassign, and `let mut` one you can. `count += 1` is short for `count = count + 1`; `-=`, `*=`, `/=`, and `%=` work the same way. Types are inferred, and you can write them when you want to:

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
| `Bytes` | raw bytes, one byte each; see [Bytes](./data.md#bytes) |
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

To handle overflow or division by zero without stopping the program, use a checked integer method. Every integer type (`Int`, `Int8` to `Int64`, and `UInt8` to `UInt64`) has `checked_add(m)`, `checked_sub(m)`, `checked_mul(m)`, `checked_div(m)`, `checked_rem(m)`, `checked_neg()`, and `checked_pow(exponent)`. Each returns `Option[N]`, where `N` is the receiver's type: `Some(result)` in that type, or `None` where the plain operation would stop with a runtime error.

```flux
import io { println }

fn main() {
    let big = 9223372036854775807
    let small = 120 as Int8
    println("{big.checked_add(1)} {small.checked_add(7)} {small.checked_add(8)}")
    println("{7.checked_div(0)} {7.checked_div(2)} {2.checked_pow(62)}")
    println("{big.checked_add(5).unwrap_or(0)}")
}
```

```output
None Some(127) None
None Some(3) Some(4611686018427387904)
0
```

The argument may be any integer type. If its value does not fit the receiver's type, as in `120i8.checked_add(300)`, the result is `None`. A negative exponent for `checked_pow` also gives `None`. For a signed minimum, `checked_rem(-1)` gives `Some(0)`, just as `%` gives `0`.

These methods need no import and are pure, so they work in derived values and `const` initializers too. They have no free-function forms. A float receiver is E005 ("`checked_` operations are for integer types"), and a float argument is E005 too. There are no `wrapping_` or `saturating_` forms.

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

## Bit operations

Integers support bitwise AND (`&`), OR (`|`), XOR (`^`), left shift (`<<`), right shift (`>>`), and bitwise NOT (`!x`). On a `Bool`, `!` still means logical NOT; `||`, `&&`, and closures `|x|` keep their meaning.

```flux
import io { println }

const READ = 1

const WRITE = 2

fn main() {
    let flags = READ | WRITE
    let word = 0x1234
    println("{flags} {flags & WRITE} {word >> 8} {word & 0xff} {!(0 as UInt8)}")
}
```

```output
3 2 18 52 255
```

Precedence follows Rust: from lowest to highest, `|`, then `^`, then `&`, then shifts, then `+` / `-`, then `*` / `/` / `%`. Comparisons apply to the result, so `6 & 3 == 2` is `true`; `1 + 2 << 3` is `24`.

`&`, `|`, and `^` take the wider of the two integer types. Shifts keep the left operand's type and drop bits shifted out. A right shift of a signed value keeps its sign: `-8 >> 1` is `-4`. A negative shift amount, or one at least as large as that type's width, is the runtime error `integer overflow`. Non-integer operands are E005 ("integer types for a bit operation"). There are no compound forms such as `&=`. The JIT runs code with bit operations on the VM.

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

Functions have no text form. Interpolation, `to_string()`, and `println` reject a function or any value containing one, however deeply nested (E005).

A raw string, `r"..."`, keeps its text exactly as written: no escapes and no braces. Add `#` around it when the text has quotes in it, which is handy for JSON:

```flux
import io { println }

fn main() {
    println(r"C:\files\{name}")
    println(r#"{"key": "value"}"#)
}
```

```output
C:\files\{name}
{"key": "value"}
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

Only the very last statement gives a value, and only if it is an expression, a `return`, or an `if` with an `else`. A final `match` whose arms all `return` also takes the returned values' type, as does an `if … else` whose branches all return. Anything else there, such as a `let`, gives `Unit`:

```flux,error=E005
fn five() -> Int {
    let x = 5
}

fn main() {
    let _a = five()
}
```

### Mutable parameters

A parameter marked `mut` starts as the argument's value, can change inside the function, and writes its final value back to the caller. Write `mut` at the call too:

```flux
import io { println }

import collections { range }

fn add_tax(mut prices: [Int], pct: Int) {
    for i in range(prices.len()) {
        prices[i] += prices[i] * pct / 100
    }
}

fn swap(mut a: Int, mut b: Int) {
    let t = a
    a = b
    b = t
}

fn main() {
    let mut list = [100, 200]
    add_tax(mut list, 10)
    let mut x = 1
    let mut y = 2
    swap(mut x, mut y)
    println("{list} {x} {y}")
}
```

```output
[110, 220] 2 1
```

The argument must be a place: a `let mut` variable, or a field or element of one, such as `mut cart.items` or `mut grid[i]`. Each index is evaluated once, before the call. Several arguments can be `mut`, as in `swap` above. These parameters work in generic functions and across modules (`lib::grow(mut a, 3)`).

A call with `mut` arguments must be a statement or the value of a `let`. To return a value as well as changing its arguments, the function must declare `-> T`; then `let first = take_first(mut list)` binds that value. Without a declared result, the function returns nothing and its call cannot be bound to a variable.

Leaving out `mut` at the call is E000: "this function has `mut` parameters: call it on its own line or as a `let` value, with `mut` before each of those arguments". Putting such a call inside a larger expression is also E000: "a call with `mut` arguments must be a statement, or the value of a `let`". So are marking an argument whose parameter is not `mut`, binding a call with no declared result, or passing something that is not a place. An immutable variable gives the existing "cannot assign to immutable local" error (E009).

A function with `mut` parameters cannot be stored or passed as a value: E000 ("`grow` has `mut` parameters and cannot be used as a value"). Methods can only mark `self` as `mut`; see [Methods](./abstraction.md#methods).

## Control flow

`if` is also an expression. `while` and `for` loop. `break` leaves the loop, and `continue` starts its next iteration:

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

`break` and `continue` must be inside a `while` or `for` body in the same function. They can be inside an `if`, a block, or a `match` arm within the loop, but cannot cross a closure boundary. Using either outside a loop, including in `act fn main`, is E000: "`break` outside a loop: it must be inside a `while` or `for` body" (or the same message for `continue`). Move it into a loop or use `return` to leave the function.

Statements are separated by newlines. To put two on one line, separate them with `;`, which has no other meaning. A comma may follow the last element of any list, so `[1, 2, 3,]` is fine; `flux fmt` removes such commas and puts each statement on its own line.

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
