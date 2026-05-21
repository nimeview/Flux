# Flux

Flux is an experimental reactive programming language with a split architecture:

- `compiler/` (Rust): parser, HIR, semantic analysis, module loading, bytecode emission.
- `runtime/` (C++): bytecode decode/execute, dependency graph, scheduler, tracing.
- `stdlib/` (C++): runtime built-in functions.
- `shared/` (C++ headers): cross-language bytecode protocol definitions.

---

## Current state (honest assessment)

Flux is currently a **promising prototype**, not yet a full production language.

### What already works well

1. **Reactive core model works**
   - `signal` + `reactive` dependency recomputation.
   - Batched `set` updates and deterministic recompute pass.

2. **Binary bytecode pipeline works end-to-end**
   - `.fx` -> compiler -> `.fxbc` -> runtime execution.

3. **Module/import baseline exists**
   - `import foo::bar` and `import foo::bar as x`.
   - Namespace lowering and user module loading from filesystem.

4. **Tooling baseline exists**
   - Trace mode with recompute reason output.
   - Lightweight graph inspector metadata (`version`, `last_eval_tick`).

### What is still limited (important)

1. **Language syntax is mostly single-line statement oriented**
   - Multiline blocks are not yet fully language-native in the way users expect from mature languages.
   - `if`/`fn` ergonomics are still transitional.

2. **Control-flow/functions are not fully runtime-lowered**
   - Frontend can parse/type-check more than backend can execute in some cases.
   - Function model and block semantics are not complete enough for real-world user code.

3. **Parser and semantics still need “full language” polish**
   - Better multiline parsing and block scopes.
   - Better error recovery and diagnostics for complex code.

4. **Runtime coverage is narrower than syntax surface**
   - Some constructs are represented but not fully executed with complete semantics.

---

## Example that users expect (target behavior)

The following style is what Flux should support naturally:

```fx
signal mouse_x = 10
signal mouse_y = 4
reactive cursor = (mouse_x + mouse_y) * 2

print(cursor)
set mouse_x = 256

fn main() {
    set mouse_y = 25
}

print(cursor)
main()
print(cursor)
```

Current prototype may not handle all aspects of this in the expected fully-language way yet.

---

## Priority roadmap to become a “real language”

### Stage A — syntax and parser completeness

- True multiline block parsing for `fn`, `if`, `match`.
- Robust statement boundaries (not fragile one-line assumptions).
- Proper nested block handling.

### Stage B — semantic/runtime parity

- Ensure every parsed construct has complete and consistent lowering + runtime behavior.
- Implement full function call semantics (definition, scope, invocation, return behavior).
- Finalize `if`/`match` execution semantics beyond transitional lowering.

### Stage C — diagnostics and developer UX

- High quality line/column diagnostics with context snippets.
- Better parse/type error messages for block/control-flow code.
- Expand trace/inspector tooling for debugging larger programs.

### Stage D — language ergonomics

- Cleaner syntax for everyday usage.
- Stable module/package conventions.
- Expanded stdlib and examples.

---

## Build and run

```bash
cargo run -- /tmp/demo.fx /tmp/demo.fxbc
cmake -S runtime -B runtime/build
cmake --build runtime/build
./runtime/build/runtime /tmp/demo.fxbc
```

---

## Practical summary

Flux is in a strong **architectural prototype phase**:

- Core idea: good ✅
- Core architecture split: good ✅
- Deterministic reactive runtime foundation: good ✅
- Full programming-language completeness: not yet ❗

This is exactly the right moment to focus on parser/block semantics, full function behavior, and frontend/runtime semantic parity.


## Block parser v1 status

- Parser now supports multiline logical statements by buffering until brace/paren balance closes.
- `if { ... } else { ... }` works across multiple lines in expression contexts.
- `fn ... { ... }` declarations are parsed from multiline form (body currently parsed as expression payload).
- Runtime remains limited by current lowering/runtime coverage for full function execution, but multiline `if` execution works end-to-end via existing lowering path.


## HIR block model v1

- `FnDef` body is now represented as `Block { stmts: Vec<Stmt> }` in HIR.
- `if` branches are represented as `Block` nodes in HIR (`then_branch`, `else_branch`).
- Parser maps multiline block syntax directly into HIR block form.
- Current runtime execution still uses expression-level lowering for condition branches (block value = last expression statement).
