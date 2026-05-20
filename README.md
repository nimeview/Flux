# Flux

Flux has split architecture and folder ownership:

- `compiler/` (Rust): parser, HIR, and binary bytecode emission.
- `runtime/` (C++): bytecode execution, reactive graph, scheduler.
- `stdlib/` (C++ headers): standard-library API contracts (e.g. `print`).
- `shared/` (C++ headers): shared protocol structures and binary bytecode layout between compiler/runtime.

## Project architecture plan

- [x] Move `print` from syntax to stdlib function call `print(x)`.
- [x] Split runtime responsibilities into modules:
  - `runtime/graph`: dependency graph storage.
  - `runtime/scheduler`: incremental recomputation.
  - `runtime/runtime_api`: token/trim helpers and runtime API utilities.
  - `stdlib/`: stdlib function implementations.
- [x] Define shared instruction representation in `shared/bytecode.hpp`.
- [x] Add binary bytecode format in `shared/` and decoder/encoder on both sides.
- [x] Add typed values and richer stdlib (math, time, collections).

## Flux example

```fx
signal mouse_x = 10
signal mouse_y = 4
reactive cursor = (mouse_x + mouse_y) * 2
print(cursor)
set mouse_x = 20
print(cursor)
```

## Build and run

```bash
cargo run -- /tmp/demo.fx /tmp/demo.fxbc
cmake -S runtime -B runtime/build
cmake --build runtime/build
./runtime/build/runtime /tmp/demo.fxbc
```


## Next language development

- [x] Add type-checker in compiler (number/bool/vector + function signatures).
- Add control flow (`if`, `match`) and user functions.
- [x] Move from RPN payload strings to typed constant/opcode bytecode format v2.
- [x] Add module system and import resolution for std/user packages.


## Bytecode v2 status

- Binary version `2` is now used.
- Expression payload is opcode-based (`push_number`, `push_bool`, `load_name`, arithmetic ops, `call`) instead of text RPN.
- Runtime decodes and executes opcode payload directly.


## Strong typing progress

- Added parser support for string literals and tuple expressions.
- Added semantic types: `String`, `Tuple`, `Option`, `Result`.
- Added parser diagnostics with `line/col` in error messages.
- Added bytecode v2 opcodes for `push_string` and `make_tuple` (compiler side).


## Control flow/functions progress

- Added parser/HIR/type-check support for `if ... { ... } else { ... }`.
- Added parser/HIR/type-check support for `match` expressions with basic patterns (`_`, bool, number, variable).
- Added function declarations `fn name(params){ ... }`.
- Added lambda expressions `|x, y| expr` with lexical scope type-checking (`Unknown` param placeholders for now).
- Current limitation: binary emission/runtime execution for `fn`, `match`, `lambda` is not implemented yet (compiler returns TODO panic for those in binary mode).


## Module system progress

- Added `import <path>` and `import <path> as <alias>` statements.
- Added namespace-aware resolution in type-checking and lowering (e.g. `s::sqrt` -> `std::sqrt`).
- Kept package layout split between `std` namespace and user namespaces (`user::<pkg>::...` style path syntax).
- Import statements are compile-time directives and are not emitted as runtime bytecode instructions.


## Module resolution

- Entry file compilation now resolves user imports from filesystem recursively.
- Import path mapping: `foo::bar` -> `foo/bar.fx` relative to importing file directory.
- `std` imports are treated as built-in namespace and do not require local files.
- Imported modules are loaded once (deduplicated by canonical path) and merged before type-check/bytecode emission.


### user package example

`main.fx`
```fx
import std as s
import pkg::vals
signal x = vals::base
reactive y = s::sqrt(x)
s::print(y)
```

`pkg/vals.fx`
```fx
signal base = 9
```


## Runtime determinism progress

- Added per-node metadata with `version` and `last_eval_tick`.
- Added deterministic scheduler order by topological affected set with stable lexical ordering.
- Added transaction/tick batching: multiple `set` updates in a row are batched and recomputed in one flush tick.
- Tick flush happens before side-effect expression execution (`expr`) and at program end.


## Tooling progress

- Added runtime trace mode: `./runtime/build/runtime <file.fxbc> --trace`.
- Added explain-why-recomputed tracing in scheduler (cause chain + tick + node).
- Added lightweight graph inspector output in trace mode (node version + last tick).
- Added reusable runtime diagnostics API (`set_trace`, `trace`) in `runtime/runtime_api`.

## HIR feature matrix (binary/runtime path)

| HIR construct | parse | typecheck | lower (emit_binary v3) | runtime-exec |
|---|---|---|---|---|
| `if` | ✅ | ✅ | ✅ (`branch_if_false` + `jump`) | ✅ |
| `match` | ✅ | ✅ | ✅ (branch-chain lowering) | ✅ (pattern bool/number compare opcodes) |
| `lambda` | ✅ | ✅ | ❌ fail-fast at compile-time | ❌ |
| `fn` | ✅ | ✅ | ❌ fail-fast at compile-time | ❌ |
| `tuple` | ✅ | ✅ | ❌ fail-fast at compile-time | ❌ |
| `string` | ✅ | ✅ | ❌ fail-fast at compile-time | ❌ |

Notes:
- `emit_binary` is intentionally strict now: unsupported constructs fail immediately with explicit diagnostics.
- The canonical `if` lowering for bytecode v3 is opcode-based control flow (`branch_if_false`, `jump`).
