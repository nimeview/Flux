//! AST → HIR lowering driver.
//!
//! Lowering is split into focused submodules:
//! - [`decls`]     — top-level declarations (struct/enum/trait/impl/event/binding)
//! - [`funcs`]     — `fn` / `rfn` and `watch` / `on` effect lowering
//! - [`stmts`]     — block, statement, match-arm and `if` lowering
//! - [`exprs`]     — expression lowering, closure desugaring, name resolution
//! - [`constfold`] — module-level constant folding
//!
//! This file keeps the two-pass driver ([`lower_program`]): pass 1 declares all
//! top-level names, pass 2 lowers bodies and collects generics / trait impls.

mod constfold;
mod decls;
mod exprs;
mod funcs;
mod modules;
mod stdlib;
mod stmts;

use std::collections::{HashMap, HashSet};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_resolve::{link_modules, root_program, LinkedProgram, ResolvedProgram};
use flux_syntax::{
    Block, ConstBinding, EnumDecl, EventDecl, FnDecl, ImportDecl, Item, LetPattern, Program,
    RFnDecl, Stmt, StructDecl, TraitDecl,
};

use crate::nodes::*;
use crate::symbol::{Symbol, SymbolId, SymbolKind, SymbolTable};

use decls::{
    lower_const_binding, lower_derived, lower_enum, lower_event, lower_impl_fns, lower_module_if,
    lower_module_let, lower_struct, lower_trait,
};
use funcs::{lower_fn, lower_on, lower_rfn, lower_watch};

use modules::{bind_root_imports, declare_module_items, lower_module_items};

use stdlib::{declare_prelude, register_stdlib_import};

use crate::unused::check_unused_bindings;

pub fn lower_program(program: &Program, diagnostics: &mut DiagnosticBag) -> Option<HirProgram> {
    lower_program_inner(program, diagnostics, None)
}

/// Lower a multi-module [`ResolvedProgram`] with qualified import names.
pub fn lower_linked_program(
    resolved: &ResolvedProgram,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirProgram> {
    let linked = link_modules(resolved, diagnostics)?;
    let program = root_program(&linked);
    lower_program_inner(&program, diagnostics, Some(&linked))
}

/// Module bindings assigned from a `watch`/`on`/fn body, which makes them
/// sources. An assignment to a name bound locally (param, `for` variable, or a
/// `let` earlier in an enclosing block) targets that local, not the module.
fn collect_reassigned_sources(program: &Program) -> HashSet<String> {
    let mut collector = AssignmentCollector {
        names: HashSet::new(),
        // Like is_variant_name in stmts: only None is special without a declaration.
        variants: HashSet::from(["None".into()]),
    };
    for item in &program.items {
        if let Item::Enum(e) = item {
            collector
                .variants
                .extend(e.variants.iter().map(|v| v.name.name.clone()));
        }
    }
    for item in &program.items {
        let locals = AssignmentLocals::default();
        match item {
            Item::Watch(w) => collector.block(&w.body, &locals),
            Item::On(o) => {
                let locals = o.param.iter().map(|p| p.name.clone()).collect();
                collector.block(&o.body, &locals);
            }
            Item::Fn(FnDecl { body, params, .. }) | Item::RFn(RFnDecl { body, params, .. }) => {
                let locals = params.iter().map(|p| p.name.name.clone()).collect();
                collector.block(body, &locals);
            }
            Item::Impl(i) => {
                for method in &i.methods {
                    let locals = method.params.iter().map(|p| p.name.name.clone()).collect();
                    collector.block(&method.body, &locals);
                }
            }
            Item::Let(l) => collector.expr(&l.value, &locals),
            Item::Derived(d) => collector.expr(&d.value, &locals),
            Item::Const(c) => collector.expr(&c.value, &locals),
            Item::If(s) => collector.if_stmt(s, &locals),
            Item::Import(_)
            | Item::TypeAlias(_)
            | Item::Struct(_)
            | Item::Enum(_)
            | Item::Trait(_)
            | Item::Event(_) => {}
        }
    }
    collector.names
}

/// Borrow the enclosing scope rather than copying all preceding local names
/// for every closure, match arm, or block. This type intentionally is not Clone.
#[derive(Default)]
struct AssignmentLocals<'a> {
    outer: Option<&'a AssignmentLocals<'a>>,
    names: HashSet<String>,
}

