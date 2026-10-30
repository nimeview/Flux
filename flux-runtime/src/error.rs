//! Language-level runtime errors raised while executing a program.

use flux_value::ArithError;

/// The first runtime error raised by an [`crate::Executor`]. Execution stops
/// at the failing operation: no further assignments, emits or watches run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub kind: ArithError,
    /// What was executing: a binding name, `watch`, `on Event`, or module init.
    pub context: Option<String>,
    /// The binding being computed or initialized, when known.
    pub binding: Option<String>,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "runtime error: {}", self.kind)?;
        if let Some(context) = &self.context {
            write!(f, " in {context}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

/// Why [`crate::Executor::with_mode`] could not produce a running program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorError {
    /// The compiled program violates a VM invariant (a compiler bug).
    Invalid(String),
    /// The program raised a runtime error while initializing.
    Runtime(RuntimeError),
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(msg) => write!(f, "invalid program: {msg}"),
            Self::Runtime(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ExecutorError {}
