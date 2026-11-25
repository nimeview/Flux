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
    pub type_params: Vec<String>,
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
    /// With `self` first (typed `Self`) when the method takes one.
    pub params: Vec<Param>,
    /// `mut self`.
    pub mut_self: bool,
    pub ret: Option<TypeName>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImplDecl {
    /// `impl[T] Pair[T]`: the parameters of a generic type's impl.
    pub type_params: Vec<String>,
    pub type_bounds: Vec<TypeBound>,
    /// `None` for `impl Type { }`: the type's own methods.
    pub trait_name: Option<Ident>,
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
    /// `mut self`: a method that changes the value it is called on. Its
    /// `self` is the first of `params`, typed `Self`.
    pub mut_self: bool,
    pub pub_: bool,
    pub kind: FnKind,
    /// The `fn` keyword.
    pub keyword: Span,
    pub name: Ident,
    pub type_params: Vec<String>,
    /// The traits each type parameter must have (`[T: Ord]`); only those
    /// with a bound.
    pub type_bounds: Vec<TypeBound>,
    pub params: Vec<Param>,
    pub ret: Option<TypeName>,
    pub body: Block,
    pub span: Span,
}

/// `T: Ord + Display` in a function's or impl's type parameters.
#[derive(Debug, Clone)]
pub struct TypeBound {
    pub param: String,
    pub traits: Vec<Ident>,
}

/// What a function may do with reactive values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FnKind {
    /// `fn`: does not read them.
    #[default]
    Plain,
    /// `track fn`: reads them, and callers depend on what it reads.
    Track,
    /// `act fn`: reads and changes them; a call is one change.
    Act,
}

#[derive(Debug, Clone)]
pub struct RFnDecl {
    pub pub_: bool,
    /// The `rfn` keyword, which `track fn` replaces.
    pub keyword: Span,
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
    /// `watch total(prev)`: names the value the watch saw last time.
    pub prev: Option<Ident>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct OnStmt {
    pub event: Ident,
    /// `on Move(x, y)`: one name per value the event carries.
    pub params: Vec<Ident>,
    /// The older `on E { n -> }`: the name, and the span from the end of the
    /// event's name to the end of `->`, which the fix rewrites to `(n) {`.
    pub arrow: Option<(Ident, Span)>,
    pub body: Block,
    pub span: Span,
}

impl OnStmt {
    /// The names the handler gives the event's values, in either form.
    pub fn names(&self) -> impl Iterator<Item = &Ident> {
        self.params
            .iter()
            .chain(self.arrow.as_ref().map(|(name, _)| name))
    }
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
    /// `name[i].f… = value`: the place's path from `name`, outermost first
    /// (empty for a plain assignment).
    pub path: Vec<PlaceStep>,
    /// `+=` and the like: the operator that combines the old value with
    /// `value`; `None` for `=`.
    pub op: Option<BinaryOp>,
    pub value: Expr,
    pub span: Span,
}

/// One step of an assigned place: `[index]` or `.field`.
#[derive(Debug, Clone)]
pub enum PlaceStep {
    Index(Expr),
    Field(Ident),
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
    /// The loop variable; for `for (k, v) in …` a hidden name the values
    /// are taken from.
    pub name: Ident,
    /// `for (k, v) in …`: the names for each element's values.
    pub pattern: Option<LetPattern>,
    pub iter: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int {
        /// Integer magnitude/value before any explicit width cast. `i128` is
        /// wide enough to represent every Flux `UInt64` literal.
        value: i128,
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
    /// `(op.f)` called as `(op.f)(3)`: the function in a field, not a method.
    /// Other grouping parentheses are not kept.
    Paren {
        inner: Box<Expr>,
        span: Span,
    },
    /// `(a, b)`: a tuple of two or more values (one is reported later).
    Tuple {
        elems: Vec<Expr>,
        span: Span,
    },
    /// `base with { x: 3 }`: a copy of the struct `base` with these fields
    /// changed. `base` is evaluated once, then the fields in order.
    With {
        base: Box<Expr>,
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
    /// `base[index]`: the array element at `index`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// `expr?`: unwraps `Ok`/`Some`, or returns the `Err`/`None` from the
    /// enclosing function.
    Try {
        expr: Box<Expr>,
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
    /// A literal (`0`, `-1`, `"a"`, `true`) the scrutinee is compared to.
    Literal(Expr),
    Tuple {
        variant: Ident,
        bindings: Vec<Ident>,
        /// A nested payload pattern (`Some(Num(n))`, `Ok(0)`); then
        /// `bindings` is empty. A lone name in `bindings` may also be a
        /// payload-free variant (`Some(LParen)`); lowering decides.
        nested: Option<Box<MatchPattern>>,
        span: Span,
    },
    Struct {
        variant: Ident,
        fields: Vec<Ident>,
        span: Span,
    },
    /// `(0, y)`: a tuple whose values match these patterns.
    TupleValue {
        elems: Vec<MatchPattern>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum StringPart {
    Text(String),
    /// `{expr}` inside a string literal.
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub guard: Option<Expr>,
    /// Statements of a block arm `p => { stmts…; expr }`, run before `expr`
    /// (empty for an expression arm).
    pub stmts: Vec<Stmt>,
    /// The arm's value: the expression, or a block arm's final expression.
    /// `None` when a block arm does not end in an expression: it leaves
    /// (`return`/`break`/`continue` last) or its value is `()`.
    pub expr: Option<Expr>,
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
    Rem,
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
    /// `(A, B)`: a tuple type.
    Tuple {
        elems: Vec<TypeName>,
        span: Span,
    },
    /// `fn(A, B) -> C`; without `->` it returns `Unit`.
    Function {
        params: Vec<TypeName>,
        ret: Option<Box<TypeName>>,
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
    /// `(a, b)`: one pattern per value of a tuple; `_` binds nothing.
    Tuple {
        elems: Vec<LetPattern>,
        span: Span,
    },
}

impl LetPattern {
    /// Primary binding name when the pattern is a single identifier.
    pub fn primary_name(&self) -> Option<&str> {
        match self {
            Self::Ident(ident) => Some(&ident.name),
            Self::Struct { .. } | Self::Tuple { .. } => None,
        }
    }

    /// All names introduced by this pattern.
    pub fn binding_names(&self) -> Vec<String> {
        match self {
            Self::Ident(ident) => vec![ident.name.clone()],
            Self::Struct { fields, .. } => fields.iter().map(|f| f.name.clone()).collect(),
            Self::Tuple { elems, .. } => elems
                .iter()
                .flat_map(|e| e.binding_names())
                .filter(|n| n != "_")
                .collect(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Self::Ident(ident) => ident.span,
            Self::Struct { span, .. } | Self::Tuple { span, .. } => *span,
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
            | Self::With { span, .. }
            | Self::Tuple { span, .. }
            | Self::Paren { span, .. }
            | Self::Match { span, .. }
            | Self::Cast { span, .. }
            | Self::Try { span, .. }
            | Self::Index { span, .. } => *span,
        }
    }
}
