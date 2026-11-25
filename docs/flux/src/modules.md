# Modules and packages

## Modules

Each `.fl` file is a module with its own names. Mark what others may use with `pub`:

```flux,file=geometry.fl
pub struct Point {
    x: Int,
    y: Int
}

pub fn origin() -> Point {
    Point { x: 0, y: 0 }
}

fn helper() -> Int {
    1
}

pub fn shifted(p: Point) -> Point {
    Point { x: p.x + helper(), y: p.y }
}
```

```flux
import geometry { Point, origin, shifted }

import io { println }

fn main() {
    let p: Point = shifted(origin())
    println("{p.x} {p.y}")
}
```

```output
1 0
```

| Form | Brings in |
|---|---|
| `import m { a, b }` | the listed names |
| `import m` | every `pub` name |
| `import m as x` | also lets you write `x::name` |
| `import a.b` | the file `a/b.fl`, relative to the importing file |

Importing a name that is not `pub` is an error. A module never sees its importer's names. Cyclic imports are an error, and a module imported along several paths is initialized once.

Importing a `pub let mut` gives you the same reactive source, not a copy. You can read it and react to it, but only its own module changes it.

A module can be a reactive component: it keeps its own state, events, handlers, and watches, and other files use it through its `pub` names:

```text
// counter.fl
import io { println }

pub let mut count = 0
pub event Inc(by: Int)

on Inc(by) {
    count += by
}

watch count {
    println("counter: {count}")
}
```

```text
// main.fl
import io { println }
import counter { count, Inc }

on Inc(by) {
    println("main heard {by}")
}

act fn main() {
    emit Inc(2)
    emit counter::Inc(3)
    println("now {count}")
}
```

This prints `counter: 0` while the modules initialize, then `main heard 2`, `main heard 3`, `now 5`, and, when `main`'s batch ends, `counter: 5`. Writing `count = 0` in `main.fl` is an error (E032): to change a module's state, emit its event or call one of its `pub act fn`s. Modules initialize in dependency order, each running its watches at the end of its own initialization, so `counter: 0` comes before anything `main.fl` does.

Types belong to their module too: two files may each declare a `Point`, and they are different types. Use another module's type through an import, as `Point` above, or as `geometry::Point`. Importing an enum makes its variants usable on their own, as in `Circle(1.0)`.

## Packages

A package is a directory with a `flux.toml`. Dependencies are other local directories:

```toml
[package]
name = "my_app"
version = "0.1.0"
entry = "main.fl"

[dependencies]
geometry = { path = "../geometry" }
```

`import geometry { area }` then finds the package's `lib.fl`. A package can import its own dependencies, but not its dependencies' dependencies.

There is no package registry yet, so a dependency given as a version such as `"1.2.0"` is an error.

## flux.lock

A project with dependencies gets a `flux.lock`, listing each package with a hash of its files. Commit it. `flux check`, `run`, and `test` refresh it when it is stale.

For a reproducible build, pass `--locked`: the lock is then never written, and any difference is an error naming what was `added`, `changed`, or `removed`.

```bash
flux run --locked
flux lock --check
```

Note that the hash covers source text, so reformatting a dependency changes it.

The repository has working examples in `examples/packages`.
