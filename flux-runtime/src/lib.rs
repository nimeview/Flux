mod backend;
mod error;
mod exec;
mod graph;
mod vm;

#[cfg(feature = "jit")]
pub use backend::JitBackend;
pub use backend::{EvalContext, ExecBackend, JitResult, VmBackend};
pub use error::{ExecutorError, RuntimeError};
pub use exec::Executor;
pub use flux_ir::{EffectMeta, EffectTrigger, HandlerKind};
#[cfg(feature = "jit")]
pub use flux_jit::JitStats;
pub use flux_value::ArithError;
pub use graph::{NodeId, NodeKind, ReactiveGraph, RuntimeValue};
pub use vm::validate_program;

/// Which execution engine the `Executor` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    /// Tree-walking stack-machine VM (default, always available).
    #[default]
    Vm,
    /// Cranelift JIT — numeric derived-nodes and pure functions are
    /// compiled to native code; strings/arrays/loops fall back to VM.
    Jit,
}
