//! `live fn`: components.
//!
//! A `live fn Counter(start: Int) { … }` is lowered like a module inside its
//! file: each member (`let`, `let mut`, `derived`, `event`, `fn`) and each
//! parameter gets the internal name `__live_Counter__<name>`, and the body
//! sees them by their bare names. The members are ordinary module-level items
//! from then on, so every check of module state applies to them; the program
//! records which template each belongs to (`HirProgram::templates`), and the
//! constructor `Counter` is an `act fn` returning an instance. At run time
//! each call of it makes a fresh copy of the members.

use std::collections::HashSet;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_syntax::{Block, FnDecl, FnKind, Ident, Item, LetPattern, LiveDecl, Program, TypeName};

use crate::context::ScopeKind;
use crate::nodes::{HirItem, HirTemplate};
use crate::symbol::{Symbol, SymbolKind, SymbolTable};

use super::modules::{declare_item_renamed, item_export_name, item_is_pub, lower_item_renamed};

/// Internal name of member `member` of the component `ctor`.
pub fn member_name(ctor: &str, member: &str) -> String {
    format!("__live_{ctor}__{member}")
}

fn error(diagnostics: &mut DiagnosticBag, message: String, span: flux_diagnostics::Span) {
    diagnostics.push(FluxError::Parse {
        message,
        span: span_to_miette(span),
        source_id: span.source,
    });
}

/// Pass 1: declares the constructor `ctor` (the component's name, internal
/// in a module) and every parameter and member under its internal name.
pub(super) fn declare_live(
    decl: &LiveDecl,
    ctor: &str,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<Symbol> {
    match symbols.declare(
        ctor,
        SymbolKind::Function { reactive: false },
        decl.name.span,
    ) {
        Ok(id) => {
            symbols.live_ctors.insert(id);
        }
        Err(prev) => return Some(prev),
    }
    let mut seen = HashSet::new();
    for p in &decl.params {
        if p.mutable {
            error(
                diagnostics,
                format!(
                    "parameter `{}` of a live fn cannot be `mut`: an instance keeps its own copy",
                    p.name.name
                ),
                p.span,
            );
        }
        if !seen.insert(p.name.name.clone()) {
            error(
                diagnostics,
                format!("parameter `{}` is declared twice", p.name.name),
                p.name.span,
            );
            continue;
        }
        if let Ok(id) = symbols.declare(
            &member_name(ctor, &p.name.name),
            SymbolKind::Source,
            p.name.span,
        ) {
            // A parameter is fixed for the instance's life: reading it is
            // not a reactive read.
            symbols.constants.insert(id);
        }
    }
    for item in &decl.items {
        let pattern = match item {
            Item::Let(l) => Some(&l.pattern),
            Item::Derived(d) => Some(&d.pattern),
            _ => None,
        };
        if let Some(pattern) = pattern {
            if !matches!(pattern, LetPattern::Ident(_)) {
                error(
                    diagnostics,
                    "a member of a live fn names one value; destructure it in a function".into(),
                    pattern.span(),
                );
                continue;
            }
        }
        if let Item::Fn(f) = item {
            if !f.type_params.is_empty() {
                error(
                    diagnostics,
                    format!(
                        "function `{}` of a live fn cannot have type parameters",
                        f.name.name
                    ),
                    f.name.span,
                );
            }
            if f.mut_self || f.params.iter().any(|p| p.mutable) {
                error(
                    diagnostics,
                    format!(
                        "function `{}` of a live fn cannot take `mut` parameters",
                        f.name.name
                    ),
                    f.name.span,
                );
            }
        }
        let Some(name) = item_export_name(item) else {
            continue;
        };
        if !seen.insert(name.clone()) {
            error(
                diagnostics,
                format!("`{name}` is declared twice in live fn `{}`", decl.name.name),
                item_span(item),
            );
            continue;
        }
        if let Some(prev) = declare_item_renamed(item, &member_name(ctor, &name), symbols) {
            super::push_duplicate(diagnostics, item, prev);
        }
    }
    None
}

fn item_span(item: &Item) -> flux_diagnostics::Span {
    match item {
        Item::Event(e) => e.name.span,
        Item::Fn(f) => f.name.span,
        Item::Let(l) => l.pattern.span(),
        Item::Derived(d) => d.pattern.span(),
        Item::Watch(w) => w.span,
        Item::On(o) => o.span,
        _ => flux_diagnostics::Span::DUMMY,
    }
}

/// Pass 2: lowers the constructor (returned) and the members (left in
/// `symbols.pending_bindings`), and records the template.
pub(super) fn lower_live(
    decl: &LiveDecl,
    ctor: &str,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirItem> {
    let ctor_id = symbols.lookup(ctor)?.id;
    let constructor = FnDecl {
        mut_self: false,
        pub_: decl.pub_,
        kind: FnKind::Act,
        keyword: decl.name.span,
        name: Ident {
            name: ctor.to_string(),
            span: decl.name.span,
        },
        type_params: Vec::new(),
        type_bounds: Vec::new(),
        params: decl.params.clone(),
        ret: Some(TypeName::Simple {
            name: ctor.to_string(),
            span: decl.name.span,
        }),
        body: Block {
            stmts: Vec::new(),
            span: decl.span,
        },
        span: decl.span,
    };
    let ctor_fn = super::funcs::lower_fn(&constructor, symbols, diagnostics, false)?;

    let index = symbols.templates.len();
    let mut template = HirTemplate {
        ctor: ctor_id,
        name: ctor.to_string(),
        params: Vec::new(),
        members: Vec::new(),
        span: decl.span,
    };
    symbols.push(ScopeKind::Module);
    for p in &decl.params {
        let Some(id) = symbols
            .lookup(&member_name(ctor, &p.name.name))
            .map(|s| s.id)
        else {
            continue;
        };
        symbols.reexport_alias(&p.name.name, id, p.name.span);
        template.params.push((id, crate::nodes::lower_type(&p.ty)));
        symbols.template_members.insert(id, index);
    }
    for item in &decl.items {
        let Some(name) = item_export_name(item) else {
            continue;
        };
        let Some(id) = symbols.lookup(&member_name(ctor, &name)).map(|s| s.id) else {
            continue;
        };
        symbols.reexport_alias(&name, id, item_span(item));
        template.members.push((name, id, item_is_pub(item)));
        symbols.template_members.insert(id, index);
    }
    // Which members a handler or watch of the body assigns: those are
    // sources even without `mut` (as in a module).
    let body = Program {
        source: decl.span.source,
        items: decl.items.clone(),
    };
    let reassigned: HashSet<String> = super::collect_reassigned_sources(&body)
        .into_iter()
        .map(|n| member_name(ctor, &n))
        .collect();
    let mut members = Vec::new();
    for item in &decl.items {
        match item {
            Item::Watch(_) | Item::On(_) => {
                symbols.template_handlers.insert(item_span(item), index);
                if let Some(hir) = super::lower_item(item, symbols, diagnostics, &reassigned) {
                    members.push(hir);
                }
            }
            _ => {
                let Some(name) = item_export_name(item) else {
                    continue;
                };
                if let Some(hir) = lower_item_renamed(
                    item,
                    &member_name(ctor, &name),
                    symbols,
                    diagnostics,
                    &reassigned,
                ) {
                    members.push(hir);
                    members.extend(std::mem::take(&mut symbols.pending_bindings));
                }
            }
        }
    }
    symbols.pop();
    symbols.templates.push(template);
    symbols.pending_bindings.extend(members);
    Some(HirItem::Fn(ctor_fn))
}
