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
- [ ] Measure runtime heap growth and choose/document a safe lifetime policy.
- [ ] Confirm cache schema/versioning and stale/corrupt entry fallback.
- [ ] Reconcile user-facing docs and accurately label experimental and stub features.
- [ ] Define and satisfy the Core release gate with concrete CLI and reactive examples.
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

## Next action

Phase 4 (runtime lifetime): inventory the process-global heap tables in `flux_value/src/heap.rs` (strings, arrays, enums), list their allocation paths and consumers, then build a repeatable stress harness that counts heap entries across repeated string/array/enum workloads. Measure before choosing a policy (intern pool, per-executor arena, or GC), and don't call it a leak without numbers.
