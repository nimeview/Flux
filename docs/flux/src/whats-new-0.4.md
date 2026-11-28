# What changed in 0.4

Flux 0.4 is a stabilization release. It adds components, a host model with timers and `extern fn`, checked arithmetic, private fields, `mut` parameters, bit operations and `Bytes`, generic inference from context, and generic methods. It also reduces work in the compiler and runtime. Changes that can alter or reject a program are listed first, each with its migration. The full list is in `CHANGELOG.md` at the root of the repository.

## Changes that can alter programs

- **Struct fields are private to their module unless marked `pub`** (E033). This applies to reads, writes, `with`, destructuring `let` patterns, and struct literals. A literal must list every field, so outside its module a type with a private field must be built through its module's functions or methods. Migration: `flux migrate --write` adds `pub` to every field of every `pub struct`. Other structs and enum variants are left as they are. Then remove `pub` where a field should be private.
- **A derived value, `const` initializer, or `track fn` cannot run an `act fn`** (E008). This includes calls through other functions and function values. An action's reads are not tracked, so a derived value computed through one could go stale. Migration: use `track fn` for a function used to compute from sources.
- **A library module's name always means the library module.** `import collections` no longer imports a file `collections.fl` beside the importer. That file used to import itself if it contained `import collections`. Warning E035 says "`collections.fl` is not imported: `collections` is a library module", with help to rename it. A file is never warned about itself. Migration: rename the local file and import it by its new name.

## Components

