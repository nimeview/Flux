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

## 32-byte `Value` (2026-11-02)

Turbo Boost off. A/B against 497e8e4 in a temporary worktree, same session.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| compute_loop | 241 ms | **110 ms** | 2.21 ms | 2.25 ms |
| recursion | 36.9 ms | **27.9 ms** | 245 µs | 242 µs |
| strings | 32.1 ms | **25.3 ms** | 28.3 ms | **25.7 ms** |
| collections | 54.6 ms | **38.3 ms** | 17.8 ms | 15.8 ms |
| structs_match | 83.8 ms | 83.8 ms | 83.1 ms | 85.6 ms |
| enum_match | 1.58 s | **1.36 s** | 296 ms | 297 ms |
| reactive_events_1000 | 11.0 ms | 9.6 ms | 12.2 ms | 11.5 ms |
| compile_inventory_totals | 2.10 ms | 2.14 ms | 2.15 ms | 2.30 ms¹ |

¹ The confidence intervals overlap (2.03–2.25 vs 2.15–2.52 ms). Compilation does not move `Value`s.

The `sample` profiler showed the VM loop's hottest instruction was `rep movsq`. Each operand push and pop moved a 72-byte `Value`, and at that size the move compiles to a block copy, which is slow to start on this Intel CPU. The size came from the inline variants: a struct's `HashMap` and an enum's two `String`s. Both are now boxed (`Value::UserStruct(Box<StructValue>)`, `Value::UserEnum(Box<EnumValue>)`; an enum payload sits inside the same box). `Value` is now 32 bytes, its largest variant being `String`, and a test keeps it that way. Two other changes:
- Integer (`Int64`) arithmetic and comparisons on the operand stack are computed in place. They use the same `flux_value::binary_i64` as before, so results and errors are unchanged.
- Tried and rejected: replacing the hashed locals map with a linear-scan vector made no measurable difference (21.5 s vs 21.4 s on a 20M-iteration loop), so locals stay hashed.

## Lazy stack-pop context (2026-11-02)

Turbo Boost off. A/B against the 32-byte `Value` tree in a temporary worktree, same session.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| enum_match | 1.31 s | **958 ms** | 255 ms | 263 ms |
| structs_match | 83.4 ms | **66.9 ms** | 86.4 ms | **67.8 ms** |
| recursion | 27.3 ms | **24.9 ms** | 243 µs | 236 µs |
| strings | 25.2 ms | 23.6 ms | 25.2 ms | 23.1 ms |
| collections | 38.3 ms | 36.4 ms | 15.4 ms | 14.8 ms |
| reactive_events_1000 | 9.3 ms | 8.7 ms | 11.3 ms | 10.3 ms |
| compute_loop | 109 ms | 110 ms | 2.21 ms | 2.23 ms |
| compile_inventory_totals | 2.12 ms | 2.15 ms | 2.26 ms | 2.26 ms² |

² Re-run alternating base and new twice. The sequential run showed +20%, which was thermal drift late in the run.

Several instructions passed an error-message context to `stack_pop` as `&format!(...)`: `BindLocal`, `VariantTagEq`, `EnumTagEq`, `FieldGet`, struct construction, `MakeArray`, and `Cast`. The message was allocated on every execution even though it is used only on stack underflow (an internal invariant). These sites now pass a closure (`stack_pop_with` / `stack_pop_opt_with`), so the message is built only when underflow actually happens. The JIT columns move because the JIT falls back to the VM for those parts.

## Shared structs and enums (2026-11-02)

Turbo Boost off. A/B against the lazy-context tree (structs and enums boxed) in a temporary worktree, same session.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| structs_match | 67.8 ms | **23.1 ms** | 68.2 ms | **24.1 ms** |
| enum_match | 944 ms | **640 ms** | 301 ms | 298 ms |

All other workloads stayed within noise (±5%).

