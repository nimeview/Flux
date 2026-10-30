# Flux Core progress checkpoint

Status: active

## Goal

Finish a dependable Flux Core milestone before expanding the language: robust compiler errors, correct multi-file diagnostics, correct and fresh LSP results, explicit runtime lifetime behavior, reliable cache recovery, accurate documentation, and a repeatable Core workflow. Then validate ordinary non-reactive programs and address the gaps they expose.

## Starting point

- Existing roadmap and phase details are in `CLAUDE.md`.
- The working tree already contains extensive uncommitted implementation changes. Preserve them; inspect the current diff before editing.
- A prior read-only review identified likely remaining issues: numeric literal parsing can panic on oversized input; imported-source CLI diagnostics may use the root source; LSP offsets count Unicode scalar values instead of UTF-16 units; imported-file changes may not invalidate root analysis; process-global runtime heap tables have no normal reclamation path; version dependencies are accepted by the manifest shape but not resolved; WASM emits only an empty module; docs disagree on version/stability.
- These are leads to verify against the current code, not permission to assume every item is still open. Reproduce each relevant issue and update status based on evidence.

## Phase checklist

- [x] Audit the current diff and verify the earliest incomplete acceptance items. (2026-10-31: pre-existing uncommitted diff passes `cargo test --workspace --all-targets`.)
- [x] Finish execution and numeric error semantics; VM is the reference backend and JIT must match or fall back. (2026-10-31: `numeric_contract.rs` covers zero division, overflow, `MIN / -1`, widths, casts, comparisons, error propagation on VM and JIT with a native-or-fail parity check; contract now written in SPEC §2 "Numeric semantics".)
- [x] Ensure malformed source and out-of-range literals produce diagnostics without compiler panic. (Spot-checked 2026-10-31: oversized ints, `i64::MIN`, bad `u64`, unterminated strings/arrays, truncated exprs all give diagnostics, exit 1, no panic.)
- [x] Carry source files through diagnostics so root and imported-file errors render correctly. (CLI + LSP, 2026-10-31; see below.)
- [x] Correct LSP UTF-16 position conversion and invalidate analysis when dependencies change. (2026-10-31; see below. Limitation: unsaved edits in an open imported buffer are not seen until saved.)
- [x] Measure runtime heap growth and choose/document a safe lifetime policy. (2026-10-31: measured leak, scoped reclamation implemented, policy in ARCHITECTURE; see below.)
- [x] Confirm cache schema/versioning and stale/corrupt entry fallback. (2026-10-31; see below.)
- [x] Reconcile user-facing docs and accurately label experimental and stub features. (2026-10-31: quick-start fixed and tested; VM default confirmed by user; MIT LICENSE added for nimewe, 2026.)
- [ ] Define and satisfy the Core release gate with concrete CLI and reactive examples. (2026-10-31: gate defined in RELEASE.md + CI; satisfied locally except audit/coverage which need network tools; audit policy decided: warn-only; needs a green CI run on GitHub.)
- [ ] Validate at least three materially different ordinary, non-reactive programs; record remaining language gaps.

## Latest completed work

### 2026-10-31 — Phase 2: diagnostic source ownership (CLI + LSP)

Reproduced first: an undefined name or type mismatch inside an imported module rendered against the root file (`Failed to read contents … OutOfBounds`); a missing import module produced a generic `failed to resolve main.fl` at 1:1; cyclic-import errors pointed at offset 0 of the root; LSP converted imported-file spans using the open buffer's text and published them on the root URI.

Behavior changes:
- Every diagnostic's `source_id` now comes from its own span (type checker, monomorphizer, HIR duplicate/unknown-export, IR cycle, runtime binding errors) instead of a root id threaded through calls. The now-unused `source` parameters were removed from internal helpers; `monomorphize_program` keeps `_source` for API stability.
- `flux_driver`: `CompileFailed { sources, diagnostics }` (primary first, `source()` accessor), `Compiled.sources`, `Checked.sources`, `Analyzed.sources`. LSP analysis now uses `try_resolve_file_with_root` and keeps the failing file's source; it no longer falls back to single-file analysis after a resolution failure.
- `flux_cli` output: `report_errors/report_warnings/report_failure(primary, sources, bag)` render each diagnostic against the source with its id.
- `flux_resolve`: missing modules report `module not found: <path>` at the `import` (previously silently swallowed by `canonicalize().ok()?`); cyclic-import and read errors point at the closing `import` statement.
- LSP: `RangedDiagnostic.path` marks imported-file diagnostics; the server publishes them to the imported file's URI (skipping files that are open), clears imported URIs that no longer have diagnostics, and clears the document's and its imports' diagnostics on `didClose`. JSON `flux lsp <file>` output adds an optional `file` field.

