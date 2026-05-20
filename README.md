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
- Move from RPN payload strings to typed constant/opcode pools in binary format v2.
- Add module system and import resolution for std/user packages.
