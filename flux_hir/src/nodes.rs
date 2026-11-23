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
    /// The functions `mut self` methods lower to (they return the changed
    /// receiver).
    pub mut_methods: std::collections::HashSet<SymbolId>,
    /// Spans of `place.m(…)` calls written as statements, lowered to
    /// `place = place.m(…)`.
    pub mut_call_sites: std::collections::HashSet<Span>,
    /// The `mut_call_sites` that call a library method changing its receiver
    /// (`xs.push(1)`); filled in after type checking.
    pub changing_calls: std::collections::HashSet<Span>,
    /// `place.m(…)` statements in a closure that captures the place's root.
    pub captured_call_sites: std::collections::HashSet<Span>,
    /// Methods of `impl Type { }` for a library type (`String`, `Int`, …),
    /// with the file that declares them: visible there and in files that
    /// import it.
    pub extension_sources: std::collections::HashMap<SymbolId, flux_diagnostics::SourceId>,
    /// Symbols of the library functions methods on library values call.
    pub builtin_ids: std::collections::HashMap<String, SymbolId>,
    /// The files each file imports.
    pub file_imports: std::collections::HashMap<
        flux_diagnostics::SourceId,
        std::collections::HashSet<flux_diagnostics::SourceId>,
    >,
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
    /// The values the event carries, in order.
    pub fields: Vec<EventField>,
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
    /// A function synthesized from a closure literal.
    pub is_closure: bool,
    pub name: SymbolId,
    pub type_params: Vec<String>,
    pub params: Vec<HirParam>,
    /// For a closure: the outer locals its body reads. At run time they
    /// follow `params`; their types come from the closure's creation site,
    /// matching `HirExpr::Closure::captures` by position.
    pub captures: Vec<SymbolId>,
    pub ret: Option<HirType>,
    pub body: HirBlock,
    pub reactive: bool,
    /// An `act fn`: it may change sources, and a call is one change.
    pub act: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirWatch {
    pub target: Option<SymbolId>,
    /// `watch total(prev)`: the value the watch saw last time.
    pub prev: Option<SymbolId>,
    pub body: HirBlock,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirOn {
    pub event: SymbolId,
    /// One name per value the event carries.
    pub params: Vec<SymbolId>,
    pub body: HirBlock,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: SymbolId,
    pub ty: HirType,
    /// Whether the source closure parameter omitted a type annotation.
    pub unannotated: bool,
    /// The checker used Int because no usable context was available.
    pub defaulted_to_int: bool,
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
    /// The steps from `name` to the assigned part (`p.x`, `grid[i][j]`);
    /// empty when the whole value is assigned. Each index is evaluated once,
    /// before `value`.
    pub path: Vec<HirPlaceStep>,
    /// `+=` and the like on a path: the part becomes `part op value`. (A
    /// plain `x += v` is lowered to `x = x + v`.)
    pub op: Option<flux_syntax::BinaryOp>,
    pub value: HirExpr,
    pub span: Span,
}

/// One step of an assigned place.
#[derive(Debug, Clone)]
pub enum HirPlaceStep {
    Index(HirExpr),
    Field { name: String, span: Span },
}

/// The hidden builtin that `base with { x: v, y: w }` calls, as
/// `(base, "x", v, "y", w)`. No user name can be spelled with a NUL.
pub const WITH_FIELDS: &str = "\0with";

/// The hidden builtin a tuple literal `(a, b)` calls, and the name of the
/// generic type `(A, B)` is lowered to. At run time a tuple is a struct of
/// this name with fields `"0"`, `"1"`, ….
pub const TUPLE: &str = "\0tuple";

/// The hidden builtin `(op.f)` lowers to, as `(op.f)`: the value of its
/// argument, so that a call of it calls the field's function, never a method.
pub const GROUPED: &str = "\0group";

/// The hidden builtin that checks a destructured value: `(value, n)` is
/// `value`, which must be a tuple of exactly `n` values.
pub const TUPLE_CHECK: &str = "\0untuple";

/// The position `t.i` names: digits without leading zeros (`0`, `12`).
pub fn tuple_index(field: &str) -> Option<usize> {
    field
        .parse::<usize>()
        .ok()
        .filter(|i| i.to_string() == field)
}

impl HirAssign {
    /// The assigned part read as an expression (`p.x`, `grid[i][j]`), for
    /// type checking.
    pub fn place_expr(&self) -> HirExpr {
        let mut place = HirExpr::Ident {
            symbol: self.name,
            span: self.span,
        };
        for step in &self.path {
            place = match step {
                HirPlaceStep::Index(index) => HirExpr::Index {
                    base: Box::new(place),
                    index: Box::new(index.clone()),
                    span: self.span,
                },
                HirPlaceStep::Field { name, span } => HirExpr::Field {
                    base: Box::new(place),
                    field: name.clone(),
                    span: *span,
                },
            };
        }
        place
    }

    /// The index expressions of the path, outermost first.
    pub fn indices(&self) -> impl Iterator<Item = &HirExpr> {
        self.path.iter().filter_map(|step| match step {
            HirPlaceStep::Index(index) => Some(index),
            HirPlaceStep::Field { .. } => None,
        })
    }

    pub fn indices_mut(&mut self) -> impl Iterator<Item = &mut HirExpr> {
        self.path.iter_mut().filter_map(|step| match step {
            HirPlaceStep::Index(index) => Some(index),
            HirPlaceStep::Field { .. } => None,
        })
    }
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
        /// Locals of the code around it, copied when the closure is created.
        captures: Vec<SymbolId>,
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
    /// `base[index]` on an array.
    Index {
        base: Box<HirExpr>,
        index: Box<HirExpr>,
        span: Span,
    },
    /// `{expr}` in a string literal: the value as text, like `to_string`.
    ToText {
        expr: Box<HirExpr>,
        span: Span,
    },
    /// `expr?` (only in functions with a `Result`/`Option` return type).
    Try {
        expr: Box<HirExpr>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub variant: String,
    pub bindings: Vec<SymbolId>,
    pub binding_fields: Vec<String>,
    pub guard: Option<HirExpr>,
    /// A literal pattern (`0`, `"a"`, `true`): the arm matches when the
    /// scrutinee `==` it. `variant` is empty and there are no bindings.
    pub literal: Option<HirExpr>,
    /// Block-arm statements run before `expr` (empty for an expression arm).
    /// Outside functions only `let`, assignment to a local, and expression
    /// statements; inside a function also control flow.
    pub body: HirBlock,
    pub expr: HirExpr,
    /// The arm ends in `return`/`break`/`continue` and has no value; `expr`
    /// is a placeholder that never runs, and the arm adds no type to the match.
    pub diverges: bool,
    /// The block arm ends in a statement that is not a jump: its value is
    /// `()` (`expr` is a placeholder).
    pub unit: bool,
    /// For an arm lowered from a nested pattern (`Some(Num(n))`) without a
    /// user guard: the payload's pattern shape, so exhaustiveness checking
    /// can count it. (Its `guard` is the synthesized nested test.)
    pub nested: Option<PatShape>,
}

/// The shape of a (sub)pattern, for exhaustiveness checking.
#[derive(Debug, Clone, PartialEq)]
pub enum PatShape {
    /// `_` or a binding: matches everything.
    Any,
    /// A literal: never exhausts a type on its own.
    Literal,
    /// A variant, with the shape of its payload (`None` = payload-free or
    /// not inspected).
    Variant {
        name: String,
        payload: Option<Box<PatShape>>,
    },
    /// `true` or `false`.
    Bool(bool),
    /// A tuple, with the shape of each value.
    Tuple(Vec<PatShape>),
}

impl HirMatchArm {
    /// Expressions evaluated by the block-arm statements, in order.
    pub fn body_exprs(&self) -> impl Iterator<Item = &HirExpr> {
        self.body.stmts.iter().flat_map(|stmt| -> Vec<&HirExpr> {
            match stmt {
                HirStmt::Let(l) => vec![&l.value],
                HirStmt::Assign(a) => a.indices().chain([&a.value]).collect(),
                HirStmt::Expr(e) => vec![e],
                _ => Vec::new(),
            }
        })
    }

    pub fn body_exprs_mut(&mut self) -> impl Iterator<Item = &mut HirExpr> {
        self.body
            .stmts
            .iter_mut()
            .flat_map(|stmt| -> Vec<&mut HirExpr> {
                match stmt {
                    HirStmt::Let(l) => vec![&mut l.value],
                    HirStmt::Assign(a) => {
                        let HirAssign { path, value, .. } = a;
                        path.iter_mut()
                            .filter_map(|step| match step {
                                HirPlaceStep::Index(index) => Some(index),
                                HirPlaceStep::Field { .. } => None,
                            })
                            .chain([value])
                            .collect()
                    }
                    HirStmt::Expr(e) => vec![e],
                    _ => Vec::new(),
                }
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
    /// `ret` is `None` for `fn(A)`, which returns `Unit`.
    Function {
        params: Vec<HirType>,
        ret: Option<Box<HirType>>,
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
            | Self::Cast { span, .. }
            | Self::Try { span, .. }
            | Self::ToText { span, .. }
            | Self::Index { span, .. } => *span,
        }
    }
}

pub fn lower_type(ty: &TypeName) -> HirType {
    match ty {
        TypeName::Simple { name, span } => HirType::Named {
            name: name.clone(),
            span: *span,
        },
        TypeName::Tuple { elems, span } => HirType::Generic {
            name: TUPLE.to_string(),
            args: elems.iter().map(lower_type).collect(),
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
        TypeName::Function { params, ret, span } => HirType::Function {
            params: params.iter().map(lower_type).collect(),
            ret: ret.as_ref().map(|r| Box::new(lower_type(r))),
            span: *span,
        },
    }
}