Files: `flux_driver/src/pipeline.rs`, `flux_cli/src/{output,main,repl}.rs`, `flux_resolve/src/lib.rs`, `flux_hir/src/lower.rs`, `flux_hir/src/lower/{modules,stdlib}.rs`, `flux_ir/src/lower.rs`, `flux_types/src/{mono.rs,check.rs,check/**}`, `flux_lsp/src/{analysis,documents,server,lib}.rs`, `flux_lsp/Cargo.toml` (tempfile dev-dep).

Tests added: `flux_cli/tests/multi_file_diagnostics.rs` (7: undefined name in import, root+import errors in one run, duplicate in import, unknown export, root warning with imports, cyclic import, missing module); `flux_lsp/tests/server.rs` (2: imported error on its own URI with exact range + clearing when import removed; didClose clears root and import).

Commands (all pass):
- `cargo test --workspace --all-targets` → 354 passed, 0 failed
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Unresolved risks:
- Unused-binding warnings are not produced for imported modules at all (only the root); decide whether that is intended.
- `300u8` is rejected only at runtime (`numeric cast out of range`), not as a compile-time literal diagnostic.
- LSP positions still count Unicode scalars, not UTF-16 (Phase 3). Imported-file edits still don't invalidate the root's cached analysis (Phase 3).
- Nothing committed.

### 2026-10-31 — Phase 1 closure + Phase 3: LSP UTF-16 and freshness

Phase 1: re-audited `flux_cli/tests/numeric_contract.rs` against the Phase 1 acceptance list and added the compact contract to `SPEC.md` (§2 "Numeric semantics"). Spot-checked the claims on the CLI: `1 / 0` → `runtime error: integer division by zero`, exit 1 on both `--vm` and `--jit`.

Phase 3, reproduced first: `flux_diagnostics::offset_to_line_col` returns *byte* columns while `position_to_offset` counted Unicode scalars, so the two directions disagreed and both were wrong for UTF-16. `span_to_lsp_range` read the root file from disk even when the open buffer had unsaved edits. `DocumentStore` reused an analysis while the root text was unchanged, even after an import changed.

Behavior changes:
- New `flux_lsp/src/position.rs`: the single place for `offset_to_position`, `position_to_offset`, and `byte_range_to_lsp`, all in UTF-16 code units, with snapping for surrogate halves and out-of-range positions. Diagnostics, definition, outline, rename edits, hover ranges, completion prefix, incremental edits, and formatting now all go through it. `RangedDiagnostic` and the JSON `flux lsp` columns now count UTF-16 units.
- Definition, outline, and hover use the open buffer for SourceId 0 instead of the on-disk file.
- `DocumentStore` stores a content hash for every imported file it read. An entry is stale if any of those files changed, and failed resolutions are never reused. The server advertises `save`; on `didChange` or `didSave` it re-publishes open documents that import the changed file.

Tests added: `position.rs` unit tests (round-trip, UTF-16 counts, clamping); `flux_lsp/tests/server.rs`: `diagnostic_range_counts_utf16_units`, `rename_edits_use_utf16_columns`, `incremental_edit_after_astral_char_uses_utf16`, `saving_an_import_refreshes_the_unchanged_root`. The last one was confirmed to fail with the dependent re-publish disabled.

