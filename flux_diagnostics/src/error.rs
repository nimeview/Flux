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
    #[diagnostic(code(flux::E002))]
    ReactiveInPureContext {
        name: String,
        #[label("reactive read")]
        span: miette::SourceSpan,
        source_id: SourceId,
        /// How to fix it; mentions `const` when `name` is an immutable `let`.
        #[help]
        help: String,
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

    /// An E005 with additional context, keeping ordinary E005 constructors unchanged.
    #[error("type mismatch: expected `{expected}`, found `{found}`")]
    #[diagnostic(code(flux::E005))]
    TypeMismatchWithHelp {
        expected: String,
        found: String,
        #[label("expected `{expected}`")]
        span: miette::SourceSpan,
        source_id: SourceId,
        #[help]
        help: String,
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
        help(
            "use `watch`, `on`, `track fn`, or `act fn` for statements with side effects or loops"
        )
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

    #[error("{}", if *assigned {
        format!("binding `{name}` is assigned; declare it with `let mut`")
    } else {
        format!("`let` without `mut` cannot read reactive binding `{name}`")
    })]
    #[diagnostic(
        code(flux::E016),
        help("use `derived` for computed values, or `let mut` for reassignable sources")
    )]
    ImmutableLetReadsReactive {
        name: String,
        assigned: bool,
        #[label("binding declared here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("side effect `{name}` is not allowed in {place}")]
    #[diagnostic(
        code(flux::E017),
        help("move I/O and printing into `watch`, `on`, or a function body such as `main`")
    )]
    SideEffectInDerived {
        name: String,
        /// "a derived expression" or "a `const` initializer".
        place: String,
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

    #[error("no method `{method}` for type `{ty}`")]
    #[diagnostic(
        code(flux::E019),
        help("methods come from `impl Trait for {ty}`; add one that defines `{method}`")
    )]
    UnknownMethod {
        method: String,
        ty: String,
        #[label("method not found")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("{message}")]
    #[diagnostic(
        code(flux::E021),
        help("`?` works in a function declared `-> Result[T, E]` on a `Result[U, E]` with the same `E`, or in `-> Option[T]` on an `Option[U]`")
    )]
    InvalidTry {
        message: String,
        #[label("here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("`{name}` in module `{module}` is private")]
    #[diagnostic(code(flux::E022), help("mark it `pub` in `{module}` to import it"))]
    PrivateImport {
        name: String,
        module: String,
        #[label("not public")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("`{name}` is already imported")]
    #[diagnostic(
        code(flux::E004),
        help("rename this definition, or import only the names you use, as in `import math {{ sqrt }}`")
    )]
    ImportedNameTaken {
        name: String,
        #[label("this name comes from an import")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("no value of type `{name}` can exist")]
    #[diagnostic(
        code(flux::E025),
        help(
            "it always contains itself; end the chain with an enum variant, `Option`, or an array"
        )
    )]
    UninhabitedType {
        name: String,
        #[label("declared here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("type alias `{name}` refers to itself")]
    #[diagnostic(
        code(flux::E024),
        help("an alias is another name for a type; use a struct or enum for a recursive type")
    )]
    CyclicTypeAlias {
        name: String,
        #[label("part of the cycle")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("`{callee}` is not a function")]
    #[diagnostic(
        code(flux::E023),
        help("only functions, closures, and values of a function type such as `fn(Int) -> Int` can be called")
    )]
    NotCallable {
        callee: String,
        #[label("called here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    #[error("{message}")]
    #[diagnostic(code(flux::E020))]
    InvalidImpl {
        message: String,
        #[label("in this impl")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },
    /// A function other than an `act fn` changes a source.
    #[error("`{function}` changes `{name}`, but only `on` handlers and `act fn` change state")]
    #[diagnostic(code(flux::E027), help("{help}"))]
    WriteOutsideAct {
        function: String,
        name: String,
        help: String,
        #[label("changes state")]
        span: miette::SourceSpan,
        source_id: SourceId,
        /// The `fn` keyword, which the fix turns into `act fn`.
        fix_span: Option<miette::SourceSpan>,
    },

    /// A derived value, `const`, or `track fn` reaches an `act fn`, whose
    /// reads are not followed.
    #[error("{place} cannot call the `act fn` `{act}`")]
    #[diagnostic(
        code(flux::E008),
        help(
            "an `act fn` is an action, and a derived value does not follow what it reads; write `track fn {act}` for a function that computes from sources"
        )
    )]
    ActInPureContext {
        place: String,
        act: String,
        #[label("runs `{act}`")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    /// Another module's code names a struct field that is not `pub`.
    #[error("field `{field}` of `{ty}` is private to its module")]
    #[diagnostic(code(flux::E033), help("{help}"))]
    PrivateField {
        field: String,
        ty: String,
        help: String,
        #[label("not visible here")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    /// A module's code changes a source another module declares.
    #[error("`{name}` belongs to another module; only that module changes it")]
    #[diagnostic(
        code(flux::E032),
        help("emit one of its events, or call one of its `pub act fn`s")
    )]
    ForeignSourceWrite {
        name: String,
        #[label("changes another module's source")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    /// Code outside a component assigns a member of one of its instances.
    #[error("`{name}` belongs to an instance of `{ty}`; only the instance's own code changes it")]
    #[diagnostic(
        code(flux::E032),
        help("emit one of its `pub` events, or call one of its `pub act fn`s")
    )]
    InstanceMemberWrite {
        name: String,
        ty: String,
        #[label("changes an instance's member")]
        span: miette::SourceSpan,
        source_id: SourceId,
    },

    /// A generic call's type argument lacks a trait its parameter's bound names.
    #[error("`{ty}` does not have trait `{trait_name}`, which `{function}` needs for `{param}`")]
    #[diagnostic(code(flux::E031), help("{help}"))]
    UnsatisfiedBound {
        ty: String,
        trait_name: String,
        function: String,
        param: String,
        help: String,
        #[label("`{param}` is `{ty}` here")]
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
            | Self::TypeMismatchWithHelp { source_id, .. }
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
            | Self::Runtime { source_id, .. }
            | Self::UnknownMethod { source_id, .. }
            | Self::InvalidImpl { source_id, .. }
            | Self::InvalidTry { source_id, .. }
            | Self::PrivateImport { source_id, .. }
            | Self::NotCallable { source_id, .. }
            | Self::CyclicTypeAlias { source_id, .. }
            | Self::UninhabitedType { source_id, .. }
            | Self::ImportedNameTaken { source_id, .. }
            | Self::WriteOutsideAct { source_id, .. }
            | Self::UnsatisfiedBound { source_id, .. }
            | Self::ForeignSourceWrite { source_id, .. }
            | Self::InstanceMemberWrite { source_id, .. }
            | Self::ActInPureContext { source_id, .. }
            | Self::PrivateField { source_id, .. } => *source_id,
        }
    }

    /// Stable diagnostic code (`E000`–`E027` and `E031`–`E033`; warnings use E015,
    /// E026, and E028–E030).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "E000",
            Self::UndefinedName { .. } => "E001",
            Self::ReactiveInPureContext { .. } => "E002",
            Self::AssignToDerived { .. } => "E003",
            Self::DuplicateDefinition { .. } => "E004",
            Self::TypeMismatch { .. } | Self::TypeMismatchWithHelp { .. } => "E005",
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
            Self::UnknownMethod { .. } => "E019",
            Self::InvalidImpl { .. } => "E020",
            Self::InvalidTry { .. } => "E021",
            Self::PrivateImport { .. } => "E022",
            Self::NotCallable { .. } => "E023",
            Self::CyclicTypeAlias { .. } => "E024",
            Self::UninhabitedType { .. } => "E025",
            Self::ImportedNameTaken { .. } => "E004",
            Self::WriteOutsideAct { .. } => "E027",
            Self::UnsatisfiedBound { .. } => "E031",
            Self::ForeignSourceWrite { .. } => "E032",
            Self::InstanceMemberWrite { .. } => "E032",
            Self::ActInPureContext { .. } => "E008",
            Self::PrivateField { .. } => "E033",
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
            | Self::TypeMismatchWithHelp { span, .. }
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
            | Self::UnknownMethod { span, .. }
            | Self::InvalidImpl { span, .. }
            | Self::InvalidTry { span, .. }
            | Self::PrivateImport { span, .. }
            | Self::NotCallable { span, .. }
            | Self::CyclicTypeAlias { span, .. }
            | Self::UninhabitedType { span, .. }
            | Self::ImportedNameTaken { span, .. }
            | Self::WriteOutsideAct { span, .. }
            | Self::UnsatisfiedBound { span, .. }
            | Self::ForeignSourceWrite { span, .. }
            | Self::InstanceMemberWrite { span, .. }
            | Self::ActInPureContext { span, .. }
            | Self::PrivateField { span, .. }
            | Self::DuplicateDefinition { span, .. } => *span,
        };
        crate::warning::span_from_miette(source_id, miette_span)
    }

    /// The edit that resolves this error, if it has one.
    pub fn fix(&self) -> Option<crate::Fix> {
        match self {
            Self::WriteOutsideAct {
                function,
                fix_span: Some(fix_span),
                ..
            } => Some(crate::Fix {
                title: format!("Make `{function}` an `act fn`"),
                span: crate::warning::span_from_miette(self.source_id(), *fix_span),
                replacement: "act fn".to_string(),
            }),
            _ => None,
        }
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

    /// Adds `error` unless the same error at the same place was already
    /// reported (the checker may infer one expression more than once).
    pub fn push(&mut self, error: FluxError) {
        let key = (error.code(), error.to_string(), error.label_span());
        let duplicate = self
            .errors
            .iter()
            .any(|e| (e.code(), e.to_string(), e.label_span()) == key);
        if !duplicate {
            self.errors.push(error);
        }
    }

    /// Adds `warning` unless the same one (code, place, and text) is already
    /// there: code lowered twice reports once.
    pub fn push_warning(&mut self, warning: crate::FluxWarning) {
        let duplicate = self.warnings.iter().any(|w| {
            w.code() == warning.code()
                && w.label_span() == warning.label_span()
                && w.to_string() == warning.to_string()
        });
        if !duplicate {
            self.warnings.push(warning);
        }
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
