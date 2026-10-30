# Flux: general-purpose language roadmap

## Active objective for Claude Code

The active objective is to bring Flux to a dependable **Flux Core** milestone. This is the current task and takes priority over later roadmap phases:

1. Make the compiler robust on malformed and out-of-range source input; it must report diagnostics instead of panicking.
2. Make diagnostics from every imported module point to the correct file, source text, line, and column in CLI and editor workflows.
3. Make LSP positions follow UTF-16 code units and keep analysis fresh when imported files change.
4. Establish runtime value lifetime and allocation behavior; either implement safe reclamation or document and bound the MVP limitation.
5. Confirm cache invalidation and recovery from stale/corrupt cache entries.
6. Reconcile README, SPEC, ARCHITECTURE, CHANGELOG, CLI help, and implemented behavior; label experimental/stub backends accurately.
7. Define and meet a release gate for the usable Core workflow: create, check, run, format, use the REPL, and build a small reactive program.

After those are complete, continue to the general-purpose ergonomics phase in this file: validate the language with at least three ordinary non-reactive programs, then address only the real gaps they expose. Do not start package-registry work or add broad new syntax before Core is dependable.

### Persistent task state and continuation

- The durable goal is tracked in `.claude/flux_goal.json`; progress and the exact next action belong in `.claude/FLUX_CORE_PROGRESS.md`.
- At the start of every session, read both files and inspect `git status` and the current diff. Continue from the earliest incomplete acceptance item; do not redo completed work or overwrite unrelated/user changes.
- After each meaningful phase, update the progress file with completed work, files changed, commands and outcomes, unresolved risks, and one concrete next action.
- Before context compaction or ending a session, save that checkpoint. A compacted or resumed session must continue the same objective.
- Continue autonomously through routine, reversible repository work. Stop for user input only when a concrete decision cannot be inferred, a required external credential/service is unavailable, or the next operation is destructive or externally visible.
- The project Stop hook may request up to three automatic continuations per Claude Code session while the goal is active. This is bounded to prevent runaway usage. If it stops at the cap, leave a checkpoint; the next `claude` session should resume the same goal from the progress file.
- Mark the goal complete only when every Core acceptance criterion is actually met and recorded. Do not mark it complete because a session ended, a context limit was reached, or a task became difficult.

## Mission

Build Flux into a dependable general-purpose programming language. Reactive programming is a distinguishing capability, not the only kind of program Flux should support. A Flux user should be able to write ordinary command-line tools and small applications, use functions, modules, collections, errors, and a useful standard library, then opt into reactive state and events where they help.

The first target is a sound, usable language core, not maximal feature count. Stabilize semantics and the developer workflow before adding breadth. Treat the VM as the reference implementation. Keep JIT experimental until it passes semantic parity checks.

## Working agreement

- Read `README.md`, `SPEC.md`, `ARCHITECTURE.md`, and the relevant crate code before changing behavior.
- Follow the phases below in order. Complete one phase at a time; do not attempt a broad rewrite.
- Start each phase by checking the current working tree and preserving user changes. Do not overwrite unrelated modifications.
- Reproduce each defect with the smallest useful regression case before fixing it.
- Keep tests close to the behavior under test. Run focused tests first, then appropriate workspace checks. Report exact commands and outcomes.
- If the language specification and implementation disagree, describe the conflict and recommend a contract before changing semantics.
- Update docs only after behavior is established. Do not claim a feature is stable merely because its syntax parses.
- Do not add new language features while an earlier phase's acceptance criteria are failing.
- Do not remove existing features unless explicitly requested.

## Current audit status

### Completed locally: compiler failure reporting

The current working tree contains an uncommitted fix for malformed-source reporting. It adds `try_resolve_file_with_root` and retains source files for resolver failures, makes CLI compile failures return failure even when the diagnostic bag is empty, and adds tests for root/import syntax errors, missing files, and valid programs.

In the current working tree, the following checks passed after this fix:

