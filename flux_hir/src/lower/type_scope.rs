//! Type names scoped to their module.
//!
//! A type (`struct`, `enum`, `trait`) declared in a module other than the root
//! gets the internal name `__mod_<index>__<Name>`, like the module's functions,
//! so two modules may declare the same name. Before lowering, every module's
//! syntax tree, the root's included, is rewritten so that its type names are
//! internal ones: its own types, the types it imports by name or with the
//! whole module (`import geo`, `import geo { Point }`), and `geo::Point`.
//! Bare variants stay as written; lowering gives each module the variants
//! of the enums it declares or imports (`Scoped::variants`). A type another module does
//! not export stays as written, and the type checker reports it as unknown;
//! `geo::Hidden` for a private one is E022.

use std::collections::{HashMap, HashSet};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, Span};
use flux_resolve::LinkedProgram;
use flux_syntax::{
    Block, ElseBranch, EnumPayload, Expr, IfStmt, Item, LetPattern, MatchArm, MatchPattern,
    PlaceStep, Program, Stmt, StringPart, TypeName,
};

/// Internal name of type `name` declared in module `index`.
pub(super) fn module_type_name(index: usize, name: &str) -> String {
    if index == 0 {
        name.to_string()
    } else {
        format!("__mod_{index}__{name}")
    }
}

/// What the type names written in one module refer to.
#[derive(Default)]
struct Scope {
    /// Bare name → internal name.
    types: HashMap<String, String>,
    /// `alias::Name` → internal name, for exported types.
    qualified: HashMap<(String, String), String>,
    /// `alias::Name` of types the module does not export.
    private: HashSet<(String, String)>,
    /// Module path of each alias, for messages.
    paths: HashMap<String, String>,
    /// Bare variant → internal name of its enum, for the enums the module
    /// declares or imports by name.
    variants: HashMap<String, String>,
}

struct TypeDecl {
    name: String,
    public: bool,
    variants: Vec<String>,
}

fn type_decls(program: &Program) -> Vec<TypeDecl> {
    program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) => Some(TypeDecl {
                name: s.name.name.clone(),
                public: s.pub_,
                variants: Vec::new(),
            }),
            Item::Enum(e) => Some(TypeDecl {
                name: e.name.name.clone(),
                public: e.pub_,
                variants: e.variants.iter().map(|v| v.name.name.clone()).collect(),
            }),
            Item::Trait(t) => Some(TypeDecl {
                name: t.name.name.clone(),
                public: t.pub_,
                variants: Vec::new(),
            }),
            _ => None,
        })
        .collect()
}

fn is_stdlib(path: &std::path::Path) -> bool {
    path.parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "stdlib")
}

/// What each module can name, after its types are rewritten.
#[derive(Default)]
pub(crate) struct Scoped {
    /// Per module: bare variant → internal name of its enum, for the enums
    /// the module declares or imports (its own win a clash).
    pub variants: Vec<HashMap<String, String>>,
    /// The root's bare type names → internal names.
    pub root_types: HashMap<String, String>,
    /// The root's `alias::Type` → internal names.
    pub root_qualified: HashMap<(String, String), String>,
}

/// Rewrites every module's type names to their internal names.
pub(crate) fn scope_type_names(
    linked: &mut LinkedProgram,
    diagnostics: &mut DiagnosticBag,
) -> Scoped {
    let mut scoped = Scoped {
        variants: vec![HashMap::new(); linked.modules.len()],
        ..Scoped::default()
    };
    let decls: Vec<Vec<TypeDecl>> = linked
        .modules
        .iter()
        .map(|m| {
            if is_stdlib(&m.path) {
                Vec::new()
            } else {
                type_decls(&m.ast)
            }
        })
        .collect();
    for index in 0..linked.modules.len() {
        if is_stdlib(&linked.modules[index].path) {
            continue;
        }
        let mut scope = Scope::default();
        // Imported types first, so the module's own names win.
        for import in linked.module_imports.get(index).into_iter().flatten() {
            let from = import.module_index;
            if from == 0 || is_stdlib(&linked.modules[from].path) {
                continue;
            }
            scope
                .paths
                .insert(import.alias.clone(), import.alias.clone());
            for decl in &decls[from] {
                let key = (import.alias.clone(), decl.name.clone());
                if !decl.public {
                    scope.private.insert(key);
                    continue;
                }
                let internal = module_type_name(from, &decl.name);
                scope.qualified.insert(key, internal.clone());
                if import.symbols.is_empty() || import.symbols.contains(&decl.name) {
                    scope.types.insert(decl.name.clone(), internal.clone());
                    for variant in &decl.variants {
                        scope.variants.insert(variant.clone(), internal.clone());
                    }
                }
            }
        }
        for decl in &decls[index] {
            let internal = module_type_name(index, &decl.name);
            scope.types.insert(decl.name.clone(), internal.clone());
            for variant in &decl.variants {
                scope.variants.insert(variant.clone(), internal.clone());
            }
        }
        let mut rewrite = Rewrite {
            scope: &scope,
            diagnostics: &mut *diagnostics,
            generics: Vec::new(),
        };
        rewrite.program(&mut linked.modules[index].ast);
        scoped.variants[index] = scope.variants.clone();
        if index == 0 {
            scoped.root_types = scope.types.clone();
            scoped.root_qualified = scope.qualified.clone();
        }
    }
    scoped
}