`sample` on enum_match showed most of the time in `malloc`/`free`. Every read of a local that held an enum (`step(op, acc)`) cloned its box and both name strings. Structs and enums never change after they are built, so they are now shared like arrays: `Value::UserStruct(Arc<StructValue>)` and `Value::UserEnum(Arc<EnumValue>)`. A copy now only bumps a reference count.

## Reused locals maps (2026-11-02)

Turbo Boost off. A/B against the shared-structs tree in a temporary worktree, same session.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| recursion | 25.9 ms | **19.4 ms** | 242 µs | 235 µs |
| collections | 37.9 ms | **28.2 ms** | 15.4 ms | 14.5 ms |
| enum_match | 630 ms | **530 ms** | 299 ms | 298 ms |
| structs_match | 22.9 ms | **19.4 ms** | 23.7 ms | **20.3 ms** |
| strings | 24.9 ms | 23.1 ms | 25.5 ms | 22.7 ms |
| compute_loop | 103 ms | 101 ms | 2.21 ms | 2.22 ms |
| reactive_events_1000 | 10.0 ms | 9.6 ms | 11.7 ms | 11.6 ms³ |
| compile_inventory_totals | 2.20 ms | 2.22 ms | 2.21 ms | 2.32 ms³ |

³ Re-run alternating base and new twice; the medians are shown. Compilation does not use the changed code. The JIT row's +4% is unexplained and probably reflects code layout.

Before this change, each interpreted call allocated a locals hash table and freed it on return, and every `let`, `for` variable, and pattern binding allocated a fresh key string, even when the name was already bound (a `let` in a loop body). Calls now reuse cleared maps from a pool (`Executor::locals_pool`, which keeps at most 64 maps). `Locals::bind` allocates a key only for a new name.

Second step, A/B against the first step:

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| recursion | 20.8 ms | **14.4 ms** | 241 µs | 237 µs |
| collections | 25.4 ms | **18.2 ms** | 13.6 ms | 13.8 ms |
| structs_match | 18.1 ms | **15.2 ms** | 18.2 ms | **16.1 ms** |
| enum_match | 493 ms | **377 ms** | 256 ms | 259 ms |

All other workloads stayed within noise.

After the first step, half of enum_match's time was still in `malloc`. Clearing a map freed its keys (the parameter names and the `Add(n)` pattern bindings), and the next call allocated them again. A reused map now keeps its keys and only unbinds the values (`Locals` maps a name to `Option<Value>`, and a key without a value behaves like a missing one). A map with more than 32 keys is cleared instead, so unbinding stays cheap. Rejected alternative: one pool per function, keyed by the function's address. It cost two hash lookups per call and made collections JIT 5% slower, confirmed on alternating re-runs. Test: `reused_call_locals_do_not_leak_between_calls`.

## JIT enum lookups; structs stay on the VM (2026-11-02)

Turbo Boost off. A/B in a temporary worktree, same session.

| Workload | JIT before | JIT after |
|----------|------------|-----------|
| enum_match | 256 ms | **134 ms** |

All other workloads stayed within noise. The ones that looked slower late in the sequential run (collections, reactive_events, compile JIT) matched on two alternating re-runs.

Compiled `match` on an enum called the host per arm (`flux_jit_enum_variant_eq`) and per payload (`flux_jit_enum_payload_get`). Each call locked the heap mutex twice and copied the enum's two name strings plus the variant name being compared. `flux_value::enum_variant_is` and `enum_payload` now take one lock and copy nothing.

**Tried and rejected: compiling structs in the JIT.** Struct support was implemented and passed the parity tests: a `TAG_STRUCT` heap record, struct construction and field reads through the host, and struct-variant patterns. It was nevertheless slower on `structs_match`: JIT 16.1 → 22.4 ms, and still 18.5 ms after the lookup fix above, against 15.9 ms on the VM path. On every call into compiled code the struct (and the enum around it) had to be copied into heap-pool records under the mutex, field names included, and every field read was another locked host call. The interpreter now reads an `Arc`-shared struct directly, which costs less than that round trip. So code that uses structs keeps running on the VM under `--jit`. Making the JIT faster for structs needs a different value representation in compiled code: handles that point at the shared `Value` instead of copies in the mutex-guarded pools. `jit_parity_structs_and_struct_variant_fields` checks VM and JIT results on that fallback path.

