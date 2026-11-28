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

| Receiver | Methods | Use |
|---|---|---|
| `String` | `len`, `chars`, `slice`, `pad_start`, `pad_end`, `split`, `split_once`, `lines`, `words`, `trim`, `contains`, `starts_with`, `ends_with`, `replace`, `to_lower`, `to_upper`, `parse_int`, `parse_float`, `to_bytes`, `to_string` | Count and work with characters or text; `slice(start, end)` excludes `end` |
| `[T]` | `len`, `get`, `first`, `last`, `map`, `filter`, `reduce`, `contains`, `find`, `any`, `all`, `enumerate`, `zip`, `slice`, `sorted`, `sorted_by`, `reversed`; `join` on `[String]`; changing `push`, `pop`, `set`, `sort`, `sort_by`, `reverse` | Read or return a new array, or change a `let mut` array |
| `Map[K, V]` | `len`, `get`, `get_or`, `has`, `keys`, `values`, `entries`; changing `set`, `remove` | Read or change entries; `Map::new()` makes an empty map without an import |
| `Option[T]` | `unwrap_or`, `map`, `and_then`, `ok_or`, `is_some`, `is_none` | Handle a missing value; `ok_or(error)` turns `None` into `Err(error)` |
| `Result[T, E]` | `unwrap_or`, `map`, `and_then`, `map_err`, `is_ok`, `is_err` | Handle success or error; `map_err(f)` changes the error value |
| numbers | `abs`, `sqrt`, `floor`, `ceil`, `pow`, `min`, `max`, `clamp`, `to_string`; integer `checked_*` methods | Compute a value; checked integer methods return `Option` |
| `Bytes` | `len`, `slice`, `to_text` | Count or slice bytes, or decode UTF-8 as a `Result` |

