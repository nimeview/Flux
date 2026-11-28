# Findings log

Notes made while writing the comparison programs: friction, limits, and
surprises in Flux (and in the other languages), with where they came up.
The report draws on these; independent agents' notes are kept separately.

- compute/binary_trees: a tuple variant holds one value (`Node(Tree, Tree)` is a parse error: "a tuple variant holds one type; use a struct for several"); written as `Node { l: Tree, r: Tree }`. Rust, Swift, OCaml, Elm, and TypeScript unions allow several.
- compute/binary_trees (depth 16): Flux VM 47.6 s, Python 2.7 s (17× slower). Profile: ~55% malloc/free. Each enum value allocates an `Arc<EnumValue>` with owned `String`s for the type and variant names, and a struct payload adds an `Arc<StructValue>` with a `HashMap<String, Value>` and owned key strings: about seven allocations per node, against one tuple in Python. Candidate fix: interned type/variant/field names and fields in declaration order (`Vec`), not a map.
- compute/maps: `map_new()` needs `import collections { map_new }` while the map's methods (`get_or`, `keys`) need no import; an empty map literal does not exist. Minor, but every other language here builds an empty map without an import.

## Reactive scenarios (author, 2026-11-27)

- **A watch writing a source is a warning (E028), drawn like an error.**
  `watch s { seen += s }` runs, but the E028 warning is rendered with the
  same `x` marker and layout as an error, so next to a real error (here
  E002 in `main`) I read it as a ban and rewrote the watch to emit an
  event. Warnings should look different (Rust: `warning:` vs `error:`).
- **`main` reads a source only through `snap(...)`.** Printing a final
  counter from `main` needs `snap(seen)`; the help text says so clearly.
- **Check time explodes with a watch at the end of a long derived chain.**
  `flux check` on 1000 chained `derived` with `watch d999 {}`: 9.5 s;
  500: 0.84 s; 250: 0.15 s; 125: 0.03 s (~n^2.5). Without the watch the
  same chain checks in 0.02 s. Something walks the dependency chain per
  node (transitive closure without memo?). Runtime is fine: 1000 updates of
  the whole chain take 186 ms.
- **Two grammars for `if`.** As a function's or arm's last statement,
  `if … else if … else` works and branches may hold statements. As an
  operand (`let x = if …`), the grammar is `if c { expr } else { expr }`:
  no `else if`, no statements in a branch (parse error "expected eq_op,
  rel_op, …" at `else if`). Rust has one `if` expression everywhere. Work
  around: move the chain into a function.
- **Recompute-everything is competitive with fine-grained graphs at these
  sizes.** Elm (no graph: derived values recomputed from the model per
  message): chain 22 ms, fan 44 ms, counters 13 ms, todos 84 ms; the JS
  signal libraries take 23–500 ms on chain/fan/counters and 29–70 ms on
  todos. Fine-grained graphs pay off when recomputation is expensive or the
  DOM is touched; the report must not sell reactivity as a speed feature.
- **Flux's graph is cheap; Flux's interpreter is not.** The same work
  without the graph (`flux/plain.fl`) takes almost the same time on the VM
  (chain 122 vs 121 ms, todos 511 vs 511 ms). With the JIT, the plain int
  loops drop to 5–8 ms, but reactive handlers stay on the VM. The todos gap
  (Flux 511 ms vs JS 29–70 ms) is struct/closure allocation in map/filter,
  the same cause as binary_trees.
- **Svelte 5 runes overflow the stack on a 1000-deep derived chain** on
  Node (RangeError) but not on Bun; Flux handles the same depth.
  Root cause: `flux_hir/src/cycles.rs` `close()` (E029 watch-cycle check,
  runs only when a program has a watch) computes the transitive effects by
  naive fixpoint iteration: every round clones every unit's sets and
  expands one level. A chain of n derived needs up to n rounds × n units ×
  O(n) sets. An SCC (Tarjan) pass in reverse topological order makes it one
  pass. A compiler bug, not a language question.

## Error messages vs Rust, TypeScript, Python, Elm (results/errors.json)

Same seven mistakes in each language (`errors/cases.py`). Flux always points
at the right span, but:
- **No "did you mean".** `totl` → Rust, TS, Python and Elm all suggest
  `total`; `xs.lenght()` → all suggest `len`/`length`. Flux says only "not
  found".
- **E019's help is wrong for a typo**: "methods come from `impl Trait for
  [Int64]`; add one that defines `lenght`" — tells the user to write the
  misspelled method.
- **A missing match arm is "E005 type mismatch: expected `exhaustive match
  (Circle, Square, Triangle)`, found `missing Triangle`"**. Rust (E0004),
  Elm (MISSING PATTERNS) give it its own error and list what is missing in
  plain words.
- **Arity**: "expected `2 arguments`, found `1 arguments`" (grammar), no
  note pointing at the definition, no inserted placeholder (Rust:
  `area(3, /* i64 */)`).
- **Option used as a number**: "expected `numeric or String type`, found
  `Option<Int64> and Int64`" — no hint to unwrap/match (Elm: "Use
  Maybe.withDefault …", TS: "possibly 'undefined'").
- **Type names differ from what the user wrote**: the program says `Int`,
  messages say `Int64`.
- **No `--explain`**: Rust's `rustc --explain E0425` has a long text per
  code; Flux has codes and a book chapter, but no per-code explanation from
  the tool.
- Good: immutable assignment has a correct, Rust-like help
  (`let mut count = ...`). Python finds 2 of 7 only at run time and accepts
  2 silently (non-exhaustive match returns None; Final reassignment runs).

## Bugs found by the restriction review (Codex), reproduced by me

Repro files in `openness/repro/`; all on the release binary built from
ac12656.
- **A component parameter can be assigned.** `live fn C(x: Int) { …
  pub act fn reset() { x = 2 } }` checks and runs; `get()` then returns 2.
  SPEC/book say parameters stay fixed for the instance's life
  (`live_fixed_param.fl`).
- **A module-level alias of an instance loses its methods.** `let a = C()`
  then `let b = a`, `b.inc()` → E019 "no method `inc` for type `Cell<C>`"
  (an internal type leaks into the message). The same alias inside a
  function works (`live_alias.fl`, `live_alias_local.fl`).
- **`flux check` passes a cross-template reactive cycle that `flux run`
  rejects with E010** (`live_template_cross.fl`: `C.d → total → C.d`).
- **SPEC vs book/binary**: SPEC's diagnostic table lacks E035; one SPEC
  paragraph calls a private import E014, its table says E022; the book says
  derived values cannot loop, but loops in helpers called by a derived value
  are accepted (the rule as written is not enforced, and Codex judges it
  unnecessary).

## Newcomer test (agent, book only; ease/LOG.md), claims I reproduced

6/6 programs finished; 10 failed checks; Flux 510 non-blank lines vs
TypeScript 303 (1.7×). Reproduced:
- **`flux fmt` refuses any file with a `//` comment inside a function**
  ("fmt cannot preserve a comment inside a declaration yet").
- **`emit` in a `match` arm inside an `on` handler is a parse error**
  (works in a `fn`); the error is the grammar-token list.
- **No warning for an unused local** (`let unused = 5`), though E015 exists.
Reported by the agent (not re-run by me): no substring/slice on strings
(workaround `s.chars().slice(a, b).join("")`); `sorted_by` takes only an
Int or String key (no tuple key, no comparator; stability undocumented);
E002 on a component instance held in a module `let` sends the user to
`const`, which then fails with E008 calling the `live fn` an "act fn";
`impl` methods callable across modules without `pub` while fields need it;
no `map_err`, so converting error types for `?` is by hand.