## Slot locals (2026-11-02)

Turbo Boost off. A/B against 25dd765 in a temporary worktree, same session.

| Workload | VM before | VM after | JIT before | JIT after |
|----------|-----------|----------|------------|-----------|
| compute_loop | 101 ms | **64 ms** | 2.20 ms | 2.21 ms |
| enum_match | 387 ms | **298 ms** | 135 ms | 138 ms |
| structs_match | 15.3 ms | **12.9 ms** | 16.5 ms | **13.8 ms** |
| recursion | 15.2 ms | **13.2 ms** | 239 µs | 242 µs |
| collections | 18.1 ms | **16.1 ms** | 14.0 ms | 15.1 ms⁴ |
| strings | 23.1 ms | 22.0 ms | 22.8 ms | 22.5 ms |
| reactive_events_1000 | 8.9 ms | 8.6 ms | 10.5 ms | 10.1 ms |
| compile_inventory_totals | 1.96 ms | 1.94 ms | 2.02 ms | 1.98 ms |

⁴ Confirmed on two alternating re-runs (+8%), with the same functions compiled. On the same program at 2,000,000 elements (`flux run --jit`, two runs each), the new build is faster: 1.42 s vs 1.50 s. So the gap is a fixed cost or code layout, not a per-element one.

Locals used to be keyed by name: every `Load`, `let`, and assignment hashed the name and compared strings. The new pass `flux_ir::resolve_locals` gives each body (function, `on`, `watch`, derived expression, module init) a frame. Parameters take slots `0..n`, and every other name the body binds gets the next slot. Local reads become `Inst::LoadLocal { slot, name }` (and `TakeLocal`), and `let`, `for`, `BindLocal`, and assignments carry their slot. The VM frame is a `Vec<Option<Value>>`.

Behavior is unchanged by construction: a slot is unbound until its binding runs, and an unbound slot reads `name` as a global, exactly as a missed name lookup did. An assignment to a reactive source has no slot and still goes by name, because the VM checked sources first. The pass runs at the end of lowering and again in `Executor::with_mode`, and it is idempotent, so hand-built IR works too. Cached IR is invalidated automatically, because the compiler fingerprint hashes `flux_ir`. The JIT treats the slot forms like their name forms. Tests: `flux_ir/tests/resolve.rs`, plus the existing VM/JIT parity and differential suites.

## Findings

1. **Arrays were copied quadratically (fixed).** `Value::Array(Vec<Value>)` has full value semantics, so every read of a local array (`len(xs)`, `get(xs, i)`, passing it to a function) deep-clones it, nested structs included. `xs = push(xs, v)` copies it as well, because the old value is still held by `xs` during the call. Doubling the size roughly quadruples the time, for building (`push`: 500 → 0.3 s, 1,000 → 0.9 s, 2,000 → 3.6 s) and for indexed scanning (1,000 → 1.1 s, 2,000 → 4.2 s). Fix:
   - `Value::Array` is an `Arc<Vec<Value>>`, so reads and argument passing share it, and mutation copies on write.
   - `x = push(x, v)` on a local lowers to `Inst::Take x`, which moves the array out of the local, so `push` appends in place. This happens only when `v` does not read `x`. The JIT treats `Take` as ineligible and falls back to the VM.
   - Statement and loop runners release the previous statement's value before running the next one, so it no longer keeps the array shared.
2. The interpreter loop cost about 1.5 µs per iteration (`compute_loop`, turbo on). Half of that went to allocation and hashing; see the sections above. Moving 72-byte values was the next largest cost (fixed by the 32-byte `Value`). Locals are now frame slots (see "Slot locals").
