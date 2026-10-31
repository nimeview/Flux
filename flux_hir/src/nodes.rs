use flux_diagnostics::Span;
use flux_syntax::TypeName;

use crate::symbol::SymbolId;

#[derive(Debug, Clone)]
pub struct HirProgram {
    pub items: Vec<HirItem>,
    pub names: std::collections::HashMap<SymbolId, String>,
    /// Maps enum variant name → enum type name.
    pub enum_variants: std::collections::HashMap<String, String>,
    /// Trait method implementations, keyed by `(receiver type tag, method name)` for
    /// exact dispatch lookup (e.g. `("Int", "to_string")`) — see `flux_types::check`'s
    /// handling of method-call syntax (`x.method(...)`).
    pub trait_impls: std::collections::HashMap<(String, String), SymbolId>,
    /// Generic function templates keyed by base name.
    pub generic_fns: std::collections::HashMap<String, GenericFnTemplate>,
    /// Type aliases declared in this module (`type UserId = Int`).
    pub type_aliases: std::collections::HashMap<String, HirType>,
    /// Qualified import lookup: `(alias, export_name)` → symbol.
    pub qualified_symbols: std::collections::HashMap<(String, String), SymbolId>,
    /// Names declared with `const` (immutable at compile time).
    pub const_bindings: std::collections::HashSet<String>,
    /// Symbol metadata for every id in `names` (kind + span for IDE diagnostics).
    pub symbols: std::collections::HashMap<SymbolId, crate::symbol::Symbol>,
}

#[derive(Debug, Clone)]
pub struct GenericFnTemplate {
    pub type_params: Vec<String>,
    pub fn_item: HirFn,
}

#[derive(Debug, Clone)]
pub enum HirItem {
    Struct(HirStruct),
    Enum(HirEnum),
    Trait(HirTrait),
    Impl(HirImpl),
    Event(HirEvent),
    Binding(HirBinding),
    SourceAssign(HirAssign),
    InitIf(HirIf),
    Fn(HirFn),
    Watch(HirWatch),
    On(HirOn),
}