Commands (all pass): `cargo test --workspace --all-targets` → 361 passed, 0 failed; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`.

Unresolved risks:
- The resolver reads imports from disk, so unsaved edits in an *open* imported buffer don't affect dependents until saved. Fixing this needs a resolver overlay for open buffers.
- Identifiers are ASCII-only in the grammar; non-ASCII appears only in strings and comments, which is what the tests use.
- `analysis.rs::rename_occurrences` (dead legacy helper) still counts scalars.
- Hover and completion have no dedicated non-ASCII protocol test; they share the converted `position_to_offset`.
- Incident: a `git stash push` on `flux_lsp/src/{documents,server}.rs` ran by mistake and was immediately popped. Both files contained only this session's edits; the pop restored them fully (177 insertions).
- Nothing committed.

### 2026-10-31 — Phase 4: runtime heap lifetime

Inventory: `flux_value::heap` keeps process-global maps for strings (interned, so deduplicated), enums, and arrays. The VM never uses them; it works with owned `Value`s. Only the JIT boundary does (opt-in via `--jit`/`FLUX_JIT=1`/`[run] jit`; VM is the current default) (`value_to_tagged` for slots and arguments, plus host functions `make_enum`, `array_finish`, `range`, `string_concat`, `call_builtin`). Compiled code embeds string-constant ids interned at compile time. Tagged handles otherwise live only for one runtime↔JIT exchange: results, emits, and handler slots are converted back to owned values right after the call.

Measurement first (`flux_cli/tests/heap_lifetime.rs`; JIT handler with an array, an enum, and a growing string per event): before the fix, enums and arrays went 100 → 2100 over 2000 events, i.e. +1 each per event, unbounded. Strings stayed at 7 in that variant.

Policy and implementation:
- `HeapScope` / `heap_scope()`: a thread-local, nestable RAII scope. Arrays and enums allocated in a scope are freed when the outermost scope ends. Strings carry per-id scope reference counts, and a string interned outside any scope is pinned (compile-time constants). Ids are never reused.
- Scopes are opened in `JitBackend::try_eval_derived` and `try_call_fn`, and in `Executor::try_run_jit_on` / `try_run_jit_watch`. The handler scopes stay open until slots are read back.
- Added `heap_stats()` / `HeapStats` for measurement.
- Measured after the fix: arrays 0 → 0 and enums 0 → 0 over 2000 events; strings do not grow even with a new distinct string each event.

Allocation bounds: `range` now goes through `flux_value::int_range`, which computes lengths in i128 (no overflow; the old VM `i += step` could panic near `i64::MAX`). It is capped at `MAX_COLLECTION_LEN` = 10,000,000, and exceeding it is the new runtime error `ArithError::AllocationLimit` ("allocation limit exceeded") on both VM and JIT. The JIT now aborts on errors returned by the `range` and `call_builtin` host calls.

Found while testing, and fixed: the empty string literal `""` parsed as an interpolation with no parts, so `let mut s = ""` read inside `watch`/`on` failed with "undefined name `symbol #SymbolId(n)`". Regression test: `core_contract::empty_string_source_is_readable_in_handlers`.

Tests added: `flux_value/tests/heap.rs` (3 scope-semantics tests); `flux_cli/tests/heap_lifetime.rs` (bounded pools over 2000 events, JIT == VM results, 4 concurrent JIT executors with correct values and no leftover arrays/enums); 4 `range` cases in `numeric_contract.rs` (VM and JIT parity); the empty-string regression.

