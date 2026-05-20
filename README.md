# Flux

Flux now has a split architecture:

1. **Rust compiler (`fluxc`)**: parses Flux source and emits text bytecode (`.fxbc`).
2. **C++ runtime (`flux_runtime`)**: executes bytecode as a reactive dependency graph.

## Implementation plan

- [x] Phase 1: define minimal language (`signal`, `reactive`, `set`, function calls (e.g. `print(x)`)) and arithmetic expressions.
- [x] Phase 2: implement Rust parser + compiler output format.
- [x] Phase 3: implement C++ runtime graph recomputation engine.
- [x] Phase 4: connect compiler output to runtime input and run end-to-end.
- [ ] Phase 5: add functions, modules, conditions, and typed values.

## Flux example

```flux
signal mouse_x = 10
signal mouse_y = 4
reactive cursor = (mouse_x + mouse_y) * 2
print(cursor)
set mouse_x = 20
print(cursor)
```

## Build and run

```bash
cargo run -- /tmp/demo.flux /tmp/demo.fxbc
cmake -S runtime -B runtime/build
cmake --build runtime/build
./runtime/build/runtime /tmp/demo.fxbc
```
