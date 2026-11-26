# What changed in 0.2

Flux 0.2 makes reactive code predictable and ordinary data code shorter. Most 0.1 programs still compile: an older form gives warning E026, which names its replacement, and `flux migrate --write` rewrites it for you. A few changes alter what a program does; they are listed first. The full list, with a migration for each change, is in `CHANGELOG.md` at the root of the repository.

## Changes that can alter programs

- **Reads are current.** Reading a derived value after changing one of its sources gives the new value, even in the same `on` body.
- **Changes settle in batches.** An `emit` or an `act fn` call opens a batch; nested `emit`s and `act fn` calls join it. Watches run once, when the batch ends, instead of after each handler.
- **Watches remember what they saw.** A watch runs only when a value it follows differs by `==` from the last time. Setting a source to its current value no longer wakes it.
- **Indices are evaluated once.** In `grid[f()][j] = v`, `f()` runs once.

```flux
import io { println }

event Deposit(amount: Int)

let mut balance = 0

derived doubled = balance * 2

on Deposit(amount) {
    balance += amount
    println("doubled is already {doubled}")
}

watch doubled {
    println("watch saw {doubled}")
}

fn main() {
    emit Deposit(5)
    emit Deposit(1)
}
```

```output
watch saw 0
doubled is already 10
watch saw 10
doubled is already 12
watch saw 12
```

## Writing state

Only an `on` handler, an `act fn`, and initialization should change a source. A plain `fn` that does gives warning E027 (an error from 0.3), and `flux migrate` turns it into an `act fn`. A `watch` that writes gives E028. `rfn` is now spelled `track fn`.

Events carry several values, and a watch can name the value it saw last time:

```flux
import io { println }

event Move(dx: Int, dy: Int)

let mut x = 0

let mut y = 0

derived far = x * x + y * y > 25

on Move(dx, dy) {
    x += dx
    y += dy
}

watch far(was) {
    println("far: {was} -> {far}")
}

act fn reset() {
    x = 0
    y = 0
}

fn main() {
    emit Move(3, 4)
    emit Move(1, 0)
    reset()
}
```

```output
far: false -> false
far: false -> true
far: true -> false
```

## Data

You can change part of a value in place, add methods to a type, and walk collections with methods from the library, which need no import:

```flux
import io { println }

struct Task {
    title: String,
    done: Bool
}

impl Task {
    fn label(self) -> String {
        let mark = if self.done { "x" } else { " " }
        "[{mark}] {self.title}"
    }
}

fn main() {
    let mut tasks = [Task { title: "write", done: false }, Task { title: "test", done: false }]
    tasks[0].done = true
    tasks.push(Task { title: "ship", done: false })
    let review = tasks[1] with { title: "review" }
    for (i, task) in tasks.enumerate() {
        println("{i} {task.label()}")
    }
    println("{review.label()} {tasks.len()}")
}
```

```output
0 [x] write
1 [ ] test
2 [ ] ship
[ ] review 3
```

- `p.x = 3`, `xs[i].done = true`, `+=` and the other compound assignments.
- `p with { x: 3 }`: a copy with some fields changed.
- `impl Type { }` with `self`, `mut self`, type functions such as `Point::new`, and `Self`. Methods can be added to library types like `String`.
- Library methods: `text.split(",")`, `xs.sorted()`, `xs.push(v)` (which changes `xs`), `o.unwrap_or(0)`, and new ones such as `find`, `any`, `enumerate`, `zip`, `slice`, `get_or`, `split_once`, and `lines`. The free functions still work.

## Tuples, paths, and variants

```flux
import io { println }

import math

enum Shape {
    Circle(Float),
    Rect { w: Float, h: Float }
}

fn area(s: Shape) -> Float {
    match s { Shape::Circle(r) => 3.0 * r * r, Rect { w, h } => w * h }
}

fn main() {
    let shapes = [Shape::Circle(1.0), Rect { w: 2.0, h: 3.0 }]
    let (small, big) = (area(shapes[0]), area(shapes[1]))
    let words = "b a c".words().sorted()
    println("{small} {big} {words} {math::sqrt(16.0)}")
}
```

```output
3.0 6.0 ["a", "b", "c"] 4.0
```

- Tuples: `(1, "a")`, `t.0`, `let (a, b) = t`, tuple patterns, and `for (k, v) in m`.
- `::` names a module's item (`math::sqrt`), a type function, or a variant (`Shape::Circle`). `m.name` still works, with warning E026.
- A variant with named fields is built like a struct: `Rect { w: 2.0, h: 3.0 }`.

## Smaller things

- Raw strings `r"C:\path"`, `;` between statements on one line, and a trailing comma in any list.
- Warnings E029 (watches that wake each other in a cycle) and E030 (a result that is computed and dropped, such as `xs.sorted()` on its own line).
- In the editor: quick fixes for E026 and E027, and hover, completion, and signature help for library methods.

## Migrating

```bash
flux check                   # lists every older form, with its replacement
flux migrate src             # shows the changes
flux migrate src --write     # makes them
```

Then read the [changes that can alter programs](#changes-that-can-alter-programs) once more: `flux migrate` changes syntax, not behavior.