impl AssignmentLocals<'_> {
    fn child(&self) -> AssignmentLocals<'_> {
        AssignmentLocals {
            outer: Some(self),
            names: HashSet::new(),
        }
    }
    fn insert(&mut self, name: String) {
        self.names.insert(name);
    }
    fn contains(&self, name: &str) -> bool {
        self.names.contains(name) || self.outer.is_some_and(|outer| outer.contains(name))
    }
}

impl Extend<String> for AssignmentLocals<'_> {
    fn extend<T: IntoIterator<Item = String>>(&mut self, names: T) {
        self.names.extend(names);
    }
}

impl FromIterator<String> for AssignmentLocals<'_> {
    fn from_iter<T: IntoIterator<Item = String>>(names: T) -> Self {
        let mut locals = Self::default();
        locals.extend(names);
        locals
    }
}

struct AssignmentCollector {
    names: HashSet<String>,
    variants: HashSet<String>,
}

impl AssignmentCollector {
    fn block(&mut self, block: &Block, locals: &AssignmentLocals<'_>) {
        self.stmts(&block.stmts, &mut locals.child());
    }

    fn stmts(&mut self, stmts: &[Stmt], locals: &mut AssignmentLocals<'_>) {
        for stmt in stmts {
            match stmt {
                Stmt::Let(l) => {
                    // Match lower_local_decl: a simple name is declared before
                    // its value, destructured fields after their value.
                    if let LetPattern::Ident(id) = &l.pattern {
                        locals.insert(id.name.clone());
                    }
                    self.expr(&l.value, locals);
                    locals.extend(l.pattern.binding_names());
                }
                Stmt::Assign(a) => {
                    if !locals.contains(&a.name.name) {
                        self.names.insert(a.name.name.clone());
                    }
                    for index in &a.indices {
                        self.expr(index, locals);
                    }
                    self.expr(&a.value, locals);
                }
                Stmt::Expr(e) => self.expr(e, locals),
                Stmt::Return(r) => {
                    if let Some(value) = &r.value {
                        self.expr(value, locals);
                    }
                }
                Stmt::Emit(e) => {
                    for arg in &e.args {
                        self.expr(arg, locals);
                    }
                }
                Stmt::If(s) => self.if_stmt(s, locals),
                Stmt::While(w) => {
                    self.expr(&w.cond, locals);
                    self.block(&w.body, locals);
                }
                Stmt::For(f) => {
                    let mut inner = locals.child();
                    inner.insert(f.name.name.clone());
                    self.expr(&f.iter, &inner);
                    self.block(&f.body, &inner);
                }
                Stmt::Watch(w) => self.block(&w.body, locals),
                Stmt::On(o) => {
                    let mut inner = locals.child();
                    inner.extend(o.param.iter().map(|p| p.name.clone()));
                    self.block(&o.body, &inner);
                }
                Stmt::Break(_) | Stmt::Continue(_) => {}
            }
        }
    }

    fn if_stmt(&mut self, s: &flux_syntax::IfStmt, locals: &AssignmentLocals<'_>) {
        self.expr(&s.cond, locals);
        self.block(&s.then_branch, locals);
        match &s.else_branch {
            Some(flux_syntax::ElseBranch::Block(b)) => self.block(b, locals),
            Some(flux_syntax::ElseBranch::If(next)) => self.if_stmt(next, locals),
            None => {}
        }
    }

    fn pattern_locals(
        &self,
        pattern: &flux_syntax::MatchPattern,
        locals: &mut AssignmentLocals<'_>,
    ) {
        use flux_syntax::MatchPattern;
        match pattern {
            MatchPattern::Simple(id) => {
                if id.name != "_" && !self.variants.contains(&id.name) {
                    locals.insert(id.name.clone());
                }
            }
            MatchPattern::Tuple {
                bindings, nested, ..
            } => {
                if let Some(nested) = nested {
                    self.pattern_locals(nested, locals);
                } else if !(bindings.len() == 1 && self.variants.contains(&bindings[0].name)) {
                    locals.extend(bindings.iter().map(|id| id.name.clone()));
                }
            }
            MatchPattern::Struct { fields, .. } => {
                locals.extend(fields.iter().map(|id| id.name.clone()));
            }
            MatchPattern::Literal(_) => {}
        }
    }

