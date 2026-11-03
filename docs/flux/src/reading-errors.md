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

## Things that catch people out

| You wrote | You get | What to do |
|---|---|---|
| `println("x")` with no import | E001, undefined name | `import io { println }` |
| `len(xs)` with no import | E001, undefined name | `import collections { len }` |
| `let limit = 10` at the top, read in `fn` | E002 | `const limit = 10`, a parameter, `rfn`, or `snap` |
| `let c = 0` at the top, assigned in `on` | E016 | `let mut c = 0` |
| `let d = x * 2`, then `d = 1` | E003 | `d` is derived; assign `x` instead |
| `let x = 1` in `fn`, then `x = 2` | E009 | `let mut x = 1` |
| `let f = \|i\| i.price` | E005, field on `Int64` | `\|i: Item\| i.price` |
| `parse_int(s)?` in a function returning `Int` | E021 | return `Result[Int, String]`, or `match` |
| `reduce(floats, 0, \|a, b\| a + b)` | E005 | start from `0.0` |
| `match n { 0 => 1 }` | E005, no catch-all | add `_ => ...` |
| `Ok(1) => { ... }` with no comma before the next arm | E000 | arms take commas, even after a block |
| `"{}"` | E000 | put an expression in, or write `\{\}` |
| A line starting with `-` | E000 | end the previous line with `-` |
| `1e`, `12abc` | E000 | a number cannot run into a name |
| `fn f() -> Int { let x = 1 }` | E005 | end with an expression |
| Printing inside a derived value | E017 | move it to a `watch` |

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
| E008 | control flow in a derived value |
| E009 | assignment to an immutable local or `const` |
| E010 | cycle among reactive values |
| E011 | `watch` or `on` outside the top level |
| E012 | import path outside the project |
| E013 | cyclic import |
| E014 | import of a missing name |
| E015 | warning: unused binding |
| E016 | a top-level `let` that needs `mut` |
| E017 | side effect in a derived value |
| E018 | runtime error |
| E019 | no such method for the type |
| E020 | `impl` does not match its trait |
| E021 | invalid `?` |
| E022 | import of a name that is not `pub` |
| E023 | calling something that is not a function |
| E024 | a type alias that refers to itself |
| E025 | a type with no possible values |
