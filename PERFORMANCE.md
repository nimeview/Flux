# Flux performance

Optimizations must improve measured workloads without changing semantics
(the VM is the reference; the JIT must match it or fall back).

## Reproducing

```bash
cargo bench -p flux_cli --bench workloads
```

The workloads live in `flux_cli/benches/workloads/`. Pure ones define
`fn run() -> Int`, which is timed on an already-initialized executor, so
compilation is excluded. Criterion keeps per-run results in
`target/criterion/` for before/after comparisons.

| Workload | Exercises |
|----------|-----------|
| `compute_loop` | `while`, integer arithmetic, locals (200k iterations) |
| `recursion` | function calls (`fib(20)`) |
| `strings` | concatenation, `str_split`, `to_string` (2,000 lines) |
| `collections` | `range`, `map`/`filter`/`reduce` with closures (20k elements) |
| `structs_match` | `push` of enum/struct values, `match`, `get` (3,000 elements) |
| `reactive_events_1000` | 1,000 events through a source and 5 derived values |
| `compile_inventory_totals` | parse → HIR → types → IR of `examples/general/inventory_totals.fl` |

## Baseline (2026-11-01)

Intel Core i9-9880H, macOS, release build; median of 10 samples.

| Workload | VM | JIT |
|----------|----|-----|
| compute_loop | 307 ms | 315 ms |
| recursion | 89 ms | 92 ms |
| strings | 34 ms | 37 ms |
| collections | 114 ms | 23 ms |
| structs_match | **26.6 s** | **29.4 s** |
| reactive_events_1000 | 7.5 ms | 9.0 ms |
| compile_inventory_totals | 1.0 ms | 1.2 ms |

Outside `collections`, the JIT is no faster than the VM: these workloads mostly fall back to it.

## After copy-on-write arrays (2026-11-01)

Same machine with Turbo Boost **off**, so the absolute numbers are not comparable with the baseline table. The "before" column is the baseline commit re-measured under the same conditions.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| compute_loop | 485 ms | 491 ms | 489 ms | 485 ms |
| recursion | 137 ms | 133 ms | 141 ms | 138 ms |
| strings | 54 ms | 52 ms | 51 ms | 54 ms |
| collections | 157 ms | 149 ms | 31 ms | 22 ms |
| structs_match | ≈26.6 s¹ | **124 ms** | ≈29.4 s¹ | **130 ms** |
| reactive_events_1000 | 15.1 ms | 14.9 ms | 17.0 ms | 17.0 ms |
| compile_inventory_totals | 1.96 ms | 2.00 ms | 2.00 ms | 1.92 ms |

¹ Baseline table (turbo on); not re-run because one sample takes minutes. The workloads that didn't change stay within noise (±4%).

Building with `push` and then scanning with `get`/`len` now scales linearly: 100,000 elements take 2.0 s and 200,000 take 3.9 s, end to end with `flux run`.

## VM hot path and JIT coverage (2026-11-01)

Turbo Boost off, as in the previous table.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| compute_loop | 491 ms | 235 ms | 485 ms | **2.2 ms** |
| recursion | 133 ms | 36 ms | 138 ms | **0.20 ms** |
| strings | 52 ms | 47 ms | 54 ms | 46 ms |
| collections | 149 ms | 54 ms | 22 ms | 22 ms |
| structs_match | 124 ms | 86 ms | 130 ms | 91 ms |
| reactive_events_1000 | 14.9 ms | 13.6 ms | 17.0 ms | 17.3 ms |
| compile_inventory_totals | 2.0 ms | 2.1 ms | 1.9 ms | 2.0 ms |

What changed in the VM (found by sampling `compute_loop` with `sample`):
- Every user call cloned the whole `CompiledFunction`, IR body included. Functions are now held in an `Arc`.
- Malloc and free took about 30% of the time. Operand stacks are now reused from a pool. An assignment overwrites the existing local in place instead of cloning its `String` key. Call arguments are taken with `split_off`, with no `format!` per argument.
- Hashing took about 16% of the time. Locals use `FxHashMap`, since their keys are compiler-generated. `Load` looks up a local once, and it skips dependency tracking for locals, which are never reactive dependencies.
- `Int64 op Int64` and `Int64` comparison have a direct path; other widths still widen to `i128`. Results and errors are the same: overflow, `MIN / -1`, and division by zero.

What changed in the JIT. Before, loops and calls inside functions fell back to the VM or were compiled incorrectly:
- Locals were plain SSA values. A loop header never saw the body's reassignment, so `while i < n { i = i + 1 }` **hung**. Locals are now Cranelift `Variable`s.
- `return` fell through to the next statement: `if x > 0 { return 1 }; 2` gave `2`. It now returns.
- An `on` handler with a payload that made a call wrote past the slot buffer, which **segfaulted**, and passed arguments to the wrong slot. Parameters are now copied into locals on entry, and calls reuse the slots.
- Function bodies may call earlier JIT functions and themselves, so `fib` compiles.

The tests in `flux_cli/tests/jit.rs` (`jit_parity_locals_reassigned_in_loops_and_branches`, `jit_parity_calls_recursion_and_early_return`, `jit_parity_call_does_not_clobber_handler_param`) cover these cases. Errors inside compiled loops (overflow, division by zero) match the VM.

Remaining: the VM still resolves locals by name, about 1.2 µs per `compute_loop` iteration. Slot-indexed locals would be the next VM step. `Value` is 72 bytes because of `UserStruct`/`UserEnum`. JIT event handling is about 25% slower than the VM in `reactive_events_1000`.

## Findings

1. **Arrays were copied quadratically (fixed).** `Value::Array(Vec<Value>)` has full value semantics, so every read of a local array (`len(xs)`, `get(xs, i)`, passing it to a function) deep-clones it, nested structs included. `xs = push(xs, v)` copies it as well, because the old value is still held by `xs` during the call. Doubling the size roughly quadruples the time, for building (`push`: 500 → 0.3 s, 1,000 → 0.9 s, 2,000 → 3.6 s) and for indexed scanning (1,000 → 1.1 s, 2,000 → 4.2 s). Fix:
   - `Value::Array` is an `Arc<Vec<Value>>`, so reads and argument passing share it, and mutation copies on write.
   - `x = push(x, v)` on a local lowers to `Inst::Take x`, which moves the array out of the local, so `push` appends in place. This happens only when `v` does not read `x`. The JIT treats `Take` as ineligible and falls back to the VM.
   - Statement and loop runners release the previous statement's value before running the next one, so it no longer keeps the array shared.
2. The interpreter loop cost about 1.5 µs per iteration (`compute_loop`, turbo on). Half of that went to allocation and hashing; see the section above. Locals are still resolved by name.
