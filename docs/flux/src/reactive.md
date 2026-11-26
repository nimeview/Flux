# Reactive programming

Reactive values are declared at the top level of a file. They form a graph: when a value changes, everything that depends on it is recomputed, and code watching it runs.

## Sources and derived values

```flux
import io { println }

event Add(amount: Int)

event Reset

let mut balance = 0

let mut limit = 100

let doubled = balance * 2

derived over = balance > limit

on Add(amount) {
    balance = balance + amount
}

on Reset {
    balance = 0
}

watch balance {
    println("balance={balance} doubled={doubled} over={over}")
}

fn main() {
    emit Add(50)
    emit Add(80)
    emit Reset()
}
```

```output
balance=0 doubled=0 over=false
balance=50 doubled=100 over=false
balance=130 doubled=260 over=true
balance=0 doubled=0 over=false
```

| Top-level form | Meaning |
|---|---|
| `let mut x = 0` | a **source**: handlers can change it |
| `let y = x * 2` | a **derived** value, because it reads a source; it cannot be assigned |
| `let z = 5` | a source that nothing can change |
| `derived y = x * 2` | a derived value, stated explicitly |
| `const N = 5` | a constant, fixed at compile time |

This is the one place where `let` means something different at the top level from inside a function, and it is the first thing to remember about Flux.

Derived values must be pure. They cannot print, write files, or `emit`, directly or through anything they call, and they cannot use loops:

```flux,error=E017
import io { println }

fn noisy(x: Int) -> Int {
    println("computing")
    x
}

let mut a = 1

let b = noisy(a)

watch b {
    println("{b}")
}

fn main() {
}
```

## Events and handlers

`event` declares an event, which can carry values. `emit` sends it, and every `on` block for it runs right there, in declaration order, before the next statement. A handler names the values in parentheses, as the event declares them:

```flux
import io { println }

event Move(dx: Int, dy: Int)

let mut x = 0

let mut y = 0

on Move(dx, dy) {
    x = x + dx
    y = y + dy
}

fn main() {
    emit Move(2, 3)
    emit Move(-1, 1)
    println("at {snap(x)}, {snap(y)}")
}
```

```output
at 1, 4
```

A handler that does not need the values may leave the parentheses out. The older form for one value, `on Tick { n -> }`, still works with warning E026, and `flux migrate` rewrites it.

## Watches

`watch x { ... }` runs once when the program starts, and again each time `x` changes. Without a name, `watch { ... }` runs when anything its body names changes.

A watch remembers what it saw. It runs only when the value differs from the one it saw last time, so a value that changes and changes back within one event wakes nothing. To see the old value, name it in parentheses; on the first run it is the current value:

```flux
import io { println }

event Set(n: Int)

let mut level = 0

on Set(n) {
    level = n
}

watch level(prev) {
    println("{prev} -> {level}")
}

fn main() {
    emit Set(3)
    emit Set(3)
    emit Set(1)
}
```

```output
0 -> 0
0 -> 3
3 -> 1
```

The second `Set(3)` changed nothing, so the watch did not run.

## When things update

Inside an `on` or `watch` body, a write changes the source at once, and reading a value derived from it gives the new value: it is recomputed when read. Watches, though, wait until the body ends, so several writes reach them as one change:

```flux
import io { println }

event Tick

let mut x = 0

let doubled = x * 2

on Tick {
    x = 1
    println("in handler: x={x} doubled={doubled}")
    x = 2
}

watch doubled {
    println("watch: doubled={doubled}")
}

fn main() {
    emit Tick()
    println("done")
}
```

```output
watch: doubled=0
in handler: x=1 doubled=2
watch: doubled=4
done
```

The watch saw `doubled` once, at 4; the intermediate 2 was read in the handler but never reached a watch. A derived value nobody reads is not computed at all until the body ends, so a value that would fail for a moment, such as `10 / a` while `a` is briefly 0, does no harm.

