//! Spans, structured errors (E000–E017), and warnings (E015) for the Flux compiler.
//!
//! Every pipeline phase appends to a shared [`DiagnosticBag`]. Errors block compilation;
//! warnings are surfaced in the CLI and LSP but do not fail the build.

mod error;
mod source;
mod warning;

pub use error::{span_to_miette, DiagnosticBag, FluxError, ReportedError};
pub use source::{line_col_to_offset, offset_to_line_col, SourceFile, SourceId, Span};
pub use warning::FluxWarning;