    fn expr(&mut self, expr: &flux_syntax::Expr, locals: &AssignmentLocals<'_>) {
        use flux_syntax::{Expr, StringPart};
        match expr {
            Expr::Int { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::String { .. }
            | Expr::Ident { .. } => {}
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    match part {
                        StringPart::Expr(e) => self.expr(e, locals),
                        StringPart::Text(_) => {}
                    }
                }
            }
            Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Try { expr, .. } => {
                self.expr(expr, locals);
            }
            Expr::Field { base, .. } => self.expr(base, locals),
            Expr::Binary { left, right, .. }
            | Expr::Index {
                base: left,
                index: right,
                ..
            } => {
                self.expr(left, locals);
                self.expr(right, locals);
            }
            Expr::Call { callee, args, .. } => {
                self.expr(callee, locals);
                for arg in args {
                    self.expr(arg, locals);
                }
            }
            Expr::Closure { params, body, .. } => {
                let mut inner = locals.child();
                inner.extend(params.iter().map(|p| p.name.name.clone()));
                self.expr(body, &inner);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.expr(cond, locals);
                self.expr(then_expr, locals);
                self.expr(else_expr, locals);
            }
            Expr::Array { elems, .. } => {
                for elem in elems {
                    self.expr(elem, locals);
                }
            }
            Expr::StructLit { fields, .. } => {
                for field in fields {
                    self.expr(&field.value, locals);
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, locals);
                for arm in arms {
                    let mut inner = locals.child();
                    self.pattern_locals(&arm.pattern, &mut inner);
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, &inner);
                    }
                    self.stmts(&arm.stmts, &mut inner);
                    if let Some(value) = &arm.expr {
                        self.expr(value, &inner);
                    }
                }
            }
        }
    }
}

fn lower_program_inner(
    program: &Program,
    diagnostics: &mut DiagnosticBag,
    linked: Option<&LinkedProgram>,
) -> Option<HirProgram> {
    let mut symbols = SymbolTable::new();
    let reassigned_sources = collect_reassigned_sources(program);

    // Imported modules are declared and lowered first, each in its own
    // scope, before any root name exists, so a module can never see them.
    let mut items = Vec::new();
    if let Some(linked) = linked {
        declare_module_items(linked, &mut symbols, diagnostics);
        if diagnostics.has_errors() {
            return None;
        }
        items.extend(lower_module_items(
            linked,
            &mut symbols,
            diagnostics,
            &reassigned_sources,
        ));
    }

    // Standard-library imports (`import io`, `import math`, …) register host intrinsics.
    // Linked programs strip `import` items from the merged root AST — read them from the root module.
    let stdlib_imports: Vec<&ImportDecl> = if let Some(linked) = linked {
        linked
            .modules
            .first()
            .map(|root| {
                root.ast
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Import(import) => Some(import),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        program
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Import(import) => Some(import),
                _ => None,
            })
            .collect()
    };
    for import in stdlib_imports {
        register_stdlib_import(import, &mut symbols, diagnostics);
    }

    if let Some(linked) = linked {
        bind_root_imports(linked, &mut symbols);
    }

    // Pass 1: declare all top-level names (events, functions, bindings).
    for item in &program.items {
        if let Some(err) = declare_item(item, &mut symbols) {
            let (name, span) = item_name_span(item);
            if symbols.imported_names.contains(&name) {
                diagnostics.push(FluxError::ImportedNameTaken {
                    name,
                    span: span_to_miette(span),
                    source_id: span.source,
                });
            } else {
                push_duplicate(diagnostics, item, err);
            }
        }
        if let Item::Let(l) = item {
            if !l.mutable {
                for name in l.pattern.binding_names() {
                    if let Some(id) = symbols.lookup(&name).map(|s| s.id) {
                        symbols.immutable_lets.insert(id);
                    }
                }
            }
        }
        if let Item::Enum(e) = item {
            for v in &e.variants {
                symbols
                    .enum_variants
                    .insert(v.name.name.clone(), e.name.name.clone());
            }
        }
    }

    if diagnostics.has_errors() {
        return None;
    }
    declare_prelude(&mut symbols, &program.items);

    // Pass 2: lower root bodies (imported modules are already lowered).

    let mut type_aliases: HashMap<String, HirType> = HashMap::new();
    let mut extra_fns: Vec<HirItem> = Vec::new();
    for item in &program.items {
        if let Item::TypeAlias(decl) = item {
            type_aliases.insert(decl.name.name.clone(), lower_type(&decl.ty));
        }
        if let Some(hir_item) = lower_item(item, &mut symbols, diagnostics, &reassigned_sources) {
            items.push(hir_item);
            items.extend(std::mem::take(&mut symbols.pending_bindings));
        }
        if let Item::Impl(impl_decl) = item {
            if let Some(extra) = lower_impl_fns(impl_decl, &mut symbols, diagnostics) {
                extra_fns.extend(extra);
            }
        }
    }
    items.extend(extra_fns);
    items.extend(std::mem::take(&mut symbols.pending_fns));
    check_alias_cycles(&program.items, &type_aliases, diagnostics);
    check_inhabited_types(&items, &symbols.export_names(), &type_aliases, diagnostics);

    let names = symbols.export_names();
    let impls = std::mem::take(&mut symbols.pending_impls);
    decls::validate_impls(&impls, &items, &names, diagnostics);
    let generic_fns: HashMap<String, GenericFnTemplate> = items
        .iter()
        .filter_map(|item| match item {
            HirItem::Fn(f) if !f.type_params.is_empty() => names.get(&f.name).map(|name| {
                (
                    name.clone(),
                    GenericFnTemplate {
                        type_params: f.type_params.clone(),
                        fn_item: f.clone(),
                    },
                )
            }),
            _ => None,
        })
        .collect();

    let trait_impls = symbols.trait_impl_index.clone();

    if diagnostics.has_errors() {
        None
    } else {
        let hir = HirProgram {
            items,
            names,
            enum_variants: symbols.enum_variants.clone(),
            trait_impls,
            generic_fns,
            type_aliases,
            qualified_symbols: symbols.qualified_symbols.clone(),
            const_bindings: symbols.const_bindings.clone(),
            symbols: symbols.export_symbols(),
        };
        check_unused_bindings(&hir, diagnostics);
        Some(hir)
    }
}

