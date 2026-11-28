# Reading errors

Compile errors have a code, the source line, and usually a hint:

```text
flux::E002

  x cannot read reactive `limit` in a pure function
   ,-[main.fl:4:5]
 3 | fn bad() -> Int {
 4 |     limit
   :     ^^|^^
   :       `-- reactive read
 5 | }
   `----
  help: `limit` is a module `let` (a reactive source); if it is a fixed value, declare it `const limit = …` to read it here, or use `snap(limit)`
```

Errors use `x`; warnings use `!` and do not stop compilation. After an error, Flux prints "For more about this error, run `flux explain E001`", using the first error code with an explanation. `flux explain E001` prints the matching [error-code chapter](./error-codes.md) section; `e1` and `1` work too. An unknown code exits 1.

The checker reports later errors in the same block after a failed statement. A failed `let` still declares its names, so a later use does not get a second "undefined name". An error about a type already marked as an error is not repeated.

## Things that catch people out

| You wrote | You get | What to do |
|---|---|---|
| `println("x")` with no import | E001, undefined name | `import io { println }` |
| `map_new()` with no import | E001 | `import collections { map_new }`, or use `Map::new()` |
| `len(xs)` with no import | E001, undefined name | `xs.len()`, or `import collections { len }` |
| `let limit = 10` at the top, read in `fn` | E002 | `const limit = 10`, a parameter, `track fn`, or `snap` |
| `let c = 0` at the top, assigned in `on` | E016 | `let mut c = 0` |
| `let d = x * 2`, then `d = 1` | E003 | `d` is derived; assign `x` instead |
| `let x = 1` in `fn`, then `x = 2` | E009 | `let mut x = 1` |
| `let f = \|i\| i.price` | E005, field on `Int` | `\|i: Item\| i.price` |
| `parse_int(s)?` in a function returning `Int` | E021 | return `Result[Int, String]`, or `match` |
| `reduce(floats, 0, \|a, b\| a + b)` | E005 | start from `0.0` |
| `Some(1) + 2` | E005 | take the value with `.unwrap_or(default)`, `match`, or `?` |
| `"n = " + n` | E005 | interpolate: `"n = {n}"` |
| `match n { 0 => 1 }` | E036, no catch-all | add `_ => ...` |
| `"{}"` | E000 | put an expression in, or write `\{\}` |
| A line starting with `-` | E000 | end the previous line with `-` |
| `1e`, `12abc` | E000 | a number cannot run into a name |
| `fn f() -> Int { let x = 1 }` | E005 | end with an expression |
| Printing inside a derived value | E017 | move it to a `watch` |

A block `match` arm needs no comma before the next arm. Other arms still take commas.

## All the codes

| Code | Meaning |
|---|---|
| E000 | parse error |
| E001 | undefined name |
| E002 | reactive read in a plain `fn` |
| E003 | assignment to a derived value |
| E004 | duplicate definition |
| E005 | type mismatch |
| E006 | unknown event |
| E007 | unknown type |
| E008 | untracked read or action in a derived value, `const`, or `track fn`: `snap()`, `act fn`, host call, or component construction; also disallowed control flow |
| E009 | assignment to an immutable local, captured local in a closure, `const`, or a `live fn` parameter |
| E010 | cycle among reactive values |
| E011 | `watch` or `on` outside the top level |
| E012 | import path outside the project |
| E013 | cyclic import |
| E014 | import of a missing name |
| E015 | warning: unused binding or local |
| E016 | a top-level `let` that needs `mut` |
| E017 | side effect in a derived value or a `const` |
| E018 | runtime error |
| E019 | no such method for the type |
| E020 | `impl` does not match its trait |
| E021 | invalid `?` |
| E022 | import of a name that is not `pub` |
| E023 | calling something that is not a function |
| E024 | a type alias that refers to itself |
| E025 | a type with no possible values |
| E026 | warning: an older form; the warning says what to write instead, and `flux migrate` can rewrite it |
| E027 | a `fn`, `track fn`, or method changes a source; write `act fn` |
| E028 | warning: a `watch` changes a source; compute the value with `let`, or `emit` an event |
| E029 | warning: watches wake each other, or one wakes itself, in a cycle; the last change in the cycle is lost |
| E030 | warning: a collection function's result is dropped; keep it, or use a changing method such as `xs.push(3)` on a `let mut` value |
| E031 | a generic call's type lacks a trait its bound names, as `largest([p])` for a `Point` without `Ord` |
| E032 | a module changes another module's source; emit that module's event or call its `pub act fn` |
| E033 | naming another module's private struct field; use the functions or methods that module provides, or mark the field `pub` there |
| E034 | a module or library function is not available on the target; for example, "`fs` is not available on target `web`" |
| E035 | warning: a local file is not imported because its name is a library module; rename the file |
| E036 | a `match` does not cover every value; add missing arms or `_ => …` |

E015 also reports an unused local `let` in functions, handlers, watches, and nested blocks: "unused local `x`". The help is "remove it, or name it `_x` to keep it unused". Names starting with `_` are skipped. Parameters are not reported.

Names and types in diagnostics use Flux spelling: `Int`, `Float`, `Option[Int]`, `Result[Int, String]`, and `Map[String, Int]`. E001, E007, E014, and E019 suggest close names where possible. E019 otherwise suggests an `impl` for a user type or lists methods for a library type. E005 reports wrong argument counts in words and shows the expected call. E036 names missing enum variants, or says that a type such as `Int` needs a catch-all arm.
