//! Cranelift JIT backend for Flux.
//!
//! # Architecture
//!
//! ```text
//! flux_ir::CompiledProgram
//!      │
//!      ▼
//! JitEngine::compile()   – translates IR → Cranelift IR → native code
//!      │
//!      ▼
//! JitEngine::eval_expr() – called by JitExecutor instead of the VM stack
//! JitEngine::run_stmts() – JIT-compiled stmt bodies (watch/on/fn)
//! ```
//!
//! Strings, user enums, and arrays use `flux_value` heap stores; numeric
//! `emit` in handlers is lowered to `flux_jit_emit`.

mod cache;
mod compiler;
mod host;

pub use cache::{cache_key, get_or_compile, JIT_CACHE_VERSION};
pub use compiler::{JitEngine, JitStats, JitTables, SlotLayout};
pub use flux_value::{
    get_array, get_enum, get_string, intern_string, store_enum, tagged_to_value, value_to_tagged,
    TaggedVal,
};
pub use host::{set_emit_hook, take_emit_events};

/// Eligibility predicates exported for tests and tooling.
pub fn code_is_jit_numeric(
    code: &[flux_ir::Inst],
    jit_fns: &std::collections::HashSet<String>,
) -> bool {
    compiler::numeric::is_numeric_code(code, jit_fns)
}

pub fn stmts_are_jit_numeric(
    stmts: &[flux_ir::IrStmt],
    jit_fns: &std::collections::HashSet<String>,
) -> bool {
    compiler::numeric::stmts_numeric(stmts, jit_fns)
}

pub fn reset_host_for_tests() {
    host::clear_stores_for_tests();
    cache::clear_for_tests();
}