`String.slice(start, end)` counts characters, not bytes. An index outside `0..=len()` or `start > end` is `index out of bounds`. `pad_start` and `pad_end` repeat and cut the fill to reach exactly the requested width in characters. An empty fill or a string already wide enough is unchanged. `sorted_by` and `sort_by` compare tuple keys part by part, including nested tuples; equal keys keep their original order. `sorted()` and `sort()` still sort only numbers and strings. See the [library methods](../../../SPEC.md#library-methods) reference for signatures and changing methods.

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

This example uses text methods, a tuple sort key, an empty map, and the two ways to turn an error into another form:

```flux
import io { println }

fn main() {
    let text = "привет мир"
    let words = [("b", 2), ("a", 2), ("c", 5)]
    let ordered = words.sorted_by(|e| (-e.1, e.0))
    let mut counts: Map[String, Int] = Map::new()
    counts["a"] = 2
    let result: Result[Int, String] = Err("bad")
    let message = result.map_err(|e| "error: {e}")
    let maybe: Option[Int] = None
    let with_error = maybe.ok_or("missing")
    println("{text.slice(0, 6)} {"7".pad_start(3, "0")} {"ab".pad_end(5, "-=")}")
    println("{ordered} {counts}")
    println("{message} {with_error}")
}
```

```output
привет 007 ab-=-
[("c", 5), ("a", 2), ("b", 2)] {"a": 2}
Err("error: bad") Err("missing")
```

Every integer type has `checked_add(m)`, `checked_sub(m)`, `checked_mul(m)`, `checked_div(m)`, `checked_rem(m)`, `checked_neg()`, and `checked_pow(exponent)`. Each returns `Option[N]` for the receiver's integer type `N`: `Some(result)` on success, or `None` for overflow, division by zero, an integer argument that does not fit `N`, or a negative exponent. A signed minimum's `checked_rem(-1)` is `Some(0)`. These are pure methods with no import or free-function forms, and may be used in derived values and `const` initializers. Float receivers and arguments are E005. There are no `wrapping_` or `saturating_` forms. See [Arithmetic](./basics.md#arithmetic) for an example.

## io

| Function | Returns | Notes |
|---|---|---|
| `print(s)`, `println(s)` | `Unit` | to standard output |
| `eprint(s)`, `eprintln(s)` | `Unit` | to standard error |
| `args()` | `[String]` | arguments after `--` |
| `read_line()` | `Result[String, String]` | one line, without the newline; `Err("end of input")` at the end |
| `read_file(path)` | `Result[String, String]` | a UTF-8 file |
| `write_file(path, s)` | `Result[Unit, String]` | create or replace |
| `read_bytes(path)` | `Result[Bytes, String]` | raw file bytes |
| `write_bytes(path, b)` | `Result[Unit, String]` | write raw bytes |
| `append_file(path, s)` | `Result[Unit, String]` | add to the end |
| `file_exists(path)` | `Bool` | |
| `read_stdin()` | `Result[String, String]` | all of standard input |

`read_bytes` and `write_bytes` are side effects, like `read_file`; they cannot run in a derived value or `const` initializer (E017). For byte methods and UTF-8 conversion, see [Bytes](./data.md#bytes).

## env, fs, path, process

For programs that work with files and other programs. A failure that can be expected, such as a missing file, is an `Err` with a message that names the file.

| Function | Returns | Notes |
|---|---|---|
| `env_var(name)` | `Option[String]` | `None` when it is not set |
| `current_dir()` | `Result[String, String]` | |
| `list_dir(path)` | `Result[[String], String]` | entry names, sorted |
| `is_dir(path)` | `Bool` | |
| `create_dir(path)` | `Result[Unit, String]` | with missing parents |
| `remove_file(path)`, `remove_dir(path)` | `Result[Unit, String]` | a directory must be empty |
| `path_join(base, part)` | `String` | |
| `file_name(path)`, `extension(path)`, `parent(path)` | `Option[String]` | |
| `run(program, args)` | `Result[(Int, String, String), String]` | exit code, stdout, stderr; no shell |
| `exit(code)` | | stops the program at once |

`exit` ends the program with a status, from anywhere:

```flux,exit=2
import io { println, eprintln }

import path { file_name, extension }

import process { exit }

fn main() {
    let file = "reports/2026/summary.csv"
    println("{file_name(file)} {extension(file)}")
    if extension(file) != Some("txt") {
        eprintln("expected a .txt file")
        exit(2)
    }
    println("not reached")
}
```

```output
Some("summary.csv") Some("csv")
```

```stderr
expected a .txt file
```

## time

Import with `import time { now, after, every, cancel, stop }`.

| Function | Returns | Notes |
|---|---|---|
| `now()` | `Int` | milliseconds since 1970-01-01 UTC |
| `after(ms: Int, Event(values…))` | `Int` | send the event once, `ms` milliseconds later; return the timer's number |
| `every(ms: Int, Event(values…))` | `Int` | send the event every `ms` milliseconds; return the timer's number |
| `cancel(timer: Int)` | | stop a timer |
| `stop()` | | end the event loop; no timer fires after the current batch |

A negative delay counts as 0. The interval for `every` is at least 1 millisecond. Write the event as it is emitted: `after(500, Tick())` or `after(10, Say("first"))`. Anything else is a type error: "an event to send, as in `after(500, Tick())`".

All five functions are side effects. They cannot run in a derived value or `const` (E017): "side effect `after` is not allowed in a derived expression". See [Timers](./reactive.md#timers) for how the event loop runs them.

## collections

| Function | Returns | Notes |
|---|---|---|
| `len(x)` | `Int` | array, string, or map |
| `range(n)`, `range(a, b)`, `range(a, b, step)` | `[Int]` | the end is excluded |
| `bytes(values: [Int])` | `Bytes` | each value must be `0..255`, else the runtime error `numeric cast out of range` |
| `push(xs, v)` | `[T]` | |
| `pop(xs)` | `[T]` | without the last element |
| `set(xs, i, v)` | `[T]` | also `xs[i] = v` |
| `get(xs, i)` | `Option[T]` | never fails; also for maps |
| `map(xs, f)` | `[U]` | |
| `filter(xs, f)` | `[T]` | |
| `reduce(xs, init, f)` | `U` | `f` returns `init`'s exact type |
| `map_new()` | `Map[K, V]` | still valid; import from `collections`, or use `Map::new()` without an import |
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
