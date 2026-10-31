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
    AssignStmt, Block, ConstBinding, EnumDecl, EventDecl, FnDecl, ImportDecl, Item, LetPattern,
    LetStmt, OnStmt, Program, RFnDecl, Stmt, StructDecl, TraitDecl, WatchStmt,
};

use crate::nodes::*;
use crate::symbol::{Symbol, SymbolKind, SymbolTable};

use decls::{
    lower_const_binding, lower_derived, lower_enum, lower_event, lower_impl_fns, lower_module_if,
    lower_module_let, lower_struct, lower_trait,
};
use funcs::{lower_fn, lower_on, lower_rfn, lower_watch};

use modules::{lower_imported_items, register_imports};

use stdlib::register_stdlib_import;

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
    let mut names = HashSet::new();
    for item in &program.items {
        match item {
            Item::Watch(WatchStmt { body, .. }) => {
                collect_assigns_in_block(body, &HashSet::new(), &mut names)
            }
            Item::On(OnStmt { body, param, .. }) => {
                let locals = param.iter().map(|p| p.name.clone()).collect();
                collect_assigns_in_block(body, &locals, &mut names)
            }
            Item::Fn(FnDecl { body, params, .. }) | Item::RFn(RFnDecl { body, params, .. }) => {
                let locals = params.iter().map(|p| p.name.name.clone()).collect();
                collect_assigns_in_block(body, &locals, &mut names);
            }
            _ => {}
        }
    }
    names
}

fn collect_assigns_in_block(
    block: &Block,
    outer_locals: &HashSet<String>,
    names: &mut HashSet<String>,
) {
    let mut locals = outer_locals.clone();
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(LetStmt { pattern, .. }) => match pattern {
                LetPattern::Ident(id) => {
                    locals.insert(id.name.clone());
                }
                LetPattern::Struct { fields, .. } => {
                    locals.extend(fields.iter().map(|f| f.name.clone()));
                }
            },
            Stmt::Assign(AssignStmt { name, .. }) => {
                if !locals.contains(&name.name) {
                    names.insert(name.name.clone());
                }
            }
            Stmt::If(s) => collect_assigns_in_if(s, &locals, names),
            Stmt::While(w) => collect_assigns_in_block(&w.body, &locals, names),
            Stmt::For(f) => {
                let mut inner = locals.clone();
                inner.insert(f.name.name.clone());
                collect_assigns_in_block(&f.body, &inner, names);
            }
            Stmt::Watch(w) => collect_assigns_in_block(&w.body, &locals, names),
            Stmt::On(o) => collect_assigns_in_block(&o.body, &locals, names),
            _ => {}
        }
    }
}

fn collect_assigns_in_if(
    s: &flux_syntax::IfStmt,
    locals: &HashSet<String>,
    names: &mut HashSet<String>,
) {
    collect_assigns_in_block(&s.then_branch, locals, names);
    match &s.else_branch {
        Some(flux_syntax::ElseBranch::Block(b)) => collect_assigns_in_block(b, locals, names),
        Some(flux_syntax::ElseBranch::If(next)) => collect_assigns_in_if(next, locals, names),
        None => {}
    }
}

fn lower_program_inner(
    program: &Program,
    diagnostics: &mut DiagnosticBag,
    linked: Option<&LinkedProgram>,
) -> Option<HirProgram> {
    let mut symbols = SymbolTable::new();
    let reassigned_sources = collect_reassigned_sources(program);

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
        if !register_imports(linked, &mut symbols, diagnostics) {
            return None;
        }
    }

    let mut items = Vec::new();

    // Pass 1: declare all top-level names (events, functions, bindings).
    for item in &program.items {
        if let Some(err) = declare_item(item, &mut symbols) {
            push_duplicate(diagnostics, item, err);
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

    // Pass 2: lower imported modules, then root bodies.
    if let Some(linked) = linked {
        items.extend(lower_imported_items(
            linked,
            &mut symbols,
            diagnostics,
            &reassigned_sources,
        ));
    }

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

    let names = symbols.export_names();
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
