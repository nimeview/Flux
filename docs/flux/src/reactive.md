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

on Add { amount ->
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

`event` declares an event, which can carry one value. `emit` sends it, and every `on` block for it runs right there, in declaration order, before the next statement.

## Watches

`watch x { ... }` runs once when the program starts, and again each time `x` changes. Without a name, `watch { ... }` runs when anything its body reads changes.

## When things update

Inside an `on` or `watch` body, a write changes the source at once, but derived values do not update until the body ends. So a body sees a consistent old state, and several writes count as one change:

```flux
import io { println }

event Tick

let mut x = 0

let doubled = x * 2

on Tick {
    x = 1
    println("in handler: x={x} doubled={doubled}")
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
in handler: x=1 doubled=0
watch: doubled=2
done
```

When a body ends, derived values recompute, then the watches they affect run. A watch's own writes settle the same way, within the same event. Each watch runs at most once per settling, so a cycle of watches always ends.

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
- **Use `rfn`**, a reactive function. Whatever calls it depends on what it reads.
- **Use `snap(x)`**, which reads the current value once, deliberately, inside an ordinary function.

```flux
import io { println }

event Up

let mut factor = 2

rfn scale(x: Int) -> Int {
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

A function may write to a source. The change propagates at once, because a function is not a batched body:

```flux
import io { println }

let mut x = 0

watch x {
    println("x={x}")
}

fn set_it() {
    x = 1
    println("after")
}

fn main() {
    set_it()
}
```

```output
x=0
x=1
after
```

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

- `watch`, `on`, and top-level `if` must be in the root module of a program.
- A failure while recomputing stops the event: later handlers do not start.
- `emit` chains count toward the 10,000 call depth limit.