The changes form one **batch**. An `emit`, or a call to an `act fn`, opens a batch when none is open; everything it starts joins it: all the handlers of the event, a nested `emit` and its handlers, and the `act fn`s they call. When the batch ends, derived values recompute, then the watches they affect run. A watch's own writes, and the events it emits, settle in the same batch. Each watch runs at most once per batch, so a cycle of watches always ends.

If something fails in the middle of a batch, the writes made before the failure stay, and nothing else runs.

## Functions and reactive values

A plain `fn` cannot read a reactive value: its result would silently depend on something the graph does not know about.

```flux,error=E002
let mut limit = 10

fn bad() -> Int {
    limit
}

fn main() {
    let _a = bad()
}
```

You have three options, depending on what you want:

- **Pass it as a parameter**, or make it a `const` if it never changes.
- **Use `track fn`**, a tracked function. Whatever calls it depends on what it reads.
- **Use `snap(x)`**, which reads the current value once, deliberately, inside an ordinary function.

```flux
import io { println }

event Up

let mut factor = 2

track fn scale(x: Int) -> Int {
    x * factor
}

let total = scale(3)

fn current() -> Int {
    snap(factor)
}

on Up {
    factor = factor + 1
}

watch total {
    println("total={total}")
}

fn main() {
    emit Up()
    println("factor={current()}")
}
```

```output
total=6
total=9
factor=3
```

Sources change in three places: an `on` handler, an `act fn`, and the module's own initialization. An `act fn` is a function that changes state; unlike an event, it can take several values and return a result:

```flux
import io { println }

let mut x = 0

watch x {
    println("x={x}")
}

act fn set_it() {
    x = 1
    println("after")
}

fn main() {
    set_it()
}
```

```output
x=0
after
x=1
```

The call to `set_it` is one batch, so the watch runs once it returns. A `fn`, `track fn`, or method that writes a source is an error, E027; the editor's quick fix and `flux migrate` turn such a `fn` into an `act fn`, and a method can call an `act fn`. A `watch` that writes one still runs but warns, E028: compute the value with `let` instead, or `emit` an event whose `on` handler changes it.

A derived value, a `const` initializer, or a `track fn` cannot run an `act fn` (E008). This includes a direct call such as `derived y = read()`, a call through a `track fn` or plain `fn`, and a call through a function value such as `apply(read)`.

An action's reads are not tracked. Before 0.4, a derived value computed through an action that only read a source could go stale. An action that wrote a source was already rejected as a side effect (E017).

Write `track fn` instead of `act fn` for a function that computes from sources. Here, `y` follows what `read` reads:

```flux
import io { println }

let mut x = 1

track fn read() -> Int {
    x
}

derived y = read()

act fn main() {
    x = 2
    println("{x} {y}")
}
```

```output
2 2
```

With `act fn read`, this program printed `2 1` before 0.4. It now reports E008: "a derived value cannot call the `act fn` `read`". `act fn main` and `on` handlers may still call any `act fn`, and an `act fn` may call another.

## Dependencies follow the branch taken

A derived `if` or `match` depends only on what the branch it took read. Writes to the other branch's sources do not recompute it:

```flux
import io { println }

event Flip

event Other

let mut use_a = true

let mut a = 1

let mut b = 2

let chosen = if use_a { a } else { b }

on Other {
    b = 20
}

on Flip {
    use_a = false
}

watch chosen {
    println("chosen={chosen}")
}

fn main() {
    emit Other()
    println("after Other")
    emit Flip()
}
```

```output
chosen=1
after Other
chosen=20
```

## Limits

- A module may have its own `on`, `watch`, and top-level `if`, but only the module that declares a source changes it (see [Modules](./modules.md)).
- A failure while settling a batch stops it, and no watch runs. The event's handlers have all run by then, since a batch settles after them.
- `emit` chains count toward the 10,000 call depth limit.