/// A type whose every value must contain another value of itself has no
/// values: `struct S { me: S }`. Starting from nothing, a type gets values once
/// a struct's fields all have some, or one of an enum's variants does.
fn check_inhabited_types(
    items: &[HirItem],
    names: &HashMap<SymbolId, String>,
    aliases: &HashMap<String, HirType>,
    diagnostics: &mut DiagnosticBag,
) {
    fn has_values(
        ty: &HirType,
        user_types: &HashSet<&str>,
        inhabited: &HashSet<&str>,
        aliases: &HashMap<String, HirType>,
    ) -> bool {
        let check = |t: &HirType| has_values(t, user_types, inhabited, aliases);
        match ty {
            HirType::Named { name, .. } => match aliases.get(name) {
                _ if user_types.contains(name.as_str()) => inhabited.contains(name.as_str()),
                Some(aliased) => check(aliased),
                None => true,
            },
            // `None`, `[]`, an empty map, and any function exist without a value inside.
            HirType::Generic { name, args, .. } if name == "Result" && args.len() == 2 => {
                check(&args[0]) || check(&args[1])
            }
            HirType::Result { ok, err, .. } => check(ok) || check(err),
            _ => true,
        }
    }
    let user_types: HashSet<&str> = items
        .iter()
        .filter_map(|item| match item {
            HirItem::Struct(s) => names.get(&s.name).map(String::as_str),
            HirItem::Enum(e) => names.get(&e.name).map(String::as_str),
            _ => None,
        })
        .collect();
    let mut inhabited: HashSet<&str> = HashSet::new();
    loop {
        let before = inhabited.len();
        for item in items {
            let (name, has) = match item {
                HirItem::Struct(s) => (
                    &s.name,
                    s.fields
                        .iter()
                        .all(|(_, ty)| has_values(ty, &user_types, &inhabited, aliases)),
                ),
                HirItem::Enum(e) => (
                    &e.name,
                    e.variants.iter().any(|(_, payload)| match payload {
                        None => true,
                        Some(HirEnumPayload::Tuple(ty)) => {
                            has_values(ty, &user_types, &inhabited, aliases)
                        }
                        Some(HirEnumPayload::Struct(fields)) => fields
                            .iter()
                            .all(|(_, ty)| has_values(ty, &user_types, &inhabited, aliases)),
                    }),
                ),
                _ => continue,
            };
            if has {
                if let Some(name) = names.get(name) {
                    inhabited.insert(name);
                }
            }
        }
        if inhabited.len() == before {
            break;
        }
    }
    for item in items {
        let (id, span) = match item {
            HirItem::Struct(s) => (&s.name, s.span),
            HirItem::Enum(e) => (&e.name, e.span),
            _ => continue,
        };
        if let Some(name) = names.get(id) {
            if !inhabited.contains(name.as_str()) {
                diagnostics.push(FluxError::UninhabitedType {
                    name: name.clone(),
                    span: span_to_miette(span),
                    source_id: span.source,
                });
            }
        }
    }
}

