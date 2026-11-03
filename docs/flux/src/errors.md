# Handling failure

Flux separates two kinds of failure:

- **Expected failures**, such as a missing file or bad input, are values: `Result` and `Option`. The type tells the caller to deal with them.
- **Bugs and limits**, such as an index out of range or integer overflow, are runtime errors. They stop the program and print a trace.

## Option and Result

`Option[T]` is `Some(value)` or `None`. `Result[T, E]` is `Ok(value)` or `Err(error)`. All four names are available without an import.

```flux
import io { println }

fn half(n: Int) -> Result[Int, String] {
    if n % 2 == 0 {
        Ok(n / 2)
    } else {
        Err("{n} is odd")
    }
}

fn first_even(xs: [Int]) -> Option[Int] {
    for x in xs {
        if x % 2 == 0 {
            return Some(x)
        }
    }
    None
}

fn main() {
    match half(7) { Ok(v) => println("half: {v}"), Err(e) => println("error: {e}") }
    println("{half(8)} {first_even([1, 2])} {first_even([1])}")
}
```

```output
error: 7 is odd
Ok(4) Some(2) None
```

## The ? operator

`?` unwraps a success or returns the failure from the function, which saves writing the same `match` again and again:

```flux
import io { println }

import str { parse_int }

fn add(a: String, b: String) -> Result[Int, String] {
    Ok(parse_int(a)? + parse_int(b)?)
}

fn main() {
    println("{add("2", "3")}")
    println("{add("2", "x")}")
}
```

```output
Ok(5)
Err("invalid integer: `x`")
```

It has strict rules. It works only in a function that declares a `Result` or `Option` return type, on a value of the same kind, and the error type must match exactly, because `?` does not convert errors. Using it anywhere else is a compile error:

```flux,error=E021
import str { parse_int }

fn number(s: String) -> Int {
    parse_int(s)?
}

fn main() {
    let _a = number("1")
}
```

## Defining your own errors

An enum works well as an error type:

```flux,exit=1
import io { println }

import str { parse_int, str_split, str_trim }

import collections { len }

enum ConfigError {
    Malformed(String),
    BadNumber(String)
}

fn parse_port(line: String) -> Result[Int, ConfigError] {
    let parts = str_split(line, "=")
    if len(parts) != 2 {
        return Err(Malformed(line))
    }
    match parse_int(str_trim(parts[1])) { Ok(n) => Ok(n), Err(_) => Err(BadNumber(line)) }
}

fn message(e: ConfigError) -> String {
    match e { Malformed(line) => "no `=` in: {line}", BadNumber(line) => "not a number in: {line}" }
}

fn main() -> Int {
    match parse_port("port = abc") {
        Ok(port) => {
            println("{port}")
            0
        },
        Err(e) => {
            println(message(e))
            1
        }
    }
}
```

The program exits with status 1, printing `not a number in: port = abc`.

## Runtime errors

A runtime error names what failed and the path to it. Execution stops at the failing statement, and nothing after it runs. Runtime errors cannot be caught from inside the language. Check first, or use the variant that returns an `Option` or `Result`, such as `get` instead of `xs[i]`.

```flux,exit=1
fn last(xs: [Int]) -> Int {
    xs[9]
}

fn main() {
    let _a = last([1, 2])
}
```

```stderr
error: runtime error: index out of bounds in `main`
  at last (<dir>/main.fl:2:5)
  at main (<dir>/main.fl:6:5)
```

Long traces are shortened, keeping the innermost 10 frames and the outermost 2.

## Failed assertions

`assert` and `assert_eq` are runtime errors when they fail, which is how tests report failure. See [Tools](./tools.md).
