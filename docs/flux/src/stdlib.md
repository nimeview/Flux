# The standard library

Import the module you need, either by listing names or all at once:

```flux
import io { println }

import math

fn main() {
    println("{max(1, 2)} {abs(-3)} {sqrt(16.0)}")
}
```

```output
2 3 4.0
```

`Ok`, `Err`, `Some`, and `None` need no import. `len`, `range`, and the rest do, unlike in many languages. A file of your own named `math.fl` next to your code takes priority over the standard module.

## Methods

Most of the library is also available as methods, which need no import:

```flux
import io { println }

struct Player {
    name: String,
    score: Int
}

fn main() {
    let words = "pear apple fig".words()
    let mut scores = [30, 10, 20]
    scores.push(40)
    scores.sort()
    let players = [Player { name: "ann", score: 7 }, Player { name: "bo", score: 3 }]
    let best = players.sorted_by(|p| p.score).last()
    let first_big = scores.find(|s| s > 15)
    println("{words.sorted()} {scores} {first_big.unwrap_or(0)}")
    let name = best.map(|p| p.name).unwrap_or("nobody")
    println("best: {name}")
    for (i, w) in words.enumerate() {
        println("{i}: {w.to_upper()}")
    }
}
```

```output
["apple", "fig", "pear"] [10, 20, 30, 40] 20
best: ann
0: PEAR
1: APPLE
2: FIG
```

`scores.push(40)` and `scores.sort()` change `scores` itself, so they go on their own line and `scores` must be `let mut`. The free functions such as `push(xs, v)` stay, and return a new array instead. The full list of methods is in the language reference.

## io

| Function | Returns | Notes |
|---|---|---|
| `print(s)`, `println(s)` | `Unit` | to standard output |
| `eprint(s)`, `eprintln(s)` | `Unit` | to standard error |
| `args()` | `[String]` | arguments after `--` |
| `read_line()` | `Result[String, String]` | one line, without the newline |
| `read_file(path)` | `Result[String, String]` | a UTF-8 file |
| `write_file(path, s)` | `Result[Unit, String]` | create or replace |
| `append_file(path, s)` | `Result[Unit, String]` | add to the end |
| `file_exists(path)` | `Bool` | |

## collections

| Function | Returns | Notes |
|---|---|---|
| `len(x)` | `Int` | array, string, or map |
| `range(n)`, `range(a, b)`, `range(a, b, step)` | `[Int]` | the end is excluded |
| `push(xs, v)` | `[T]` | |
| `pop(xs)` | `[T]` | without the last element |
| `set(xs, i, v)` | `[T]` | also `xs[i] = v` |
| `get(xs, i)` | `Option[T]` | never fails; also for maps |
| `map(xs, f)` | `[U]` | |
| `filter(xs, f)` | `[T]` | |
| `reduce(xs, init, f)` | `U` | `f` returns `init`'s exact type |
| `map_new()` | `Map[K, V]` | needs a type from an annotation or first `set` |
| `map_has(m, k)` | `Bool` | |
| `map_remove(m, k)` | `Map[K, V]` | |
| `map_keys(m)`, `map_values(m)` | arrays | in key order |

These return a new value rather than changing their argument, so write `xs = push(xs, v)`. That form updates the array in place when nothing else holds it, so building an array this way is not quadratic.

## str

| Function | Returns | Notes |
|---|---|---|
| `str_len(s)` | `Int` | Unicode scalar values |
| `str_split(s, sep)` | `[String]` | |
| `str_chars(s)` | `[String]` | one string per character |
| `str_join(xs, sep)` | `String` | |
| `str_trim(s)` | `String` | |
| `str_contains(s, x)`, `str_starts_with(s, x)`, `str_ends_with(s, x)` | `Bool` | |
| `str_replace(s, from, to)` | `String` | |
| `str_to_lower(s)`, `str_to_upper(s)` | `String` | |
| `to_string(v)` | `String` | the same text interpolation gives |
| `parse_int(s)` | `Result[Int, String]` | no whitespace trimming |
| `parse_float(s)` | `Result[Float, String]` | no whitespace trimming |

## math

| Function | Returns | Notes |
|---|---|---|
| `min(a, b)`, `max(a, b)`, `clamp(x, lo, hi)` | the arguments' type | integers stay integers |
| `abs(x)` | the argument's type for an integer, `Float` otherwise | the smallest `Int` overflows, as `-x` does |
| `sqrt(x)`, `floor(x)`, `ceil(x)`, `pow(x, y)` | `Float` | also for integer arguments, so `pow(2, 3)` is `8.0` |
| `assert(cond)`, `assert_eq(a, b)` | `Unit` | a runtime error if they fail; `testing` exports them too |

## Limits

| Limit | Value | Past it |
|---|---|---|
| Nested calls | 10,000 | runtime error |
| Array or map length | 10,000,000 | runtime error |
| A string built with `+` or interpolation | 100,000,000 bytes | runtime error |
| Bracket nesting | 256 levels | compile error |
| One expression's depth, such as `1 + 1 + ...` | 1,000 levels | compile error |
