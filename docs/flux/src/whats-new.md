# What changed in 0.3

Flux 0.3 adds generic data types, trait bounds, a library for command-line programs, and modules that own their types and reactive state. Most 0.2 programs still compile; the changes that can alter or reject one are listed first, each with its migration. The full list is in `CHANGELOG.md` at the root of the repository. [What changed in 0.2](./whats-new-0.2.md) is still available.

## Changes that can alter programs

- **A `fn` that changes a source is an error** (E027), as announced in 0.2; it was a warning. This includes `fn main`. `flux migrate --write` turns such a function into an `act fn`.
- **Integers and floats compare exactly.** `9007199254740993 == 9007199254740992.0` is `false`; the integer used to be rounded to a float first.
- **Type names belong to their module.** Another file's type is used through an import (`import geo { Point }`) or as `geo::Point`, and only if it is `pub`.
- **Another module's source is read-only** (E032). Change it through one of that module's events or `pub act fn`s.
- **Modules initialize in dependency order**: a module after the modules it imports, each running its watches at the end of its own initialization.

## Generic types, bounds, and generic impls

Structs and enums take type parameters. The values decide the type arguments, so you never write them when building a value; an annotation or a return type supplies what the values leave open, as `Tree[String]` for `Leaf` below. A type parameter can name the traits it needs (`T: Ord`), and `impl[T]` gives every instance of a generic type its methods. The library traits are `Eq`, `Ord`, and `Display`.

```flux
import io { println }

struct Pair[T] {
    a: T,
    b: T
}

enum Tree[T] {
    Leaf,
    Node { value: T, left: Tree[T], right: Tree[T] }
}

impl[T] Pair[T] {
    fn swap(self) -> Pair[T] {
        Pair { a: self.b, b: self.a }
    }
}

impl[T: Ord] Tree[T] {
    fn insert(self, x: T) -> Tree[T] {
        match self {
            Leaf => Node { value: x, left: Leaf, right: Leaf },
            Node { value, left, right } => {
                if x < value {
                    Node { value: value, left: left.insert(x), right: right }
                } else {
                    Node { value: value, left: left, right: right.insert(x) }
                }
            }
        }
    }
    fn size(self) -> Int {
        match self { Leaf => 0, Node { value, left, right } => 1 + left.size() + right.size() }
    }
}

fn largest[T: Ord](xs: [T]) -> T {
    let mut best = xs[0]
    for x in xs {
        if x > best {
            best = x
        }
    }
    best
}

fn main() {
    let p = Pair { a: 1, b: 2 }
    let words: Tree[String] = Leaf
    let t = words.insert("pear").insert("apple").insert("plum")
    println("{p.swap()} {largest([3, 9, 2])} {t.size()}")
}
```

```output
Pair { a: 2, b: 1 } 9 3
```

A call whose type lacks a bound's trait is the new error E031, at the call: `largest([Point { x: 1, y: 2 }])` says that `Point` does not have `Ord`. Bounds are optional: code that compares `T` without one keeps working. See [Closures, generics, and traits](./abstraction.md).

## Command-line programs

New modules: `env` (`env_var`, `current_dir`), `fs` (`list_dir`, `is_dir`, `create_dir`, `remove_file`, `remove_dir`), `path` (`path_join`, `file_name`, `extension`, `parent`), and `process` (`exit`, `run`), and `read_stdin` in `io`. Failures that a program should expect, such as a missing directory, are `Err` with a message, like `read_file`'s.

```flux
import io { println }

import path { file_name, extension, path_join }

fn main() {
    let full = path_join("logs", "app.txt")
    println("{full} {file_name(full)} {extension(full)}")
    println("{9007199254740993 == 9007199254740992.0}")
}
```

```output
logs/app.txt Some("app.txt") Some("txt")
false
```

`examples/general/dir_report.fl` lists a directory and exits with status 1 when it cannot be read.

## Reactive components

A module can keep its own sources, events, `on` handlers, and watches, and other files use it through its `pub` names: `emit counter::Inc(1)`, `on counter::Inc(by) { }`. Only the module changes its own sources. See [Modules and packages](./modules.md).

## Smaller changes

- A generic call can be the argument of another: `id(id(3))`.
- An array literal takes unknown parts from later elements: `[None, Some(1)]` is an `[Option[Int]]`.
- A block `match` arm that ends in `if … else` takes the value of the branch that runs.
- A type parameter named twice (`fn f[T, T]`) is an error (E004).
- New error codes: E031 (a bound's trait is missing) and E032 (another module's source changed).
