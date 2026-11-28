# Flux usability log: six programs written from the book only

Tester profile: knows Rust, TypeScript and Python well, had never used Flux. The only documentation used was the book (`docs/flux/src/*.md`, read cover to cover first, about 2,800 lines) plus the diagnostics the tool printed. Tool: `target/release/flux` (`check`, `run --quiet`, `fmt`, `test`). TypeScript versions run with `node --experimental-strip-types` (Node 22.16).

The failure counts below include only `flux check` or `flux run` calls on the program files that failed. Throwaway probes in a scratch file, used to find out what exists (for example "is there a `substring`?"), are listed separately as probes.

## Summary

| program | finished? | failed checks/runs | Flux lines (non-blank) | TS lines (non-blank) | biggest obstacle |
|---|---|---|---|---|---|
| wordfreq | yes | 2 | 55 (51) | 28 (25) | `sorted_by` takes only a number or string key: no tuple key, no comparator. I had to rely on map key order plus sort stability, which the book does not document |
| ledger | yes | 1 | 134 (123) | 89 (79) | strings have no substring or slice; I worked around it with `s.chars().slice(a, b).join("")` |
| thermostat | yes | 2 | 60 (44) | 38 (33) | `else if` is a parse error inside a top-level `derived`, and the error lists grammar token names |
| todos | yes | 2 | 82 (69) | 46 (41) | a module-level component instance cannot be read from a plain `fn` (E002); both suggested fixes were wrong |
| game | yes | 3 | 91 (70) | 46 (40) | `emit` is rejected inside a `match` arm in an `on` handler, though the book says arms can emit |
| config (2 modules) | yes | 0 | 167 (153): config.fl 110, main.fl 57 | 95 (85): config.ts 50, main.ts 45 | nothing blocked me. Converting error types by hand for `?` (no `map_err`) was the main extra work |
| **total** | 6/6 | **10** | 589 (510) | 342 (303) | |

Flux is about 1.7 times as many lines as TS. Part of that is `flux fmt`, which puts a blank line between top-level declarations and expands every `if/else` with blocks onto 5 lines. The TS was written compactly, so the ratio of non-blank lines (1.68) overstates the real difference in verbosity a little. The real extra lines come from hand-written helpers for things TS has built in: substring, zero-padding, regexes, `map_err`, and comparator sort.

Every Flux program produces the same output as its TS twin. I checked each pair with `diff`; the only difference is the OS error text for a missing file in config.

## Failures per program

### 1. wordfreq (2 failures)

