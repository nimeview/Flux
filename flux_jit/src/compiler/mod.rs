//! Cranelift JIT compiler for Flux IR (split into focused submodules).

mod emit;
mod engine;
mod lower_inst;
mod lower_stmt;
pub(crate) mod numeric;
mod slots;
mod tables;

pub use engine::{JitEngine, JitStats};
pub use slots::SlotLayout;
pub use tables::JitTables;
