# What changed in 0.5

Flux 0.5 addresses problems found in the comparison study and bugs found during implementation. Value `if` branches and `match` arms now use one form across more contexts. Diagnostics give more specific messages and have explanations through `flux explain`. The standard library adds methods for text, sorting, and maps. Struct and enum values also use fewer VM instructions. The full list is in `CHANGELOG.md` at the root of the repository.

## Value `if` and `match`

A value `if` branch can run statements before its value, and `else if` works in a binding or another expression:

```flux
import io { println }

fn describe(n: Int) -> String {
    let text = if n < 0 {
        let magnitude = -n
        "minus {magnitude}"
    } else if n == 0 {
        "zero"
    } else {
        "plus {n}"
    }
    text
}

fn main() {
    println("{describe(-4)} {describe(0)} {describe(3)}")
}
```

```output
minus 4 zero plus 3
```

A `match` arm can be one `emit` or assignment statement. A block arm needs no comma after it. In an `on` handler, arm blocks can also write sources and use loops:

```flux
import io { println }

event Pick(n: Int)

event Chosen

on Pick(n) {
    match n {
        0 => emit Chosen(),
        _ => {}
    }
}

on Chosen {
    println("chosen")
}

fn main() {
    emit Pick(0)
    emit Pick(1)
}
```

```output
chosen
```

See [Control flow](./basics.md#control-flow) and [Blocks in arms](./data.md#blocks-in-arms).

## Library methods

`String.slice(start, end)` counts characters. `pad_start` and `pad_end` repeat a fill string to a width. `sorted_by` and `sort_by` accept tuple keys, compared in order; equal keys keep their order. `Map::new()` makes an empty map without an import.

```flux
import io { println }

fn main() {
    let text = "привет мир"
    let words = [("b", 2), ("a", 2), ("c", 5)]
    let ordered = words.sorted_by(|e| (-e.1, e.0))
    let mut counts: Map[String, Int] = Map::new()
    counts["a"] = 2
    println("{text.slice(0, 6)} {"7".pad_start(3, "0")}")
    println("{ordered} {counts}")
}
```

```output
привет 007
[("c", 5), ("a", 2), ("b", 2)] {"a": 2}
```

See [The standard library](./stdlib.md#methods) for the method list.

## Diagnostics and changed behavior

Errors now name types as Flux writes them and give more specific help for close names, imports, wrong argument counts, and reactive effects. `flux explain E001` prints the matching [error-code explanation](./error-codes.md); it also accepts short forms such as `e1` and `1`. The CLI suggests an explanation command after a compile error.

Mixed `Int` and `Float` branches of a value `if` or `match` now produce a `Float` value at run time: `if true { 3 } else { 2.5 }` gives `3.0`. E015 now warns for an unused local `let`, unless its name starts with `_`. A non-exhaustive `match` is E036 instead of E005. See [Reading errors](./reading-errors.md).

[What changed in 0.4](./whats-new-0.4.md) is still available.
