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

## Components

`live fn` declares a component. Calling it makes a new instance with its own sources, derived values, events, handlers, and watches. Members and parameters use their bare names inside the body. Outside, code uses `pub` members:

```flux
import io { println }

event Tick

live fn Counter(start: Int) {
    pub let mut n = start
    pub derived double = n * 2
    pub event Inc(by: Int)

    on Inc(by) {
        n += by
    }

    on Tick {
        n += 1
    }

    watch n {
        println("n={n}")
    }

    pub act fn reset() {
        n = 0
    }
}

let a = Counter(0)

let b = Counter(10)

derived total = a.double + b.double

watch total {
    println("total={total}")
}

act fn main() {
    emit a.Inc(5)
    a.reset()
    emit Tick()
    println("{a.n} {b.double}")
}
```
```output
total=20
n=0
n=10
1 22
total=24
n=1
n=11
```

The module's watch runs first, then each instance's watch in creation order. `act fn main` is one batch, so its watches run after it returns. For `a`, `Inc` raises `n` to 5, `reset` puts it back to 0, and `Tick` adds 1. `b` receives only `Tick`, so its `n` becomes 11. The print in `main` sees these values before the watches run.

A component may contain `let`, `let mut`, `derived`, `event`, `on`, `watch`, and functions (`fn`, `act fn`, `track fn`). Each member may be `pub`. A member names one value; it cannot use a destructuring pattern. A member function cannot have type parameters, `mut` parameters, or `mut self`. A component parameter cannot be `mut`. Each instance keeps its own copy of its parameters. They are fixed for its life, and reading them is not a reactive read.

Making an instance is an action, like calling an `act fn`. Module initialization, `on` handlers, `act fn`s, and functions may make instances. A derived value, `const`, or `track fn` that makes one is E008. A module-level `let` whose value makes an instance runs once at startup. For example, `let label = Label(a, "a")` reads the module binding `a` then; it is not a derived value.

The component's name is also the instance's type. Use it in arrays (`[Counter]`), function parameters (`fn total(xs: [Counter]) -> Int`), or another component's parameters (`live fn Label(c: Counter, name: String)`). An instance is a handle. `let b = a`, storing it in an array or field, and passing it all give the same instance. `==` and `!=` compare identity. Interpolation shows a name and number, such as `Counter#3`. The numbers count every instance the program made.

Outside the component, read a public value with `a.n`, emit a public event with `emit a.Inc(1)`, or call a public function with `a.reset()`. `emit` names an instance by a chain of names, as in `emit car.engine.Faster()`. To emit to an element, bind it first: `let item = list.items[i]`, then `emit item.Toggle()`. A private member is E033. Assigning a member from outside, including `xs[0].n += 1`, is E032. Emit one of its `pub` events or call one of its `pub act fn`s to change it. An event or function cannot be read as a value: `let f = c.reset` is an error.

A derived value or `track fn` that reads `a.double` follows that instance's member. If a mutable binding holding the instance changes, it follows the new instance. The same holds for an array of instances. Reads through a called function are followed too: `derived sum = total(all)` follows the `c.double` reads in `fn total(xs: [Counter])`. A plain function may read `a.n` for its current value. A watch without a target cannot read an instance's member. Give the read a name, as in `derived n = x.n`, then use `watch n { … }`.

An instance's handler for its own event runs only when that instance receives it. A handler for a global event, such as `Tick`, runs for every instance. Module handlers run first, then instances in creation order. Each instance's handlers run in declaration order. An instance made during an event does not receive that event. Watches also run for modules first, then instances in creation order. A new instance's watches run once at the end of the batch that made it.

An instance lives while a variable, field, array element, or another instance's member holds it. Global event handlers and watches do not keep it alive. What an instance's `watch` remembers to detect changes does not keep instances alive. A watch with `prev` keeps the values it hands back as `prev`. When its last handle is dropped, its handlers and watches stop no later than the end of the outermost batch. Making an instance and dropping the result as a statement warns, E030. Bind it instead, as in `let x = Counter(0)`.

A component's `let mut` and `derived` members may hold instances. A list component can have `pub let mut items: [Item] = []` and `pub derived left: Int = items.filter(|i| !i.done).len()`. It can add an item with `on Push(text) { items.push(Item(text)) }`. It can remove one with `items = items.filter(|x| x != gone)`. An instance removed from the list and held nowhere else is dropped and stops reacting.

An instance may never come to hold itself, directly or through the instances it holds. A tree node adopting its own ancestor would do this. Storing such a value in a member fails with the runtime error "an instance cannot hold itself, directly or through the instances it holds". This applies to assignment, `push`, and a derived value's new result. The error is reported where the store happens, such as "at on Adopt". The value is not stored. This includes a component's derived value that would hold a module array containing that same instance.

Recursive components support trees whose children change. A node can have `pub let mut children: [Node] = []` and `pub derived size: Int = 1 + children.map(|c| c.size).reduce(0, |x, y| x + y)`. Such a member needs a type annotation because its type depends on itself.

Chains of tens of thousands of instances are computed without exhausting the stack. This also holds when read from the far end inside a batch.

A cycle among a component's own members read by their bare names is E010. A cycle through module bindings or other components is also E010, as in `C.d → total → C.d`. Members appear as `C.a` in cycle reports. The same cycle is reported every time.

If a value comes to read itself at run time through instances, it fails with "a derived value depends on itself". It never returns an old value. During module initialization, reading a module binding that is not set yet fails with "a binding was read before its initial value was set".

Export a component with `pub live fn`. Import it like a function: `import counter { Counter, Label }`. Its name is then also the type. Component code runs on the VM. With `--jit`, the results are the same.

## Limits

- A module may have its own `on`, `watch`, and top-level `if`, but only the module that declares a source changes it (see [Modules](./modules.md)).
- A failure while settling a batch stops it, and no watch runs. The event's handlers have all run by then, since a batch settles after them.
- `emit` chains count toward the 10,000 call depth limit.
