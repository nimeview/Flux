use miette::Diagnostic;
use thiserror::Error;

use crate::{SourceId, Span};

/// Non-fatal compiler warnings (compilation may still succeed).
#[derive(Debug, Clone, Diagnostic, Error)]
pub enum FluxWarning {
    #[error("unused binding `{name}`")]
    #[diagnostic(
        code(flux::E015),
        help("remove the binding or reference it from a derived value, watch, or handler")
    )]
    UnusedBinding {
        name: String,
        #[label("never read")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
}

impl FluxWarning {
    pub fn source_id(&self) -> SourceId {
        match self {
            Self::UnusedBinding { source_id, .. } => *source_id,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::UnusedBinding { .. } => "E015",
        }
    }

    pub fn label_span(&self) -> Span {
        let source_id = self.source_id();
        let miette_span = match self {
            Self::UnusedBinding { span, .. } => *span,
        };
        span_from_miette(source_id, miette_span)
    }
}

pub(crate) fn span_from_miette(source_id: SourceId, span: miette::SourceSpan) -> Span {
    let start = span.offset();
    Span {
        source: source_id,
        start,
        end: start + span.len(),
    }
}
