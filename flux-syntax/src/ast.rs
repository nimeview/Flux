use flux_diagnostics::{SourceId, Span};

/// Parsed Flux source unit.
#[derive(Debug, Clone)]
pub struct Program {
    pub source: SourceId,
    pub items: Vec<Item>,
}

/// Top-level item.
#[derive(Debug, Clone)]
pub enum Item {
    Import(ImportDecl),
    TypeAlias(TypeAliasDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Event(EventDecl),
    Fn(FnDecl),
    RFn(RFnDecl),
    Derived(DerivedStmt),
    Const(ConstBinding),
    Let(LetStmt),
    Watch(WatchStmt),
    On(OnStmt),
    If(IfStmt),
}

#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub pub_: bool,
    pub name: Ident,
    pub ty: TypeName,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ConstBinding {
    pub pub_: bool,
    pub name: Ident,
    pub ty: Option<TypeName>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub pub_: bool,
    pub name: Ident,
    pub type_params: Vec<String>,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: Ident,
    pub ty: TypeName,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub pub_: bool,
    pub name: Ident,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Ident,
    pub payload: Option<EnumPayload>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum EnumPayload {
    Tuple(TypeName),
    Struct(Vec<(Ident, TypeName)>),
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: String,
    pub alias: Option<String>,
    pub symbols: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub pub_: bool,
    pub name: Ident,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeName>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImplDecl {
    pub trait_name: Ident,
    pub target_type: TypeName,
    pub methods: Vec<FnDecl>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EventDecl {
    pub pub_: bool,
    pub name: Ident,
    pub params: Vec<Param>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub pub_: bool,
    pub name: Ident,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Option<TypeName>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RFnDecl {
    pub pub_: bool,
    pub name: Ident,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Option<TypeName>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct DerivedStmt {
    pub pub_: bool,
    pub pattern: LetPattern,
    pub ty: Option<TypeName>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WatchStmt {
    pub target: Option<Ident>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct OnStmt {
    pub event: Ident,
    pub param: Option<Ident>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeName,
    pub span: Span,
}

/// Closure parameter, with an optional explicit type annotation (`|x: Int|`).
/// When absent, the type is inferred from the call-site context.
#[derive(Debug, Clone)]
pub struct ClosureParam {
    pub name: Ident,
    pub ty: Option<TypeName>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(LetStmt),
    Assign(AssignStmt),
    Emit(EmitStmt),
    Return(ReturnStmt),
    Break(Span),
    Continue(Span),
    Expr(Expr),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Watch(WatchStmt),
    On(OnStmt),
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub pub_: bool,
    pub mutable: bool,
    pub pattern: LetPattern,
    pub ty: Option<TypeName>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EmitStmt {
    pub event: Ident,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_branch: Block,
    pub else_branch: Option<ElseBranch>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    If(Box<IfStmt>),
    Block(Block),
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub cond: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub name: Ident,
    pub iter: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int {
        value: i64,
        span: Span,
    },
    Float {
        value: f64,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    InterpolatedString {
        parts: Vec<StringPart>,
        span: Span,
    },
    Ident {
        name: String,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    Field {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
    Closure {
        params: Vec<ClosureParam>,
        body: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    If {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        span: Span,
    },
    Array {
        elems: Vec<Expr>,
        span: Span,
    },
    StructLit {
        name: Ident,
        fields: Vec<FieldInit>,
        span: Span,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Cast {
        expr: Box<Expr>,
        ty: String,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Simple(Ident),
    Tuple {
        variant: Ident,
        bindings: Vec<Ident>,
        span: Span,
    },
    Struct {
        variant: Ident,
        fields: Vec<Ident>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum StringPart {
    Text(String),
    Ident(String),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub guard: Option<Expr>,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeName {
    Simple {
        name: String,
        span: Span,
    },
    Generic {
        name: String,
        args: Vec<TypeName>,
        span: Span,
    },
    Array {
        inner: Box<TypeName>,
        span: Span,
    },
    Option {
        inner: Box<TypeName>,
        span: Span,
    },
    Result {
        ok: Box<TypeName>,
        err: Box<TypeName>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum LetPattern {
    Ident(Ident),
    Struct {
        name: Ident,
        fields: Vec<Ident>,
        span: Span,
    },
}

impl LetPattern {
    /// Primary binding name when the pattern is a single identifier.
    pub fn primary_name(&self) -> Option<&str> {
        match self {
            Self::Ident(ident) => Some(&ident.name),
            Self::Struct { .. } => None,
        }
    }

    /// All names introduced by this pattern.
    pub fn binding_names(&self) -> Vec<String> {
        match self {
            Self::Ident(ident) => vec![ident.name.clone()],
            Self::Struct { fields, .. } => fields.iter().map(|f| f.name.clone()).collect(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Self::Ident(ident) => ident.span,
            Self::Struct { span, .. } => *span,
        }
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Int { span, .. }
            | Self::Float { span, .. }
            | Self::Bool { span, .. }
            | Self::String { span, .. }
            | Self::InterpolatedString { span, .. }
            | Self::Ident { span, .. }
            | Self::Call { span, .. }
            | Self::Field { span, .. }
            | Self::Closure { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::If { span, .. }
            | Self::Array { span, .. }
            | Self::StructLit { span, .. }
            | Self::Match { span, .. }
            | Self::Cast { span, .. } => *span,
        }
    }
}