#[derive(Debug, Clone)]
pub struct HirStruct {
    pub name: SymbolId,
    pub type_params: Vec<String>,
    pub fields: Vec<(String, HirType)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEnum {
    pub name: SymbolId,
    pub variants: Vec<(String, Option<HirEnumPayload>)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirEnumPayload {
    Tuple(HirType),
    Struct(Vec<(String, HirType)>),
}

#[derive(Debug, Clone)]
pub struct HirTrait {
    pub name: SymbolId,
    pub methods: Vec<(String, Vec<HirType>, Option<HirType>)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirImpl {
    pub trait_name: String,
    pub target_type: HirType,
    pub methods: Vec<HirFn>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEvent {
    pub name: SymbolId,
    pub payload: Option<EventField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EventField {
    pub name: String,
    pub ty: HirType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirBinding {
    pub name: SymbolId,
    pub kind: crate::BindingKind,
    pub ty: Option<HirType>,
    pub value: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirFn {
    pub name: SymbolId,
    pub type_params: Vec<String>,
    pub params: Vec<HirParam>,
    pub ret: Option<HirType>,
    pub body: HirBlock,
    pub reactive: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirWatch {
    pub target: Option<SymbolId>,
    pub body: HirBlock,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirOn {
    pub event: SymbolId,
    pub param: Option<SymbolId>,
    pub body: HirBlock,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: SymbolId,
    pub ty: HirType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirBlock {
    pub stmts: Vec<HirStmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirStmt {
    Let(HirLet),
    Assign(HirAssign),
    Emit(HirEmit),
    Return(HirReturn),
    Break(Span),
    Continue(Span),
    Expr(HirExpr),
    If(HirIf),
    While(HirWhile),
    For(HirFor),
    Watch(HirWatch),
    On(HirOn),
}

#[derive(Debug, Clone)]
pub struct HirReturn {
    pub value: Option<HirExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirLet {
    pub name: SymbolId,
    pub ty: Option<HirType>,
    pub value: HirExpr,
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirAssign {
    pub name: SymbolId,
    pub value: HirExpr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEmit {
    pub event: SymbolId,
    pub args: Vec<HirExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirIf {
    pub cond: HirExpr,
    pub then_branch: HirBlock,
    pub else_branch: Option<HirElse>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirElse {
    If(Box<HirIf>),
    Block(HirBlock),
}

#[derive(Debug, Clone)]
pub struct HirWhile {
    pub cond: HirExpr,
    pub body: HirBlock,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirFor {
    pub name: SymbolId,
    pub iter: HirExpr,
    pub body: HirBlock,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirExpr {
    Int {
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
    Ident {
        symbol: SymbolId,
        span: Span,
    },
    Snap {
        expr: Box<HirExpr>,
        span: Span,
    },
    Call {
        callee: Box<HirExpr>,
        args: Vec<HirExpr>,
        span: Span,
    },
    Field {
        base: Box<HirExpr>,
        field: String,
        span: Span,
    },
    Closure {
        fn_symbol: SymbolId,
        span: Span,
    },
    Unary {
        op: flux_syntax::UnaryOp,
        expr: Box<HirExpr>,
        span: Span,
    },
    Binary {
        op: flux_syntax::BinaryOp,
        left: Box<HirExpr>,
        right: Box<HirExpr>,
        span: Span,
    },
    If {
        cond: Box<HirExpr>,
        then_expr: Box<HirExpr>,
        else_expr: Box<HirExpr>,
        span: Span,
    },
    Array {
        elems: Vec<HirExpr>,
        span: Span,
    },
    StructLit {
        name: String,
        fields: Vec<(String, HirExpr)>,
        span: Span,
    },
    EnumVariant {
        enum_name: String,
        variant: String,
        payload: Option<Box<HirExpr>>,
        span: Span,
    },
    Match {
        scrutinee: Box<HirExpr>,
        arms: Vec<HirMatchArm>,
        span: Span,
    },
    Cast {
        expr: Box<HirExpr>,
        ty: String,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub variant: String,
    pub bindings: Vec<SymbolId>,
    pub binding_fields: Vec<String>,
    pub guard: Option<HirExpr>,
    /// Block-arm statements run before `expr`: only `let`, assignment to a
    /// local, and expression statements (empty for an expression arm).
    pub body: HirBlock,
    pub expr: HirExpr,
}

impl HirMatchArm {
    /// Expressions evaluated by the block-arm statements, in order.
    pub fn body_exprs(&self) -> impl Iterator<Item = &HirExpr> {
        self.body.stmts.iter().filter_map(|stmt| match stmt {
            HirStmt::Let(l) => Some(&l.value),
            HirStmt::Assign(a) => Some(&a.value),
            HirStmt::Expr(e) => Some(e),
            _ => None,
        })
    }

    pub fn body_exprs_mut(&mut self) -> impl Iterator<Item = &mut HirExpr> {
        self.body.stmts.iter_mut().filter_map(|stmt| match stmt {
            HirStmt::Let(l) => Some(&mut l.value),
            HirStmt::Assign(a) => Some(&mut a.value),
            HirStmt::Expr(e) => Some(e),
            _ => None,
        })
    }
}

#[derive(Debug, Clone)]
pub enum HirType {
    Named {
        name: String,
        span: Span,
    },
    Generic {
        name: String,
        args: Vec<HirType>,
        span: Span,
    },
    TypeParam {
        name: String,
        span: Span,
    },
    Array {
        inner: Box<HirType>,
        span: Span,
    },
    Option {
        inner: Box<HirType>,
        span: Span,
    },
    Result {
        ok: Box<HirType>,
        err: Box<HirType>,
        span: Span,
    },
}

impl HirExpr {
    pub fn span(&self) -> Span {
        match self {
            Self::Int { span, .. }
            | Self::Float { span, .. }
            | Self::Bool { span, .. }
            | Self::String { span, .. }
            | Self::Ident { span, .. }
            | Self::Snap { span, .. }
            | Self::Call { span, .. }
            | Self::Field { span, .. }
            | Self::Closure { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::If { span, .. }
            | Self::Array { span, .. }
            | Self::StructLit { span, .. }
            | Self::EnumVariant { span, .. }
            | Self::Match { span, .. }
            | Self::Cast { span, .. } => *span,
        }
    }
}

pub fn lower_type(ty: &TypeName) -> HirType {
    match ty {
        TypeName::Simple { name, span } => HirType::Named {
            name: name.clone(),
            span: *span,
        },
        TypeName::Generic { name, args, span } => HirType::Generic {
            name: name.clone(),
            args: args.iter().map(lower_type).collect(),
            span: *span,
        },
        TypeName::Array { inner, span } => HirType::Array {
            inner: Box::new(lower_type(inner)),
            span: *span,
        },
        TypeName::Option { inner, span } => HirType::Option {
            inner: Box::new(lower_type(inner)),
            span: *span,
        },
        TypeName::Result { ok, err, span } => HirType::Result {
            ok: Box::new(lower_type(ok)),
            err: Box::new(lower_type(err)),
            span: *span,
        },
    }
}