- `cargo test --workspace --all-targets`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`

Do not redo this phase. Inspect and preserve its changes. Before any later phase, check `git status` and treat these edits as user work. Do not commit unless asked.

### Remaining confirmed or suspected issues

1. **VM/JIT semantic mismatch:** `let mut a = 1\nlet b = a / 0` previously yielded `Err("div by zero")` in VM and `inf` in JIT. Integer overflow and boundary behavior also need a shared contract.
2. **Imported-source diagnostics:** parser failures now retain their source, but type/name diagnostics that refer to imported modules may still be rendered against the root file. Audit diagnostic source selection across the full pipeline and LSP.
3. **LSP positions:** the current position conversion counts Unicode scalar values in at least one path; LSP positions use UTF-16 code units. Non-ASCII identifiers/text can shift ranges.
4. **LSP imported-file invalidation:** `DocumentStore` caches analysis by root document text. An imported file changing may leave diagnostics or symbol results stale.
5. **Runtime heap lifetime:** process-global maps store strings, arrays, and enums. There is no ordinary reclamation path visible in the current code. Measure this under repeated allocation before selecting a lifetime/ownership design; do not call it a proven leak without measurements.
6. **Compile cache:** confirm that cached IR is invalidated across compiler/schema changes and that failed or stale cache data always falls back to compilation.
7. **Docs and release scope:** README, SPEC, ARCHITECTURE, CLI help, and changelog mix implemented, experimental, and planned behavior. WASM currently emits only an empty valid module.
8. **CI/release evidence:** coverage is generated without a threshold; security audit is non-blocking; stable/release builds and supported targets need explicit MVP criteria.

## Product direction and staged target

The end goal is a practical general-purpose language whose core is pleasant for ordinary imperative/functional programs and whose reactive model is integrated consistently. Do not define success as “the language has many syntax features.” Define success as programs being understandable, predictable, debuggable, portable across supported machines, and easy to build and test.

### First dependable milestone: Flux Core

- reliable parsing, module resolution, type checking, compilation, and execution;
- primitives, bindings, functions, control flow, arrays, and imports;
- a precise type and runtime error model;
- a small standard library for I/O, strings, collections, and basic math;
- `init`, `check`, `run`, `fmt`, a usable REPL, useful diagnostics, and editor support;
- VM as the guaranteed execution engine;
- reactive sources, derived values, events, and watches as an explicit, tested language capability.

### General-purpose maturity after Core

- complete ordinary control/data programming patterns without requiring reactive constructs;
- reliable user-defined data types, pattern matching, generics, and trait/interface abstractions, with explicit compatibility and inference rules;
- module visibility, reusable packages, dependency/version management, and reproducible builds;
- testing support, useful stack traces, debugging hooks, documentation generation, and stable editor tooling;
- broader standard library and platform support based on real example programs;
- optional optimized backends (JIT first; WASM only when its supported subset and runtime contract are real and documented).

Do not start every maturity item at once. Add a capability only when there is a concrete program it enables, a defined semantic contract, tests, docs, and an acceptable diagnostic story. JIT remains opt-in/experimental until parity is demonstrated. Do not present a stub backend as usable output.

## Phase 1 — freeze the execution contract

### Tasks

1. Inventory numeric behavior in `flux_value/src/ops.rs`, `flux-runtime/src/exec/values.rs`, `flux_jit/src/compiler/emit.rs`, and type inference/promotion code.
2. Write a compact contract for:
   - integer and float division, including zero;
   - integer overflow and signed minimum divided by negative one;
   - numeric widths and promotion;
   - comparisons across supported numeric types;
   - casts that overflow or lose precision;
   - the representation of runtime errors and their propagation.
3. Add one table-driven test set for the contract in VM mode. Avoid tests that only restate helper implementation details.
4. Decide how division-by-zero is typed. Ensure runtime values do not silently violate the static type promised by the checker.
5. Make VM implement the documented contract. Preserve structured language errors; do not panic on user input.
6. Add JIT parity cases for every operation JIT claims to support. If exact parity cannot be implemented safely, make JIT eligibility reject/fall back for those operations and explain the fallback in stats.

### Acceptance

- VM tests cover normal values and edge cases, including zero division and overflow.
- Every JIT-eligible test produces the same language-level value/error as VM.
- Unsupported operations fall back instead of returning a different result.
- `cargo test -p flux_value`, `cargo test -p flux-runtime`, and `cargo test -p flux_cli --test jit` pass.

## Phase 2 — diagnostic source ownership

### Tasks

1. Trace `SourceId` from parsing through HIR, type checking, diagnostics, CLI rendering, and LSP conversion.
2. Find all callers that assume one `SourceFile` can render diagnostics for a multi-file program.
3. Introduce the smallest clear result shape that carries source files alongside diagnostics, or render each diagnostic using the source mapped by its `SourceId`.
4. Cover parser, undefined-name, type mismatch, duplicate declaration, import/export, and warning diagnostics in both root and imported modules.
5. Preserve the newly added resolver failure behavior and avoid losing warnings during failed resolution.

### Acceptance

- A diagnostic points to the correct path, line, column, and source snippet for root and imported files.
- Multiple diagnostics from different files render against their own contents in one invocation.
- Missing-file and cyclic-import diagnostics remain useful when no source text exists for the target.
- Focused resolver, driver, and CLI tests pass.

## Phase 3 — LSP correctness and freshness

### Tasks

1. Audit every conversion between LSP positions and byte offsets. Centralize conversion and implement UTF-16 code-unit rules in both directions.
2. Use the correct source contents for diagnostics, definitions, rename edits, and document symbols in imported files.
3. Include imported file versions/content fingerprints in analysis freshness, or explicitly invalidate all dependent open documents when an imported file changes.
4. Clear diagnostics on document close where required by the protocol.
5. Add tests using accented characters and astral-plane characters before/inside/after identifiers, plus an import edited while its root stays unchanged.

### Acceptance

- Hover, completion, definition, rename, formatting, and diagnostics point to the same symbols under UTF-16 positions.
- Updating an imported source refreshes the dependent root's diagnostics and symbols without editing the root.
- LSP tests exercise protocol-visible positions rather than only private conversion helpers.

## Phase 4 — runtime lifetime and stress behavior

### Tasks

1. Identify which heap values are interned for process lifetime and which are allocated per evaluation. Record allocation paths and consumers.
2. Build a repeatable stress harness that runs representative string/array/enum workloads and records live memory or heap entry counts over time.
3. Establish expected lifetime semantics: process-lifetime intern pool, executor lifetime, or explicit garbage collection/arena reset. Avoid invalidating tagged handles still reachable from another executor/thread.
4. Implement reclamation only after ownership and thread-safety requirements are clear.
5. Check overflow and unbounded loops in range/array allocation; return a language error or bound allocation where appropriate.

### Acceptance

- Repeated workload measurements show bounded memory for values no longer reachable, or documentation explicitly scopes the limitation for MVP.
- Concurrent executor use does not corrupt or reuse live handles.
- Stress tests exercise growth and teardown without relying on arbitrary sleeps.

## Phase 5 — cache validity and recovery

### Tasks

1. Audit IR and analysis cache keys, serialization versions, and read/write error behavior.
2. Include compiler/IR schema identity in the cache version/key so old binaries cannot interpret stale IR silently.
3. Treat corrupt, truncated, stale, or incompatible blobs as cache misses and recompile.
4. Avoid turning cache filesystem failures into compilation success with stale in-memory data.
5. Add tests for stale schema, truncation, corrupt bytes, dependency edits, and cache disabled mode.

### Acceptance

- Any invalid cache entry safely recompiles from source.
- Changing a dependency or cache schema cannot return old IR.
- Cache failure does not change program semantics or hide diagnostics.

## Phase 6 — MVP surface and documentation

### Tasks

1. Run every README command from a clean temporary project and note mismatches.
2. Verify `flux init` output and entry-point behavior from both project root and a subdirectory.
3. Label `--jit` and `wasm` honestly; a stub must not appear to have generated a runnable artifact.
4. Reconcile feature tables and roadmap across README, SPEC, ARCHITECTURE, CHANGELOG, CLI help, and crate docs.
5. Define what “stable MVP” means and list deferred features without claiming they are implemented.
6. Add or confirm a license file that matches the declared workspace license before any public release.

### Acceptance

- A new user can initialize, type-check, run, format, and inspect a small reactive app using only README instructions.
- Every documented command's result matches the documentation.
- Experimental/stub features are clearly identified at the command and documentation level.

## Phase 7 — CI/release gate

### Tasks

1. Make CI build and test the stated MVP targets on supported operating systems.
2. Decide whether Clippy should include all targets; fix or explicitly scope test-only lints.
3. Make security audit status visible and decide whether failures block release.
4. Publish a coverage baseline before adding a threshold; target compiler, resolver, runtime, and CLI error paths first.
5. Add a clean release build and quick-start smoke test.

### Acceptance

- CI verifies format, lint, full tests, release build, and quick-start behavior on supported platforms.
- Security and coverage status are visible and have explicit owners/threshold decisions.
- A release candidate is reproducible from a clean checkout.

## Phase 8 — ordinary general-purpose programming ergonomics

Start this phase after Phases 1–7 have a reliable Core baseline.

### Tasks

1. Collect 3–5 representative ordinary programs (for example: a command-line text processor, a small data transformation tool, and a config-driven utility) and identify gaps by trying to write them in Flux.
2. Define the minimum error-handling model: typed `Result`/`Option` behavior, propagation ergonomics, and whether runtime faults are recoverable. Make CLI/library failures observable and composable.
3. Specify module visibility, naming, initialization, and dependency rules before growing package support.
4. Audit generics, structs/enums, match exhaustiveness, traits, and method dispatch. Keep only behavior with clear type-checking and diagnostic rules in the stable core.
5. Add library-facing APIs and a standard-library organization that users can discover without memorizing built-in names.

### Acceptance

- At least three materially different non-reactive programs can be implemented without awkward language workarounds.
- Error handling is explicit and type-safe for expected failures such as file and parse errors.
- Reusable code can be split into modules with predictable visibility and import behavior.
- Each retained advanced feature has positive and negative tests and clear diagnostics.

## Phase 9 — project/package workflow and ecosystem

Only begin after users can write and test multiple-file programs reliably.

### Tasks

1. Define package identity, dependency source/version, lockfile, and reproducible resolution requirements.
2. Add a build/test workflow that works from a clean checkout and reports actionable failures.
3. Provide standard project layouts and examples for both CLI and reactive programs.
4. Decide versioning and compatibility policy for the language, standard library, compiler, and package format.
5. Publish package tooling only after security, integrity, cache, and offline behavior are specified.

### Acceptance

- A user can create a project, add a dependency, build/test it reproducibly, and understand failures from diagnostics.
- A lockfile pins resolved dependencies and is honored by clean builds.
- At least one documented sample consumes a reusable local/package dependency.

## Phase 10 — optimization and additional targets

### Tasks

1. Measure VM performance on representative programs before optimizing.
2. Make JIT eligibility explicit and verify output, errors, and observable effects against VM.
3. Keep unsupported IR on a safe fallback path and expose why a fallback happened.
4. For each additional backend (including WASM), document the supported language subset, runtime/host APIs, and failure behavior before exposing a success-shaped command.
5. Add a backend only when it has end-to-end examples, parity/validation tests, and a release target.

### Acceptance

- Optimizations improve measured workloads without changing program semantics.
- Backend limitations are machine-checkable where possible and clearly diagnosed.
- No backend reports success for code it silently omitted.

## Execution policy for follow-up requests

When asked to continue, inspect the current diff and identify the earliest unfinished phase. Implement only that phase unless the user names another. Keep changes reviewable, run focused checks and appropriate broad checks, and finish with a concise report of behavior changed, tests run, remaining risks, and the next phase. Never bundle arithmetic semantics, LSP changes, memory redesign, package management, and documentation cleanup into one patch.