Commands (all pass): `cargo test --workspace --all-targets` → 368 passed, 0 failed; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`.

Unresolved risks / findings:
- Not reproduced against the pre-fix code: the claim that unbounded distinct strings would have grown without scopes is reasoned (each `value_to_tagged` interned outside a scope was pinned), not measured.
- `flux_jit::host` emit state is a global Mutex, so concurrent handlers that `emit` on different threads could interleave queued emits. The concurrency test avoids `emit`. Pre-existing and not addressed.
- String interpolation `"v{n}"` with a non-String `n` is a type error, because SPEC defines interpolation as plain concatenation. This is a Phase 8 ergonomics gap.
- ARCHITECTURE still says "VM remains the default executor", which is stale (Phase 6).
- The Phase 4 changes are not committed (HEAD is `30a4bc7`).

### 2026-10-31 — Phase 5: compile cache validity and recovery

Audit findings (reproduced from the code and on disk):
- Compiler identity was a manual `CACHE_VERSION` byte only (2). A rebuilt compiler with changed lowering but a decodable IR layout would silently reuse old IR.
- There was no payload checksum. A corrupted byte that still bincode-decodes would run as a different program.
- Cached IR that decoded but failed executor validation surfaced as a `VM validation failed` diagnostic instead of recompiling.
- In-memory `compile()` (REPL, tests) used the raw source text as the cache key, and therefore as the file name. `flux_cli/.flux/cache/` holds 263 such files with multi-line program text as names. They are git-ignored and left in place; the new code never reads them.
- Already fine: the key covers every module's path and contents, so a dependency edit causes a miss (existing tests); filesystem write failures don't change semantics; the analysis cache is in-process and keyed the same way.

Changes:
- `flux_driver/build.rs` emits `FLUX_COMPILER_FINGERPRINT`, a hash of `src/**/*.rs`, `*.pest`, and `Cargo.toml` for syntax, diagnostics, hir, types, ir, resolve, std, value, and driver, re-run when any of them changes. `COMPILER_FINGERPRINT` scopes the on-disk file name and is stored in the blob header.
- Blob layout v3: magic, version, fingerprint length and bytes, SHA-256(payload), payload. `deserialize_program` rejects any mismatch or truncation.
- `emit_ir` validates cached IR with the now-public `flux_runtime::validate_program`; an invalid entry is invalidated and rebuilt.
- A missing cache key now falls back to `hash_source(contents)` instead of the raw text.
- ARCHITECTURE gains an "IR cache contract" paragraph.

Tests: `flux_cli/tests/cache_recovery.rs` (7): every single-byte flip rejected, truncation at every length, other-compiler fingerprint and v2 layout rejected, on-disk garbage/empty/truncated/flipped entries recompiled and replaced, decodable-but-invalid IR rebuilt, `--no-cache` neither reads a wrong-but-valid entry nor writes, cache file names are 64-hex hashes for both file and in-memory compiles.

Commands (all pass): `cargo test --workspace --all-targets` → 375 passed, 0 failed; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`.

Unresolved risks:
- That the fingerprint changes on a source edit relies on cargo `rerun-if-changed` plus content hashing. It was not checked by editing a compiler file and rebuilding.
- The fingerprint uses `std` `DefaultHasher`, which is stable within one toolchain; a toolchain change yields a different binary and fingerprint anyway.
- Phases 4 and 5 are not committed (HEAD is `30a4bc7`).

### 2026-10-31 — Phase 6: MVP surface (quick-start) — mostly done

Ran the README and quick-start flow from a clean temp directory (root and subdirectory) and round-tripped `flux fmt` over every valid program in `programs/ok`, `programs/demo`, and `examples`, working on copies in the scratchpad. Mismatches found and fixed:
- **`flux fmt` corrupted or lost code.** It dropped every comment. It emitted `on E { {…} }`, which does not parse, for every handler. It added trailing commas in struct and enum bodies, which do not parse. It printed `2.0` as `2`, silently turning a Float into an Int. It dropped `as alias` on imports. Before the fix, 8 repo programs failed to type-check after formatting. Now comments between items, same-line trailing comments, and end-of-file comments are kept, including a blank line after a comment. A comment inside a declaration makes fmt refuse and leave the file unchanged. Empty-param events print without `()`.
- `print`/`println` printed strings with quotes (`"hi"`); they now print the raw text. Nested values keep their display form.
- `flux run`/`check` without a file only looked at `./flux.toml`; they now search parent directories, and so does the `[run] jit` lookup.
- The `flux init` template warned on first run (`let mut x = 0` unused). The new template uses `import io`, an event, a source, a derived value, an `on` handler, and a `watch`, and is warning-free and fmt-stable.
- `flux wasm` printed `wasm backend: Stub, 8 bytes` with exit 0 and *ran the program* (side effects, cache writes). It now type-checks and lowers only, writes nothing, and exits 1 with an "experimental stub … nothing was written" error. Its help text says it is a stub that always fails.
- The CHANGELOG [Unreleased] "Fixed" section lists this session's behavior changes.

Verified: all 72 valid programs still type-check after fmt, print identical stdout (VM), and format idempotently.

Tests: `flux_cli/tests/quickstart.rs` (init → check without warnings → run → nested-dir run → fmt no-op; wasm fails and writes nothing; missing-manifest message); `flux_cli/tests/fmt_roundtrip.rs` (all repo programs type-check after fmt and fmt is idempotent); 3 new `flux_fmt` comment tests.

