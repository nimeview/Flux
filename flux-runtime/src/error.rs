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
    /// Extra text, e.g. the two values of a failed `assert_eq`.
    pub detail: Option<String>,
    /// What was running when it failed, innermost first.
    pub trace: Vec<TraceFrame>,
    /// Where the current frame failed, until its trace entry takes it.
    pub(crate) pending_loc: Option<flux_ir::SourceLoc>,
}

/// One entry of [`RuntimeError::trace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFrame {
    /// A function name, `<closure>`, a binding, `watch`, or `on Event`.
    pub name: String,
    /// `path:line:column`, when known.
    pub location: Option<String>,
}

impl std::fmt::Display for TraceFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.location {
            Some(location) => write!(f, "{} ({location})", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// Frames shown from each end of a longer trace.
const TRACE_INNER: usize = 10;
const TRACE_OUTER: usize = 2;

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "runtime error: {}", self.kind)?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        if let Some(context) = &self.context {
            write!(f, " in {context}")?;
        }
        // `{:#}` adds the trace.
        if !f.alternate() {
            return Ok(());
        }
        let hidden = self.trace.len().saturating_sub(TRACE_INNER + TRACE_OUTER);
        for (i, frame) in self.trace.iter().enumerate() {
            if hidden == 0 || i < TRACE_INNER || i >= TRACE_INNER + hidden {
                write!(f, "\n  at {frame}")?;
            } else if i == TRACE_INNER {
                write!(f, "\n  ... {hidden} more")?;
            }
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
