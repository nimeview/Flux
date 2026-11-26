# Modules and packages

## Modules

Each `.fl` file is a module with its own names. Mark what others may use with `pub`:

```flux,file=geometry.fl
pub struct Point {
    pub x: Int,
    pub y: Int
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

### Private fields

A struct field is private to its module unless you put `pub` before its name. Making the struct `pub` lets other modules use the type; mark each field they may name separately:

```text
// bank.fl
pub struct Account {
    pub owner: String,
    balance: Int
}

impl Account {
    fn open(owner: String) -> Self {
        Self { owner: owner, balance: 0 }
    }

    fn deposit(mut self, amount: Int) {
        self.balance += amount
    }

    fn balance(self) -> Int {
        self.balance
    }
}
```

```text
// main.fl
import bank { Account }
import io { println }

fn main() {
    let mut a = Account::open("ann")
    a.deposit(5)
    println("{a.owner} {a.balance()}")
}
```

This prints `ann 5`. Inside `bank.fl`, every field is visible, including in its functions, methods, and `impl` blocks. A method can share a field's name: `balance()` reads the private `balance` field.

In another file, only `pub` fields can be named in reads (`a.owner`), writes (`a.owner = "jo"`), `with { … }`, `let Account { … } = a` patterns, and struct literals. A literal must list every field, so an `Account` cannot be built by a literal outside `bank.fl`; use `Account::open` or the methods the module provides. Writing `a.balance` or `Account { owner: "x", balance: 1 }` in `main.fl` is E033: "field `balance` of `Account` is private to its module". Completion after `.` leaves out another module's private fields.

Enum variants' named fields, such as `Rect { w: Float }`, have no `pub` and stay visible wherever the enum is.

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

`flux check --locked main.fl` and `flux run --locked main.fl` use the same project root as `./main.fl`. Either spelling checks the same lock.

Note that the hash covers source text, so reformatting a dependency changes it.

The repository has working examples in `examples/packages`.