`live fn` declares a component; `pub live fn` exports it. Each call makes an instance with its own sources, derived values, events, handlers, and watches. Outside the body, read `pub` values with `a.n`, emit events with `emit a.Inc(1)`, and call functions with `a.reset()`. Derived values follow the instance members they read. See [Components](./reactive.md#components).

An instance is a handle and lives while a value holds it. A component's `let mut` and `derived` members may hold other instances. Lists and trees can change their children. Removing an instance from a list drops it when nothing else holds it, and it stops reacting. An instance cannot hold itself, directly or through other instances. A store that would do so fails at run time without storing the value.

Recursive components can read members of earlier instances of the same component. A member whose type depends on itself needs a type annotation. A value that comes to read itself at run time fails with "a derived value depends on itself". Component code runs on the VM, with the same results under `--jit`.

## Timers and the host model

The `time` module provides `now`, `after`, `every`, `cancel`, and `stop`. `after(500, Tick())` sends an event once; `every(500, Tick())` sends it repeatedly. After `main`, `flux run` keeps running while a timer is set, until `stop()`. Each firing is one batch. A component's timer for its own event sends it to that instance without keeping it alive. `flux test` does not run timers. See [Timers](./reactive.md#timers) and the [time module](./stdlib.md#time).

`extern fn` declares a function provided by the host; `pub extern fn` exports it. Calls are actions and cannot run in a derived value, `const`, or `track fn` (E008). `flux check` accepts the declarations. `flux run` provides no host functions and cannot start a program that declares one.

Rust hosts provide functions with `Host::function` and start programs with `compile_file_with_host` or `Executor::with_host`. `EventSender::send` queues events from another thread. `run_event_loop` delivers them in order, each as one batch, and runs due timers. A host can drive time itself with `stopped`, `next_timer_in`, and `run_due_timers`. See the [host model in the specification](https://github.com/nimeview/Flux/blob/main/SPEC.md#11-host-model).

## Target checks

`flux check --target cli|web` selects a target; the default is `cli`. `web` has no `fs`, `process`, or `env`, and omits command-line and file functions from `io`. Using an unavailable module or function is E034. `web` can only be checked; nothing builds or runs for it yet. See [flux check](./tools.md#flux-check).

## Checked integer arithmetic

Every integer type has `checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem`, `checked_neg`, and `checked_pow`. They return `Some(result)` in the receiver's type, or `None` for overflow, division by zero, an integer argument that does not fit that type, or a negative exponent. These methods need no import and are pure, including in derived values and `const` initializers. See [Arithmetic](./basics.md#arithmetic).

## Private fields

Mark a field `pub` before its name, as in `pub owner: String`, to use it from another module. Inside the module, all fields remain visible. Enum variants' named fields stay visible wherever the enum is. Completion after `.` leaves out another module's private fields. See [Private fields](./modules.md#private-fields).

## Mutable parameters

Write `mut` at both the declaration and the call: `fn grow(mut xs: [Int], n: Int)` and `grow(mut xs, 3)`. The parameter starts as the argument, and its final value is written back to the caller's variable, field, or element. Several parameters can be `mut`, including in generic functions and across modules.

The call must be a statement or a `let` value. It returns a value only with a declared `-> T`. See [Mutable parameters](./basics.md#mutable-parameters).

## Bit operations and Bytes

Integers support `&`, `|`, `^`, `<<`, `>>`, and bitwise `!`. On `Bool`, `!` stays logical NOT. Shifts keep the left operand's type and drop shifted-out bits. Signed right shifts preserve the sign. A negative shift, or one at least the type's width, is `integer overflow`. The JIT runs code with bit operations on the VM. See [Bit operations](./basics.md#bit-operations).

`Bytes` holds raw bytes. Build it with `bytes([Int])` from `collections`, or a string's `to_bytes()`. Indexing gives `UInt8`; methods provide `len`, `slice`, and UTF-8 decoding with `to_text`. The `io` module has `read_bytes` and `write_bytes`. See [Bytes](./data.md#bytes).

## Generic inference and generic methods

A generic call infers missing type arguments from its expected type. This can come from an annotated binding, a declared return type, a parameter, a struct field, an assignment, or an `if` or `match` branch whose type is known. Type functions of generic types work too: `let b: Bag[Int] = Bag::new()`. Call sites have no explicit type arguments. If a type argument stays unknown, the error suggests giving the call's place a type or passing arguments of known types. See [Generics](./abstraction.md#generics).

Methods can declare their own type parameters, with optional bounds: `fn map[U](self, f: fn(T) -> U) -> Box[U]`. Calls infer them from arguments and the expected type. Each distinct instantiation is specialized. A method with its own type parameters cannot take `mut self`. See [Generic methods](./abstraction.md#generic-methods).

## Performance

Source writes in a batch and dependency reads now take constant work per write or read. Module `let mut` arrays change in place for `xs.push(v)` and `xs[i] = v`. These operations used to do quadratic work.

Library calls, component checks, value copies, and mailbox checks do less work. Against 0.3.1, retired instructions fell 15.8% for VM recursion and about 20% for collections on both engines. With `--jit`, handlers and watches without a loop run on the VM. Eligible bodies with `while` or `for` loops still run compiled. Results are the same.

Compile work fell in cycle checks, unused lookups, and grammar rules. Compiling `inventory_totals.fl` still uses 2.1% more instructions than in 0.3.1; `programs/demo/showcase.fl` uses 1.2% more. See [performance measurements](https://github.com/nimeview/Flux/blob/main/PERFORMANCE.md#04-against-03-2026-11-27).

## Smaller changes

- In the editor, component members have hover and go to definition by name in the body and through an instance. Completion after `a.` lists its `pub` members. The outline shows a component as a class with its members.
- `-128i8` is the literal -128 of type `Int8`; it used to fail at run time. `flux fmt` keeps suffixed literals as written, including `120i8`, `-128i8`, and `2.5f32`.
- In the editor, a watch cycle (E029) through an imported file shows there even when it is open. The warning goes when the importer closes.
- A derived cycle among module values is reported the same way on every run.
- The sample module `programs/ok/modules/math.fl` was renamed `arith.fl`; it shadowed the library's `math`.

[What changed in 0.3](./whats-new-0.3.md) is still available.