/// Resolving an alias follows the aliases it names, so a cycle would never end.
fn check_alias_cycles(
    items: &[Item],
    aliases: &HashMap<String, HirType>,
    diagnostics: &mut DiagnosticBag,
) {
    fn names_in(ty: &HirType, out: &mut Vec<String>) {
        match ty {
            HirType::Named { name, .. } | HirType::TypeParam { name, .. } => {
                out.push(name.clone());
            }
            HirType::Generic { name, args, .. } => {
                out.push(name.clone());
                args.iter().for_each(|arg| names_in(arg, out));
            }
            HirType::Array { inner, .. } | HirType::Option { inner, .. } => names_in(inner, out),
            HirType::Result { ok, err, .. } => {
                names_in(ok, out);
                names_in(err, out);
            }
            HirType::Function { params, ret, .. } => {
                params.iter().for_each(|param| names_in(param, out));
                if let Some(ret) = ret {
                    names_in(ret, out);
                }
            }
        }
    }
    for item in items {
        let Item::TypeAlias(decl) = item else {
            continue;
        };
        let name = &decl.name.name;
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![name.clone()];
        let mut cyclic = false;
        while let Some(next) = pending.pop() {
            let Some(aliased) = aliases.get(&next) else {
                continue;
            };
            let mut refs = Vec::new();
            names_in(aliased, &mut refs);
            cyclic |= refs.contains(name);
            pending.extend(refs.into_iter().filter(|r| seen.insert(r.clone())));
        }
        if cyclic {
            diagnostics.push(FluxError::CyclicTypeAlias {
                name: name.clone(),
                span: span_to_miette(decl.name.span),
                source_id: decl.name.span.source,
            });
        }
    }
}

pub(super) fn declare_item(item: &Item, symbols: &mut SymbolTable) -> Option<Symbol> {
    match item {
        Item::Event(EventDecl { name, .. }) => symbols
            .declare(&name.name, SymbolKind::Event, name.span)
            .err(),
        Item::Fn(FnDecl { name, .. }) => symbols
            .declare(
                &name.name,
                SymbolKind::Function { reactive: false },
                name.span,
            )
            .err(),
        Item::RFn(RFnDecl { name, .. }) => symbols
            .declare(
                &name.name,
                SymbolKind::Function { reactive: true },
                name.span,
            )
            .err(),
        Item::Struct(StructDecl { name, .. }) => symbols
            .declare(&name.name, SymbolKind::Builtin, name.span)
            .err(),
        Item::Enum(EnumDecl { name, .. }) => symbols
            .declare(&name.name, SymbolKind::Builtin, name.span)
            .err(),
        Item::Trait(TraitDecl { name, .. }) => symbols
            .declare(&name.name, SymbolKind::Builtin, name.span)
            .err(),
        Item::Impl(_) => None,
        Item::Const(ConstBinding { name, .. }) => {
            match symbols.declare(&name.name, SymbolKind::Source, name.span) {
                Ok(id) => {
                    symbols.constants.insert(id);
                    None
                }
                Err(prev) => Some(prev),
            }
        }
        Item::Derived(stmt) => {
            for binding_name in stmt.pattern.binding_names() {
                if symbols
                    .declare(&binding_name, SymbolKind::Source, stmt.pattern.span())
                    .is_err()
                {
                    if let Some(sym) = symbols.lookup(&binding_name) {
                        return Some(sym.clone());
                    }
                }
            }
            None
        }
        Item::Let(let_stmt) => {
            for binding_name in let_stmt.pattern.binding_names() {
                if symbols
                    .declare(&binding_name, SymbolKind::Source, let_stmt.pattern.span())
                    .is_err()
                {
                    if let Some(sym) = symbols.lookup(&binding_name) {
                        return Some(sym.clone());
                    }
                }
            }
            None
        }
        Item::Watch(_) | Item::On(_) | Item::If(_) | Item::Import(_) | Item::TypeAlias(_) => None,
    }
}

