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
| `import m as x` | also lets you write `x.name` |
| `import a.b` | the file `a/b.fl`, relative to the importing file |

Importing a name that is not `pub` is an error. A module never sees its importer's names. Cyclic imports are an error, and a module imported along several paths is initialized once.

Importing a `pub let mut` gives you the same reactive source, not a copy.

Struct, enum, and trait names are currently global to the program, so two modules cannot define the same type name.

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
