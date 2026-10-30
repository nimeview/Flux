use miette::Diagnostic;
use thiserror::Error;

use crate::{SourceId, Span};

/// All recoverable Flux compiler errors.
#[derive(Debug, Clone, Diagnostic, Error)]
pub enum FluxError {
    #[error("parse error: {message}")]
    #[diagnostic(code(flux::E000), help("check syntax near the highlighted token"))]
    Parse {
        message: String,
        #[label("here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("undefined name `{name}`")]
    #[diagnostic(code(flux::E001))]
    UndefinedName {
        name: String,
        #[label("not found")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("cannot read reactive `{name}` in a pure function")]
    #[diagnostic(
        code(flux::E002),
        help(
            "use `snap({name})` to take a snapshot, or move this code into `watch`, `on`, or `rfn`"
        )
    )]
    ReactiveInPureContext {
        name: String,
        #[label("reactive read")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("cannot assign to derived binding `{name}`")]
    #[diagnostic(
        code(flux::E003),
        help("derived bindings are read-only; assign to a source binding instead")
    )]
    AssignToDerived {
        name: String,
        #[label("invalid assignment")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("duplicate definition of `{name}`")]
    #[diagnostic(code(flux::E004))]
    DuplicateDefinition {
        name: String,
        #[label("previous definition")]
        prev_span: miette::SourceSpan,
        #[label("duplicate")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("type mismatch: expected `{expected}`, found `{found}`")]
    #[diagnostic(code(flux::E005))]
    TypeMismatch {
        expected: String,
        found: String,
        #[label("expected `{expected}`")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("unknown event `{name}`")]
    #[diagnostic(code(flux::E006))]
    UnknownEvent {
        name: String,
        #[label("unknown event")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("unknown type `{name}`")]
    #[diagnostic(code(flux::E007))]
    UnknownType {
        name: String,
        #[label("unknown type")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("control flow `{construct}` is not allowed in a derived expression")]
    #[diagnostic(
        code(flux::E008),
        help("use `watch`, `on`, or `rfn` for statements with side effects or loops")
    )]
    InvalidControlFlowInDerived {
        construct: String,
        #[label("not allowed here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("cannot assign to immutable local `{name}`")]
    #[diagnostic(
        code(flux::E009),
        help("use `let mut {name} = ...` to declare a mutable local, or assign to a `let mut` source binding")
    )]
    AssignToImmutableLocal {
        name: String,
        #[label("immutable local")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("cyclic dependency in reactive graph: {cycle}")]
    #[diagnostic(
        code(flux::E010),
        help("break the cycle by making one binding a source, or remove a dependency edge")
    )]
    CyclicDependency {
        cycle: String,
        #[label("part of cycle")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("`{construct}` is only allowed at module level")]
    #[diagnostic(
        code(flux::E011),
        help("move this `{construct}` out of the function body and declare it at the top level of the module")
    )]
    ReactiveEffectInFunction {
        construct: String,
        #[label("not allowed here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("import path escapes project root: `{path}`")]
    #[diagnostic(
        code(flux::E012),
        help("use a relative import within the project directory")
    )]
    ImportEscape {
        path: String,
        #[label("invalid import")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("cyclic import: {cycle}")]
    #[diagnostic(code(flux::E013), help("remove the circular import dependency"))]
    CyclicImport {
        cycle: String,
        #[label("import cycle")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("unknown export `{name}` in module `{module}`")]
    #[diagnostic(code(flux::E014), help("check the module's public exports"))]
    UnknownExport {
        name: String,
        module: String,
        #[label("not exported")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("`let` without `mut` cannot read reactive binding `{name}`")]
    #[diagnostic(
        code(flux::E016),
        help("use `derived` for computed values, or `let mut` for reassignable sources")
    )]
    ImmutableLetReadsReactive {
        name: String,
        #[label("reactive read")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("side effect `{name}` is not allowed in a derived expression")]
    #[diagnostic(
        code(flux::E017),
        help("move I/O and printing into `watch`, `on`, or a function body")
    )]
    SideEffectInDerived {
        name: String,
        #[label("side effect")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("{message}")]
    #[diagnostic(code(flux::E018))]
    Runtime {
        message: String,
        #[label("while evaluating this")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
}

impl FluxError {
    pub fn source_id(&self) -> SourceId {
        match self {
            Self::Parse { source_id, .. }
            | Self::UndefinedName { source_id, .. }
            | Self::ReactiveInPureContext { source_id, .. }
            | Self::AssignToDerived { source_id, .. }
            | Self::DuplicateDefinition { source_id, .. }
            | Self::TypeMismatch { source_id, .. }
            | Self::UnknownEvent { source_id, .. }
            | Self::UnknownType { source_id, .. }
            | Self::InvalidControlFlowInDerived { source_id, .. }
            | Self::AssignToImmutableLocal { source_id, .. }
            | Self::CyclicDependency { source_id, .. }
            | Self::ReactiveEffectInFunction { source_id, .. }
            | Self::ImportEscape { source_id, .. }
            | Self::CyclicImport { source_id, .. }
            | Self::UnknownExport { source_id, .. }
            | Self::ImmutableLetReadsReactive { source_id, .. }
            | Self::SideEffectInDerived { source_id, .. }
            | Self::Runtime { source_id, .. } => *source_id,
        }
    }

    /// Stable diagnostic code (`E000`–`E018`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "E000",
            Self::UndefinedName { .. } => "E001",
            Self::ReactiveInPureContext { .. } => "E002",
            Self::AssignToDerived { .. } => "E003",
            Self::DuplicateDefinition { .. } => "E004",
            Self::TypeMismatch { .. } => "E005",
            Self::UnknownEvent { .. } => "E006",
            Self::UnknownType { .. } => "E007",
            Self::InvalidControlFlowInDerived { .. } => "E008",
            Self::AssignToImmutableLocal { .. } => "E009",
            Self::CyclicDependency { .. } => "E010",
            Self::ReactiveEffectInFunction { .. } => "E011",
            Self::ImportEscape { .. } => "E012",
            Self::CyclicImport { .. } => "E013",
            Self::UnknownExport { .. } => "E014",
            Self::ImmutableLetReadsReactive { .. } => "E016",
            Self::SideEffectInDerived { .. } => "E017",
            Self::Runtime { .. } => "E018",
        }
    }

    /// Primary source span for this error (duplicate site for E004, not the previous definition).
    pub fn label_span(&self) -> Span {
        let source_id = self.source_id();
        let miette_span = match self {
            Self::Parse { span, .. }
            | Self::UndefinedName { span, .. }
            | Self::ReactiveInPureContext { span, .. }
            | Self::AssignToDerived { span, .. }
            | Self::TypeMismatch { span, .. }
            | Self::UnknownEvent { span, .. }
            | Self::UnknownType { span, .. }
            | Self::InvalidControlFlowInDerived { span, .. }
            | Self::AssignToImmutableLocal { span, .. }
            | Self::CyclicDependency { span, .. }
            | Self::ReactiveEffectInFunction { span, .. }
            | Self::ImportEscape { span, .. }
            | Self::CyclicImport { span, .. }
            | Self::UnknownExport { span, .. }
            | Self::ImmutableLetReadsReactive { span, .. }
            | Self::SideEffectInDerived { span, .. }
            | Self::Runtime { span, .. }
            | Self::DuplicateDefinition { span, .. } => *span,
        };
        crate::warning::span_from_miette(source_id, miette_span)
    }
}

pub fn span_to_miette(span: Span) -> miette::SourceSpan {
    span.to_miette()
}

/// Collects errors and warnings during compilation.
#[derive(Debug, Default)]
pub struct DiagnosticBag {
    pub errors: Vec<FluxError>,
    pub warnings: Vec<crate::FluxWarning>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, error: FluxError) {
        self.errors.push(error);
    }

    pub fn push_warning(&mut self, warning: crate::FluxWarning) {
        self.warnings.push(warning);
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        !self.has_errors() && !self.has_warnings()
    }

    pub fn extend(&mut self, other: Self) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

/// Wrapper for early return after diagnostics were collected.
#[derive(Debug)]
pub struct ReportedError;