struct Rewrite<'a> {
    scope: &'a Scope,
    diagnostics: &'a mut DiagnosticBag,
    /// Type parameters in scope (`fn id[T]`): never an imported type.
    generics: Vec<String>,
}

impl Rewrite<'_> {
    /// Names the compiler makes for an imported module's items start with
    /// `__mod_`; a program may not write one, which would reach a private
    /// item or clash with one.
    fn reserved(&mut self, name: &str, span: Span) {
        if name.split("::").any(|part| part.starts_with("__mod_")) {
            self.diagnostics.push(FluxError::Parse {
                message: format!(
                    "`{name}`: names starting with `__mod_` are reserved for the compiler"
                ),
                span: span_to_miette(span),
                source_id: span.source,
            });
        }
    }

    /// A type name as written (`Point`, `geo::Point`) → its internal name.
    fn type_name(&mut self, name: &str, span: Span) -> String {
        self.reserved(name, span);
        if self.generics.iter().any(|g| g == name) {
            return name.to_string();
        }
        if let Some((alias, rest)) = name.split_once("::") {
            return self
                .qualified_path(alias, rest, span)
                .unwrap_or_else(|| name.to_string());
        }
        self.scope
            .types
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// `alias::Type…` with the first two parts replaced by the internal name,
    /// when `alias` imports a module that declares `Type`.
    fn qualified_path(&mut self, alias: &str, rest: &str, span: Span) -> Option<String> {
        let (ty, tail) = match rest.split_once("::") {
            Some((ty, tail)) => (ty, Some(tail)),
            None => (rest, None),
        };
        let key = (alias.to_string(), ty.to_string());
        if self.scope.private.contains(&key) {
            self.diagnostics.push(FluxError::PrivateImport {
                name: ty.to_string(),
                module: self
                    .scope
                    .paths
                    .get(alias)
                    .cloned()
                    .unwrap_or_else(|| alias.to_string()),
                span: span_to_miette(span),
                source_id: span.source,
            });
            return None;
        }
        let internal = self.scope.qualified.get(&key)?;
        Some(match tail {
            Some(tail) => format!("{internal}::{tail}"),
            None => internal.clone(),
        })
    }

    /// A path in an expression or pattern: `Point::new`, `geo::Point::new`,
    /// `Shape::Circle`, `geo::Shape::Circle`, or a bare variant `Circle`.
    fn path(&mut self, name: &str, span: Span) -> String {
        self.reserved(name, span);
        match name.split_once("::") {
            Some((first, rest)) => {
                if let Some(path) = self.qualified_path(first, rest, span) {
                    return path;
                }
                match self.scope.types.get(first) {
                    Some(internal) => format!("{internal}::{rest}"),
                    None => name.to_string(),
                }
            }
            None => self.variant(name),
        }
    }

    /// A bare variant stays as written: lowering resolves it through the
    /// variants visible in this module (`Scoped::variants`).
    fn variant(&self, name: &str) -> String {
        name.to_string()
    }

    fn ty(&mut self, ty: &mut TypeName) {
        match ty {
            TypeName::Simple { name, span } => *name = self.type_name(name, *span),
            TypeName::Generic { name, args, span } => {
                *name = self.type_name(name, *span);
                args.iter_mut().for_each(|a| self.ty(a));
            }
            TypeName::Array { inner, .. } | TypeName::Option { inner, .. } => self.ty(inner),
            TypeName::Result { ok, err, .. } => {
                self.ty(ok);
                self.ty(err);
            }
            TypeName::Tuple { elems, .. } => elems.iter_mut().for_each(|e| self.ty(e)),
            TypeName::Function { params, ret, .. } => {
                params.iter_mut().for_each(|p| self.ty(p));
                if let Some(ret) = ret {
                    self.ty(ret);
                }
            }
        }
    }

    fn program(&mut self, program: &mut Program) {
        for item in &mut program.items {
            self.item(item);
        }
    }

    fn item(&mut self, item: &mut Item) {
        match item {
            Item::Import(_) => {}
            Item::TypeAlias(alias) => self.ty(&mut alias.ty),
            Item::Struct(s) => {
                s.name.name = self.type_name(&s.name.name, s.name.span);
                let outer = self.generics.len();
                self.generics.extend(s.type_params.iter().cloned());
                for field in &mut s.fields {
                    self.ty(&mut field.ty);
                }
                self.generics.truncate(outer);
            }
            Item::Enum(e) => {
                e.name.name = self.type_name(&e.name.name, e.name.span);
                for v in &mut e.variants {
                    match &mut v.payload {
                        Some(EnumPayload::Tuple(ty)) => self.ty(ty),
                        Some(EnumPayload::Struct(fields)) => {
                            fields.iter_mut().for_each(|(_, ty)| self.ty(ty))
                        }
                        None => {}
                    }
                }
            }
            Item::Trait(t) => {
                t.name.name = self.type_name(&t.name.name, t.name.span);
                for m in &mut t.methods {
                    m.params.iter_mut().for_each(|p| self.ty(&mut p.ty));
                    if let Some(ret) = &mut m.ret {
                        self.ty(ret);
                    }
                }
            }
            Item::Impl(imp) => {
                if let Some(t) = &mut imp.trait_name {
                    t.name = self.type_name(&t.name, t.span);
                }
                self.ty(&mut imp.target_type);
                for f in &mut imp.methods {
                    let outer = self.generics.len();
                    self.generics.extend(f.type_params.iter().cloned());
                    self.fn_parts(&mut f.params, &mut f.ret, &mut f.body);
                    self.generics.truncate(outer);
                }
            }
            Item::Event(e) => e.params.iter_mut().for_each(|p| self.ty(&mut p.ty)),
            Item::Fn(f) => {
                self.reserved(&f.name.name, f.name.span);
                let outer = self.generics.len();
                self.generics.extend(f.type_params.iter().cloned());
                self.fn_parts(&mut f.params, &mut f.ret, &mut f.body);
                self.generics.truncate(outer);
            }
            Item::RFn(f) => {
                let outer = self.generics.len();
                self.generics.extend(f.type_params.iter().cloned());
                self.fn_parts(&mut f.params, &mut f.ret, &mut f.body);
                self.generics.truncate(outer);
            }
            Item::Derived(d) => {
                self.let_pattern(&mut d.pattern);
                if let Some(ty) = &mut d.ty {
                    self.ty(ty);
                }
                self.expr(&mut d.value);
            }
            Item::Const(c) => {
                if let Some(ty) = &mut c.ty {
                    self.ty(ty);
                }
                self.expr(&mut c.value);
            }
            Item::Let(l) => self.let_stmt(l),
            Item::Watch(w) => self.block(&mut w.body),
            Item::On(o) => self.block(&mut o.body),
            Item::If(i) => self.if_stmt(i),
        }
    }

    fn fn_parts(
        &mut self,
        params: &mut [flux_syntax::Param],
        ret: &mut Option<TypeName>,
        body: &mut Block,
    ) {
        params.iter_mut().for_each(|p| self.ty(&mut p.ty));
        if let Some(ret) = ret {
            self.ty(ret);
        }
        self.block(body);
    }

    fn let_stmt(&mut self, l: &mut flux_syntax::LetStmt) {
        self.let_pattern(&mut l.pattern);
        if let Some(ty) = &mut l.ty {
            self.ty(ty);
        }
        self.expr(&mut l.value);
    }

    fn let_pattern(&mut self, pattern: &mut LetPattern) {
        match pattern {
            LetPattern::Ident(_) => {}
            LetPattern::Struct { name, .. } => name.name = self.type_name(&name.name, name.span),
            LetPattern::Tuple { elems, .. } => elems.iter_mut().for_each(|e| self.let_pattern(e)),
        }
    }

    fn block(&mut self, block: &mut Block) {
        for stmt in &mut block.stmts {
            self.stmt(stmt);
        }
    }

    fn if_stmt(&mut self, i: &mut IfStmt) {
        self.expr(&mut i.cond);
        self.block(&mut i.then_branch);
        match &mut i.else_branch {
            Some(ElseBranch::If(nested)) => self.if_stmt(nested),
            Some(ElseBranch::Block(b)) => self.block(b),
            None => {}
        }
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let(l) => self.let_stmt(l),
            Stmt::Assign(a) => {
                for step in &mut a.path {
                    if let PlaceStep::Index(index) = step {
                        self.expr(index);
                    }
                }
                self.expr(&mut a.value);
            }
            Stmt::Emit(e) => e.args.iter_mut().for_each(|a| self.expr(a)),
            Stmt::Return(r) => {
                if let Some(v) = &mut r.value {
                    self.expr(v);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => self.expr(e),
            Stmt::If(i) => self.if_stmt(i),
            Stmt::While(w) => {
                self.expr(&mut w.cond);
                self.block(&mut w.body);
            }
            Stmt::For(f) => {
                if let Some(p) = &mut f.pattern {
                    self.let_pattern(p);
                }
                self.expr(&mut f.iter);
                self.block(&mut f.body);
            }
            Stmt::Watch(w) => self.block(&mut w.body),
            Stmt::On(o) => self.block(&mut o.body),
        }
    }

    fn pattern(&mut self, pattern: &mut MatchPattern) {
        match pattern {
            MatchPattern::Simple(ident) => ident.name = self.path(&ident.name, ident.span),
            MatchPattern::Literal(e) => self.expr(e),
            MatchPattern::Tuple {
                variant,
                bindings,
                nested,
                ..
            } => {
                variant.name = self.path(&variant.name, variant.span);
                // A lone name may be a payload-free variant (`Some(Red)`).
                for b in bindings.iter_mut() {
                    b.name = self.variant(&b.name);
                }
                if let Some(nested) = nested {
                    self.pattern(nested);
                }
            }
            MatchPattern::Struct { variant, .. } => {
                variant.name = self.path(&variant.name, variant.span)
            }
            MatchPattern::TupleValue { elems, .. } => {
                elems.iter_mut().for_each(|e| self.pattern(e))
            }
        }
    }

    fn arm(&mut self, arm: &mut MatchArm) {
        self.pattern(&mut arm.pattern);
        if let Some(g) = &mut arm.guard {
            self.expr(g);
        }
        for stmt in &mut arm.stmts {
            self.stmt(stmt);
        }
        if let Some(e) = &mut arm.expr {
            self.expr(e);
        }
    }

    fn expr(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Int { .. } | Expr::Float { .. } | Expr::Bool { .. } | Expr::String { .. } => {}
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let StringPart::Expr(e) = part {
                        self.expr(e);
                    }
                }
            }
            Expr::Ident { name, span } => *name = self.path(name, *span),
            Expr::Call { callee, args, .. } => {
                self.expr(callee);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::Field { base, .. } => self.expr(base),
            Expr::Closure { params, body, .. } => {
                for p in params.iter_mut() {
                    if let Some(ty) = &mut p.ty {
                        self.ty(ty);
                    }
                }
                self.expr(body);
            }
            Expr::Unary { expr, .. }
            | Expr::Paren { inner: expr, .. }
            | Expr::Try { expr, .. }
            | Expr::Cast { expr, .. } => self.expr(expr),
            Expr::Binary { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.expr(cond);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            Expr::Array { elems, .. } | Expr::Tuple { elems, .. } => {
                elems.iter_mut().for_each(|e| self.expr(e))
            }
            Expr::StructLit { name, fields, .. } => {
                self.reserved(&name.name, name.span);
                // A struct, or a struct variant (`Rect { … }`, `Shape::Rect { … }`).
                name.name = if name.name.contains("::") {
                    self.path(&name.name, name.span)
                } else if self.scope.types.contains_key(&name.name) {
                    self.type_name(&name.name, name.span)
                } else {
                    self.variant(&name.name)
                };
                fields.iter_mut().for_each(|f| self.expr(&mut f.value));
            }
            Expr::With { base, fields, .. } => {
                self.expr(base);
                fields.iter_mut().for_each(|f| self.expr(&mut f.value));
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                arms.iter_mut().for_each(|a| self.arm(a));
            }
            Expr::Index { base, index, .. } => {
                self.expr(base);
                self.expr(index);
            }
        }
    }
}
