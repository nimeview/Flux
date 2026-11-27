# Status and limits

Flux is at version 0.3. This page says what you can rely on.

## Stable

These change only by a deliberate decision, recorded in the changelog:

- The behavior of the VM, which is the reference for what programs mean.
- Numbers, strings, arrays, `Map`, `Option`, `Result`, structs, enums, recursive types, generic functions, structs, and enums, trait bounds and generic impls, closures, and function types.
- Reactive values: `let mut`, `derived`, `event`, `on`, `watch`, `track fn`, `act fn`.
- Modules, with their own types and reactive state, and local packages with `flux.lock`.
- `flux init`, `check`, `run`, `test`, `fmt`, `lock`, `repl`, and `lsp-server`.
- The standard library modules `io`, `collections`, `math`, `str`, `option`, `result`, `testing`, `env`, `fs`, `path`, and `process`.
- Linux and macOS, which CI tests.

## Experimental

These may change or disappear, with no compatibility promise:

- **`--jit`**, the Cranelift backend. It gives the same results as the VM, and falls back to the VM for what it cannot compile. It is much faster in numeric loops and recursion. It does not compile structs, maps, `Option`, `Result`, closures, or functions that print. Event handlers and watches whose bodies have no loop run on the VM; copying values into compiled slots and back costs more than their code. Eligible bodies with a `while` or `for` loop still run compiled. `flux run --jit --verbose` shows what fell back and why.
- **`flux wasm`**, a stub that always fails and writes nothing.
- **`flux migrate`**, which rewrites older forms to the current syntax (see [Tools](./tools.md#flux-migrate)).

## Not there yet

- A package registry, and dependencies by version.
- Windows support.
- Rerunning a cycle of watches until values stop changing.
- Catching a runtime error from inside the language.

## Known limits

- In a trace, a statement written directly in a `match` arm reports the line of the `match`. A top-level `if` has no location.
- When compiled code fails, it is run again on the VM to report the error, so a failure after long native work can take a while.
- Strings order by Unicode code point, not alphabetically.
- In the editor, a watch cycle (E029) that runs through two files is reported by `flux check`, but not shown when both files are open.

## Versions

Flux follows [Semantic Versioning](https://semver.org) with the usual reading for 0.x: while the version starts with 0, a change that can break a program raises the second number (0.1 to 0.2), and anything else raises the third (0.1.0 to 0.1.1).

A breaking change is one that makes a program that compiled stop compiling or behave differently. Fixing a bug can do that when a program relied on the bug, and such fixes are listed in the changelog. Changes to anything listed as experimental above are not breaking. The compiler's cache and internal formats are not part of the language and can change at any time.

The compiler, the standard library, and the language share one version number.