pub(super) fn push_duplicate(diagnostics: &mut DiagnosticBag, item: &Item, prev: Symbol) {
    let (name, span) = item_name_span(item);
    diagnostics.push(FluxError::DuplicateDefinition {
        name,
        prev_span: span_to_miette(prev.span),
        span: span_to_miette(span),
        source_id: span.source,
    });
}

fn item_name_span(item: &Item) -> (String, flux_diagnostics::Span) {
    match item {
        Item::Event(e) => (e.name.name.clone(), e.span),
        Item::Fn(f) => (f.name.name.clone(), f.span),
        Item::RFn(f) => (f.name.name.clone(), f.span),
        Item::Const(c) => (c.name.name.clone(), c.span),
        Item::Let(l) => (
            l.pattern.primary_name().unwrap_or("let").to_string(),
            l.span,
        ),
        Item::Derived(d) => (
            d.pattern.primary_name().unwrap_or("derived").to_string(),
            d.span,
        ),
        Item::Watch(w) => ("watch".into(), w.span),
        Item::On(o) => (o.event.name.clone(), o.span),
        Item::If(i) => ("if".into(), i.span),
        Item::Import(i) => (i.path.clone(), i.span),
        Item::Struct(s) => (s.name.name.clone(), s.span),
        Item::Enum(e) => (e.name.name.clone(), e.span),
        Item::Trait(t) => (t.name.name.clone(), t.span),
        Item::Impl(i) => (i.trait_name.name.clone(), i.span),
        Item::TypeAlias(t) => (t.name.name.clone(), t.span),
    }
}

pub(super) fn lower_item(
    item: &Item,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reassigned_sources: &HashSet<String>,
) -> Option<HirItem> {
    match item {
        Item::Event(decl) => lower_event(decl, symbols),
        Item::Const(decl) => lower_const_binding(decl, symbols, diagnostics).map(HirItem::Binding),
        Item::Derived(stmt) => lower_derived(stmt, symbols, diagnostics),
        Item::Let(let_stmt) => lower_module_let(let_stmt, symbols, diagnostics, reassigned_sources),
        Item::Fn(decl) => lower_fn(decl, symbols, diagnostics, false).map(HirItem::Fn),
        Item::RFn(decl) => lower_rfn(decl, symbols, diagnostics).map(HirItem::Fn),
        Item::Watch(stmt) => lower_watch(stmt, symbols, diagnostics).map(HirItem::Watch),
        Item::On(stmt) => lower_on(stmt, symbols, diagnostics).map(HirItem::On),
        Item::If(stmt) => lower_module_if(stmt, symbols, diagnostics).map(HirItem::InitIf),
        Item::Import(_) => None,
        Item::Struct(decl) => lower_struct(decl, symbols).map(HirItem::Struct),
        Item::Enum(decl) => lower_enum(decl, symbols).map(HirItem::Enum),
        Item::Trait(decl) => lower_trait(decl, symbols).map(HirItem::Trait),
        Item::Impl(_) => None, // methods lowered separately in lower_impl_fns
        Item::TypeAlias(_) => None,
    }
}

#[cfg(test)]
mod assignment_discovery_tests {
    use super::*;
    use flux_diagnostics::SourceId;

    fn assigned(source: &str) -> HashSet<String> {
        let mut diagnostics = DiagnosticBag::new();
        let ast = flux_syntax::parse_program(SourceId(0), source, &mut diagnostics)
            .unwrap_or_else(|| panic!("{source}: {:?}", diagnostics.errors));
        assert!(
            !diagnostics.has_errors(),
            "{source}: {:?}",
            diagnostics.errors
        );
        collect_reassigned_sources(&ast)
    }