| # | cmd | code | paraphrase | message told me what to do? | book covered it? | what I changed |
|---|---|---|---|---|---|---|
| 1 | check | E001 | undefined name `map_new` | **no**: just "not found", with no hint to import `collections` (the book's table promises such a hint only for `len`) | yes: the Maps example imports it | added `import collections { map_new }`. I had assumed it was a prelude name like `Some`, because methods such as `.words()` need no import |
| 2 | check | E005 | `sorted_by(\|e\| (-e.1, e.0))`: "expected `a key that is a number or a string`, found `(Int64, String)`" | **partly**: it says what is allowed, but not how to sort by two keys | no: `sorted_by` appears in one example, with no rules, no stability guarantee and no comparator form | sorted by `-e.1` only, relying on map key order plus a stable sort, which I confirmed by experiment |

Not counted as a failure: `flux fmt` refused the file with "fmt cannot preserve a comment inside a declaration yet; the file was left unchanged". The cause was an ordinary `//` comment inside `main`. I moved the comment above the function.

### 2. ledger (1 failure)

| # | cmd | code | paraphrase | message told me what to do? | book covered it? | what I changed |
|---|---|---|---|---|---|---|
| 1 | check | E019 (+2 cascading E005) | no method `slice` for `String` | **misleading**: the help says "methods come from `impl Trait for String`; add one that defines `slice`". It is followed by two E005 errors about a type `<error>` on the next `match` | no: the book never says how to take part of a string | probed `substring`, `substr`, `s[1]`, `strip_prefix` and `trim_start_matches`; none exist. Used `raw.chars().slice(1, raw.len()).join("")` |

Extra work, not a failure: `parse_int` returns `Result[Int, String]` and `?` does not convert errors. The book shows no `map_err`, so I wrote a helper `fn digits(s, err) -> Result[Int, LineError]` that wraps a `match`.

### 3. thermostat (2 failures)

| # | cmd | code | paraphrase | message told me what to do? | book covered it? | what I changed |
|---|---|---|---|---|---|---|
| 1 | check | E000 | `derived mode = if a { Heat } else if b { Cool } else { Idle }`: "expected eq_op, rel_op, add_op, bits_op, mul_op, kw_with, index_suffix, try_suffix, field_suffix, call_args, or kw_as", pointing at `else` | **no**: these are internal grammar rule names, and the help is only "check syntax near the highlighted token" | no: the book never shows `else if` at all, and never says derived values restrict it | I guessed wrong: I thought `BAND { Heat }` was being parsed as a struct literal, as in Rust's if-condition ambiguity, and put the conditions in parentheses |
| 2 | check | E000 | the same error, now at the `else` after `( … ) { Heat }` | no | no | a probe showed that `else if` works in a `fn` but not in a top-level `derived`. Rewrote it as `else { if … }` |

Behavior I needed to learn: `watch mode(prev)` runs at start-up with `prev == mode`, so printing only real changes needs an `if prev != mode` guard. The book documents this.

### 4. todos (2 failures)

| # | cmd | code | paraphrase | message told me what to do? | book covered it? | what I changed |
|---|---|---|---|---|---|---|
| 1 | check | E002 ×3 | `let todos = TodoList()` at module level; "cannot read reactive `todos` in a pure function" in `render`, `toggle` and `main` | **misleading**: the help says "if it is a fixed value, declare it `const todos = …` … or use `snap(todos)`". An instance handle is a fixed value, so `const` looked right | partly: the book says a module `let` that makes an instance "runs once at startup", but its example only uses the instance from `act fn main` and derived values | followed the help: changed it to `const` |
| 2 | check | E008 | "a `const` initializer cannot call the `act fn` `TodoList`"; help: "write `track fn TodoList`" | **misleading**: `TodoList` is a `live fn`, not an `act fn`, and making it a `track fn` is impossible or meaningless | yes: the book says a `const` that makes an instance is E008 | created the instance in `main` and passed it to `render` and `toggle` as a parameter |

What worked as the book says: instances in arrays, `items.filter(|x| x != gone)` to remove one, `let item = list.items[i]` then `emit item.Toggle()`, a derived `left` that follows each item's `done`, and an enum-typed event payload.

TS side, for fairness: Node's strip-types mode rejected `constructor(readonly title: string)` (parameter properties), which cost one TS failure.

### 5. game (3 failures)

| # | cmd | code | paraphrase | message told me what to do? | book covered it? | what I changed |
|---|---|---|---|---|---|---|
| 1 | check | E000 | `Some(v) => emit Kick(v),` in a `match` inside `on Tick`: the same grammar token list as above | **no** | partly: the book says "Inside a function, an arm can … `emit` an event", but its example only shows a block arm | wrapped the arm in `{ emit Kick(v) }` |
| 2 | check | E001 | undefined name `range` | no: no import hint | yes: the control-flow example imports `collections { range }` | added the import. This error appeared only after the parse error was fixed, and the next parse error appeared only after this one was fixed, so I fixed them one at a time |
| 3 | check | E000 | "`emit` is not supported inside a match arm block" | **partly**: clear about what is forbidden, but says nothing about what to do instead. A probe showed the same code compiles inside a plain `fn` | **contradicted**: the book says arms can emit; the limit is that an `on` handler is not "a function" | replaced the `match` with `let kick = script(ticks)` and `if kick != None { emit Kick(kick.unwrap_or(0)) }` |

What worked as the book says: `after` for the countdown, `every` for ticks, `cancel` and `stop()`, the event loop continuing after `main` returns, a derived value calling a `fn` that loops, and watch output arriving after the handler's prints (batching). The run took 0.57 s wall time for 20 ticks of 20 ms plus a 150 ms countdown.

### 6. config (0 failures)

Two modules: `config.fl` has `pub enum Value`, `pub enum ConfigError`, and `pub struct Config` with a private `entries` field, plus `parse`, `message` and typed getters. `main.fl` holds the validation and the summary. It compiled the first time. Importing `ConfigError` made its variants (`Io`, `OutOfRange`) usable in `main.fl`, as the book says. Variants named after built-in types (`Int(Int)`, `Bool(Bool)`) caused no trouble. I also added `config_test.fl` with two `fn test_*` functions; `flux test` ran them without a `flux.toml`.

Probe (not a failure): reading `cfg.entries` from `main.fl` gives E033, one of the best messages I saw (quoted below).

Surprise: the methods in `impl Config` have no `pub`, yet `main.fl` can call them. The book's `bank.fl` example works the same way but never states the rule. `flux fmt` also deletes the blank lines between methods in an `impl`.

## Things the book did not tell me (gaps)

1. **How to take part of a string.** There is no `slice`, `substring` or index for `String`; the book does not say so, or what to use instead (`chars().slice().join("")`).
2. **`else if` is not allowed in a top-level `derived` or `let`.** The book never mentions `else if` anywhere, and it works in functions.
3. **`emit` is not allowed in a `match` arm inside an `on` handler.** The book says arms can emit "inside a function", and a reader does not know that a handler is not a function for this purpose.
4. **`sorted_by`:** which key types it accepts (number or string only), whether it is stable (it appears to be), how to sort in descending order (negate the key), and whether a comparator form exists (I found none).
5. **The list of methods.** stdlib.md says "The full list of methods is in the language reference", which the rules of this test kept me from reading. I guessed `chars()`, `trim()`, `lines()`, `split_once()`, `get_or()`, `starts_with()`, `ends_with()`, `contains()`, `join()`, `keys()` and `to_lower()`. All of them existed, which suggests the naming is predictable, but the book should list them.
6. **Converting errors for `?`:** the book says `?` does not convert, but shows no `map_err` or `ok_or` (I did not find out whether they exist), so I wrote a helper.
7. **Using a module-level component instance from ordinary functions.** The book does not say that plain `fn`s cannot read it, or that the fix is to pass it as a parameter.
8. **Methods are visible to other modules without `pub`** while fields are not. The example implies it; the text never says it.
9. **`flux fmt` refuses to format a file that has a comment inside a function.** Not mentioned in the book.
10. **Unused locals and imports give no warning** (`let unused = 3` and an unused `import process { exit }` both pass `check` silently), although E015 "unused binding" is listed.
11. **Enums print fully qualified** (`Filter::All`), and named-field variants print with their fields in alphabetical order (`Shape::Rect { h: 3.0, w: 2.0 }`). This is shown in the book but not explained.
12. **Removing an array element by index:** there is no `remove(i)`; the book's components section hints at `filter(|x| x != gone)`. Nothing covers plain arrays.
13. **`flux test` without a `flux.toml`:** the book says "under the project", but a bare folder works.

## Rules that surprised me

| rule | justified once explained? | Rust / TypeScript |
|---|---|---|
| A top-level `let` is a reactive source, so a plain `fn` cannot read it (E002), even when it holds a component handle that never changes | **partly.** The purity model is consistent and the E002 help is good for numbers. For an instance handle, neither fix it offers (`const`, `snap`) is the right one | Rust: a `static` needs `OnceLock`/`LazyLock`, or you pass the value; it is explicit but has no special category. TS: any module variable is readable from any function |
| `else if` works in functions but not in derived values | **no**, it looks like a grammar gap | both allow `else if` in any expression position (Rust) or as a ternary chain (TS) |
| `emit` is allowed in a `match` arm in a `fn` but not in an `on` handler | **no.** I cannot see why a handler is more restricted than a plain function | no such distinction exists in either language |
| Methods need no import, but free functions such as `map_new`, `range` and `len` do, and `Map` has no literal or `Map::new()` | **partly.** Library modules give namespacing, but making an empty map is basic, and the E001 message does not suggest the import | Rust: `use std::collections::HashMap; HashMap::new()`, and rustc suggests the `use`. TS: `new Map()` is global |
| `sorted_by` takes only a number or string key | **no.** Tuples already have `==` and are matched structurally; ordering them is the expected next step | Rust: `sort_by_key(\|e\| (Reverse(e.1), e.0.clone()))` or `sort_by(cmp)`. TS: comparator function |
| No string slicing | **no** for a general-purpose language | Rust `&s[1..]`, or char-based `s.chars().skip(1).collect()`; TS `s.slice(1)` |
| `?` does not convert error types | **yes**: exact types are predictable. But without `map_err` in the book, it costs a helper per error source | Rust has `From` plus `map_err`; TS uses exceptions |
| `push(xs, v)` returns a new array, while `xs.push(v)` changes `xs` | **yes.** Warning E030 catches the dropped result, and the book says so up front | Rust `Vec::push` mutates; TS `push` mutates and returns the length |
| `watch x(prev)` runs once at start with `prev == x` | **yes**, it is documented, and the guard is one line | n/a. In MobX or Vue, a `watch` fires on change, not at start, unless you pass `immediate` |
| Struct fields are private to their module, but methods are public | **partly.** Hiding fields suits a module that guards its invariants; open methods without saying so is inconsistent | Rust: both private by default. TS: both public unless `#`/`private` |
| `fmt` refuses files with comments inside functions | **no** | `rustfmt` and `prettier` keep comments |
| The formatter's layout is fixed: blank lines between all top-level items, every `if/else` with blocks expanded to 5 lines | **yes** (no options, like `gofmt`), but it inflates line counts | rustfmt is similar; prettier keeps a short `if (x) return;` on one line |

## Error messages: best and worst

**Best**

- E033: "field `entries` of `Config` is private to its module", with help "only `Config`'s module can name it; use the functions or methods that module provides, or mark the field `pub` there". It names the cause and gives both fixes.
- E005 from `sorted_by`: "expected `a key that is a number or a string`, found `(Int64, String)`". It states the rule in plain words, even if it does not show the workaround.
- E002 (for a number source): "`x` is a module `let` (a reactive source); if it is a fixed value, declare it `const x = …` …, or use `snap(x)`". It teaches the reactive model at the point of confusion.
- The runtime and exit-status behavior was never a problem: `main -> Int` and `return 1` did exactly what I expected.

**Worst**

- E000: "expected eq_op, rel_op, add_op, bits_op, mul_op, kw_with, index_suffix, try_suffix, field_suffix, call_args, or kw_as", with help "check syntax near the highlighted token". It leaks grammar rule names, gives no hint that `else if` or `emit` is the problem, and appeared for two different causes. This one cost me the most time.
- E019: "no method `slice` for type `String`" with help "methods come from `impl Trait for String`; add one that defines `slice`". It tells you to extend a built-in type instead of pointing to the real API, and it brings two cascading E005 errors about a type `<error>`.
- E008 for a component: "a `const` initializer cannot call the `act fn` `TodoList`", with help "write `track fn TodoList` for a function that computes from sources". It calls a `live fn` an `act fn` and suggests an impossible fix. It came right after the E002 help that sent me to `const` in the first place.
- E001 "undefined name `map_new`" / "`range`": just "not found", although the compiler knows exactly which library module exports the name.
- "`emit` is not supported inside a match arm block": honest, but it contradicts the book, and it was hidden behind the earlier errors, so I had to fix three times to reach it.

## Overall verdict

1. For a Rust or TS programmer, the plain-data side of Flux (structs, enums, `match`, `Result`/`?`, maps, closures, modules) feels like a simplified Rust and is easy. config compiled on the first try, and ledger needed one fix.
2. The reactive side (sources, `derived`, `on`, `watch`, batches, timers) is the real selling point, and the book explains it well. The thermostat and game logic were short and correct once they parsed.
3. Most of the friction came from inconsistent edges, not from deep concepts: `else if` and `emit`-in-arm work in one context and not in another, and both errors leaked parser internals.
4. In total: 10 failed checks across 6 programs, all 6 finished. Four failures had misleading or no guidance, and only about one in three messages told me exactly what to do.
5. Missing everyday string tools (substring, padding) and an ordering API limited to one key are the biggest gaps for ordinary CLI work.
6. Fix first: make the parser accept `else if` everywhere and `emit` in every arm, or at least say "not supported in a derived value; use `else { if … }`".
7. Second: replace the grammar-token E000 message with a human sentence, and drop cascading `<error>` diagnostics.
8. Third: make E001 suggest the import ("`map_new` is in `collections`: add `import collections { map_new }`"), and stop E019 from suggesting `impl Trait for String` for library-like names.
9. Fourth: fix the E002/E008 advice for component instances (suggest passing the instance as a parameter), and add string `slice`, tuple keys for `sorted_by` (or a comparator), and a method list in the book.
10. With those fixes, I would rate Flux as easy, roughly at the level of Go, for this audience; today it is "easy, but expect to probe the compiler for what exists".

## Files

- `wordfreq/main.fl`, `main.ts`, `input.txt`
- `ledger/main.fl`, `main.ts`, `ledger.csv`
- `thermostat/main.fl`, `main.ts`
- `todos/main.fl`, `main.ts`
- `game/main.fl`, `main.ts`
- `config/config.fl`, `main.fl`, `config_test.fl`, `config.ts`, `main.ts`, and `good.conf`, `badport.conf`, `badtype.conf`, `missing.conf`, `syntax.conf`, `dup.conf`