Commands (all pass): `cargo test --workspace --all-targets` → 382 passed, 0 failed; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`.

Incident: my first round-trip loop ran repo programs in place. `programs/ok/io/cat.fl` wrote `programs/ok/io/cat_input.txt`, and an interrupted iteration left `programs/ok/io/.__rt.fl`. Both were created by me and deleted; later runs used scratchpad copies with stdin from /dev/null.

**Blocked on user decisions:**
1. **LICENSE file:** the workspace declares `license = "MIT"`, but the repo has no LICENSE file. MIT needs a copyright holder name and year; ask the user.
2. ~~Default engine~~ **Decided 2026-10-31: keep the VM as the default** ("Оставить VM"). JIT stays opt-in. Code and docs already match, so nothing changes.

Phase 8 findings (do not fix in Core): string literals have no escape sequences (`"\n"` is a literal backslash-n; SPEC defines none); interpolation `"{n}"` with a non-String is a type error by spec.

### 2026-10-31 — Phase 6 follow-up: README feature table and versions

- Feature table: every row is exercised by a program in `programs/ok`, `programs/demo`, or `examples`, and those already run in the regression tests. The one exception was `continue`: `break_continue.fl` contains no `continue`. Verified that it works on the VM and on the JIT (JIT falls back to the VM for that function, same value). Added `core_contract::continue_skips_iteration_on_vm_and_jit`.
- Versions disagreed (Cargo 0.1.0, REPL banner "v0.6", README "v0.4+", SPEC "v1.0 draft" / "stable v1.0"). Removed the unfounded README/SPEC labels, and the REPL banner now uses `CARGO_PKG_VERSION`. The CHANGELOG heading "[Unreleased] — v0.4" is left as is: choosing the release number is the user's call.
- Engine default decided by the user: keep the VM (memory updated).

Commands: `cargo test --workspace --all-targets` → all pass (see the count in the session report); `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`.

Phase 6 closed: `LICENSE` (MIT, "Copyright (c) 2026 nimewe", user-provided) added; README links it.

### 2026-10-31 — Phase 7: CI / release gate

Audit of `.github/workflows/ci.yml`: clippy linted no tests or benches; `cargo audit` had `continue-on-error: true`, so failures were invisible; coverage ran twice on nightly and published nothing; there was no quick-start smoke test on the release build; nothing used `--locked`.

Changes to `ci.yml`:
- `test`: `--all-targets --locked`.
- New `release` job (ubuntu-latest, macos-15): `cargo build --workspace --release --locked`, `cargo test -p flux_cli --release --test quickstart`, and a shell smoke test of the release binary (`init`/`check`/`run`).
- `jit`: also runs `numeric_contract`.
- `clippy`: `--all-targets --locked -D warnings`.
- `coverage`: stable toolchain, a single run, summary in `$GITHUB_STEP_SUMMARY`, `lcov` artifact, no threshold.
- `audit`: still non-blocking, but its output always goes to the run summary and a failure raises a `::warning::` annotation.

`RELEASE.md` defines the gate (blocking vs visible checks), the supported platforms (Linux and macOS; Windows not part of the MVP), and the stable / experimental / stub / deferred scope; README links it.

Verified locally (all pass): release build `--locked`; `cargo test -p flux_cli --release --locked --test quickstart` (3/3); the release-binary shell smoke; `flux_jit` and `flux_cli --test jit --test numeric_contract` with `--test-threads=1`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; the workflow YAML parses (Ruby YAML). The earlier `cargo test --workspace --all-targets` passed: 383.

Not verified locally: the `audit` and `coverage` jobs (`cargo-audit` and `cargo-llvm-cov` are not installed and were not fetched); no GitHub Actions run has happened yet (nothing pushed); actionlint is unavailable.

Decided by the user (2026-10-31): a `cargo audit` failure **warns only and does not block** a release. Recorded in RELEASE.md and in the ci.yml comment.

## Next action

Push or open a PR only when the user asks, then read the first CI run and fix any job that fails on GitHub, especially `audit` and `coverage`, which never ran locally. Once CI is green, mark the release-gate item and the Core goal complete in this file and in `.claude/flux_goal.json`. Then start Phase 8: write three ordinary non-reactive programs (a text processor, a data transform, a config-driven utility) and record the gaps. String escapes and non-String interpolation are already known.