    #[test]
    fn finds_assignments_through_expression_and_statement_positions() {
        let expr = "match 0 { _ => { if true { x = 1 } 0 } }";
        // Parse only: later type/effect restrictions are independent of this prepass.
        for body in [
            expr.to_string(),
            format!("let y = {expr}"),
            format!("let mut y = 0\ny = {expr}"),
            format!("let mut y = [0]\ny[{expr}] = 1"),
            format!("return {expr}"),
            format!("emit ev({expr})"),
            format!("if {expr} {{ 0 }} else if {expr} {{ 1 }}"),
            format!("while {expr} {{ break }}"),
            format!("for y in {expr} {{ continue }}"),
            format!("let f = |y| {expr}"),
            format!("let y = -({expr})"),
            format!("let y = 1 + ({expr})"),
            format!("let y = ({expr}) as Int"),
            format!("let y = ({expr})?"),
            format!("let y = ({expr}).total"),
            format!("let y = ({expr})[0]"),
            format!("let y = [0][{expr}]"),
            format!("let y = f({expr})"),
            format!("let y = ({expr})(0)"),
            format!("let y = [{expr}]"),
            format!("let y = S {{ total: {expr} }}"),
            format!("let y = \"value={{{expr}}}\""),
            format!("let y = if true {{ {expr} }} else {{ 0 }}"),
            format!("let y = if true {{ 0 }} else {{ {expr} }}"),
            format!("match {expr} {{ _ => 0 }}"),
            format!("match 0 {{ _ if {expr} => 0, _ => 1 }}"),
            format!("match 0 {{ _ => {expr} }}"),
        ] {
            let source = format!("fn main() {{ {body} }}\n");
            assert_eq!(assigned(&source), HashSet::from(["x".into()]), "{source}");
        }
    }

    #[test]
    fn scopes_arm_bindings_and_locals_through_the_final_expression() {
        let write = "match 0 { _ => { if true { x = 1 } 0 } }";
        for pattern in ["x", "Some(x)", "Some(Some(x))", "Wrap { x }"] {
            let source = format!("fn f() {{ match 0 {{ {pattern} if {write} => {{ let y = {write}\n{write} }}, _ => 0 }} }}\n");
            assert!(assigned(&source).is_empty(), "{source}");
        }
        let source = format!("fn f() {{ match 0 {{ _ => {{ let mut x = 0\n{write} }} }} }}\n");
        assert!(assigned(&source).is_empty());
        // Neither an arm binding nor a nested block's local escapes its scope.
        for body in [
            format!("match 0 {{ x => 0, _ => {write} }}"),
            format!("if true {{ let x = 0 }}\n{write}"),
        ] {
            assert_eq!(
                assigned(&format!("fn f() {{ {body} }}\n")),
                HashSet::from(["x".into()])
            );
        }
    }

    #[test]
    fn respects_existing_initializer_loop_closure_and_handler_scopes() {
        let write = "match 0 { _ => { if true { x = 1 } 0 } }";
        for body in [
            format!("let mut x = {write}"),
            format!("let mut x = 0\n{write}"),
            format!("for x in [{write}] {{ {write} }}"),
            format!("let f = |x| {write}"),
            format!("on ev {{ x -> {write} }}"),
        ] {
            assert!(
                assigned(&format!("fn f() {{ {body} }}\n")).is_empty(),
                "{body}"
            );
        }
        // Destructured fields enter scope after their value is evaluated.
        let source = format!("fn f() {{ let S {{ x }} = {write} }}\n");
        assert_eq!(assigned(&source), HashSet::from(["x".into()]));
        let source = format!("fn f() {{ let c = |x| 0\n{write} }}\n");
        assert_eq!(assigned(&source), HashSet::from(["x".into()]));
    }

    #[test]
    fn variant_names_are_not_arm_locals() {
        let source = "enum E { Empty }\nfn f() { match 0 { Some(Empty) => { if true { Empty = 1 } 0 }, _ => 0 } }\n";
        assert_eq!(assigned(source), HashSet::from(["Empty".into()]));
    }

    #[test]
    fn finds_assignments_in_module_closure_values() {
        let expr = "|t| match 0 { _ => { if true { x = 1 } 0 } }";
        for decl in [
            format!("let f = {expr}"),
            format!("const f = {expr}"),
            format!("derived f = {expr}"),
        ] {
            assert_eq!(assigned(&format!("{decl}\n")), HashSet::from(["x".into()]));
        }
    }
}
