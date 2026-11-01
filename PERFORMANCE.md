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
| `enum_match` | `match` on integers and on a user enum with payloads, in a loop (100k iterations) |
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

## `match` in the JIT (2026-11-01)

Turbo Boost off. A/B against the previous commit (7e9b2f9) in a temporary worktree, same session.

| Workload | JIT before | JIT after |
|----------|-----------|-----------|
| enum_match (new) | 1.748 s (fell back to the VM) | **0.315 s** |
| collections | 22.2 ms | 23.0 ms |
| reactive_events_1000 | 17.1 ms | 17.5 ms |

All other workloads, VM and JIT, stayed within noise (±4%). VM `enum_match`: 1.57 s before and after.

- `BindLocal`, `EnumTagEq`/`VariantTagEq`, and `VariantPayloadGet` for tuple variants are now JIT-eligible. Their old exclusion predates locals becoming Cranelift variables, which scope a binding to its branch. Struct-variant fields stay on the VM.
- Correctness fix in the same change: structs, `Option`, and `Result` have no tagged form, and compiled code received them as `()` (`pass(Some(3))` returned `()`). The runtime now runs an entry on the VM when an argument, event payload, or reactive node it reads has no tagged form. `collections` and `reactive_events` pay about 3% for that check, which is folded into building the slot buffer.
- The differential test now has `match` at normal frequency and a user enum. All 141 programs that produce a value are compiled natively (before: 105 of 152).

## Event propagation (2026-11-02)

Turbo Boost off. A/B against 2535202 in a temporary worktree, same session.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| reactive_events_1000 | 12.19 ms | **9.91 ms** | 14.83 ms | **10.69 ms** |
| collections | 51.6 ms | 51.6 ms | 21.2 ms | **16.2 ms** |
| structs_match | 77.3 ms | 77.0 ms | 78.4 ms | 74.9 ms |

All other workloads stayed within noise.

- VM: recomputing a derived value cloned the node's whole IR (`Vec<Inst>` with its strings) on every recomputation. Node code is now shared (`Executor::node_code: Vec<Arc<[Inst]>>`).
- JIT: every entry (a derived value, a call, a handler) converted every reactive node into the slot buffer, then grew the buffer: about O(nodes²) per event. It now fills only the nodes the entry reads or assigns (`JitEngine::reads`), reuses one buffer for calls and derived values, and builds the emit table only when the handler emitted something.
- Correctness fixes found while profiling (tests in `flux_cli/tests/jit.rs`):
  - `emit` from a JIT-compiled function was lost (only handlers drained the queue). Such functions now run on the VM (`emits_events`).
  - An assignment to a source inside `if`/`while`/`for` in a JIT handler or watch was lost (only top-level assignments were read back). Those bodies now run on the VM (`conditional_source_assignment`).
  - Handlers and watches that are not eligible now report a reason.

The JIT remains slightly slower than the VM on `reactive_events`. There, each derived value is a single operation, and entering compiled code costs more than that operation.

## Findings

1. **Arrays were copied quadratically (fixed).** `Value::Array(Vec<Value>)` has full value semantics, so every read of a local array (`len(xs)`, `get(xs, i)`, passing it to a function) deep-clones it, nested structs included. `xs = push(xs, v)` copies it as well, because the old value is still held by `xs` during the call. Doubling the size roughly quadruples the time, for building (`push`: 500 → 0.3 s, 1,000 → 0.9 s, 2,000 → 3.6 s) and for indexed scanning (1,000 → 1.1 s, 2,000 → 4.2 s). Fix:
   - `Value::Array` is an `Arc<Vec<Value>>`, so reads and argument passing share it, and mutation copies on write.
   - `x = push(x, v)` on a local lowers to `Inst::Take x`, which moves the array out of the local, so `push` appends in place. This happens only when `v` does not read `x`. The JIT treats `Take` as ineligible and falls back to the VM.
   - Statement and loop runners release the previous statement's value before running the next one, so it no longer keeps the array shared.
2. The interpreter loop cost about 1.5 µs per iteration (`compute_loop`, turbo on). Half of that went to allocation and hashing; see the section above. Locals are still resolved by name.
