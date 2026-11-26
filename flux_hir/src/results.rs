//! E030: a collection function called only for a result nobody uses.
//!
//! The collection functions return a new value and leave their arguments as
//! they were, so `push(xs, 3)` alone does nothing. A block's last statement
//! may be its value, so it is checked only where the value is dropped: in
//! loop bodies, handlers, and watches.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxWarning};

use crate::nodes::*;
use crate::{SymbolId, SymbolKind};

/// The functions of the `collections` module.
const COLLECTION_FNS: &[&str] = &[
    "len",
    "range",
    "push",
    "set",
    "pop",
    "get",
    "map",
    "filter",
    "reduce",
    "map_new",
    "map_has",
    "map_remove",
    "map_keys",
    "map_values",
];

pub fn check_unused_results(hir: &HirProgram, diagnostics: &mut DiagnosticBag) {
    for item in &hir.items {
        match item {
            HirItem::Fn(f) => block(&f.body, false, hir, diagnostics),
            HirItem::Impl(i) => {
                for m in &i.methods {
                    block(&m.body, false, hir, diagnostics);
                }
            }
            HirItem::Watch(w) => block(&w.body, true, hir, diagnostics),
            HirItem::On(o) => block(&o.body, true, hir, diagnostics),
            HirItem::InitIf(i) => if_stmt(i, true, hir, diagnostics),
            _ => {}
        }
    }
}

/// `dropped`: whether the block's last statement is a dropped value too.
fn block(b: &HirBlock, dropped: bool, hir: &HirProgram, diagnostics: &mut DiagnosticBag) {
    let last = b.stmts.len().saturating_sub(1);
    for (i, s) in b.stmts.iter().enumerate() {
        match s {
            HirStmt::Expr(e) if i < last || dropped => check(e, hir, diagnostics),
            HirStmt::If(branch) => if_stmt(branch, i < last || dropped, hir, diagnostics),
            HirStmt::While(w) => block(&w.body, true, hir, diagnostics),
            HirStmt::For(f) => block(&f.body, true, hir, diagnostics),
            _ => {}
        }
    }
}

/// `dropped`: whether the `if`'s value is dropped, and with it the value of
/// each branch's last statement.
fn if_stmt(i: &HirIf, dropped: bool, hir: &HirProgram, diagnostics: &mut DiagnosticBag) {
    block(&i.then_branch, dropped, hir, diagnostics);
    match &i.else_branch {
        Some(HirElse::If(next)) => if_stmt(next, dropped, hir, diagnostics),
        Some(HirElse::Block(b)) => block(b, dropped, hir, diagnostics),
        None => {}
    }
}

/// Library methods that only compute a value (the changing ones, like
/// `xs.push(1)`, store their result back).
const COMPUTING_METHODS: &[&str] = &[
    "len",
    "get",
    "map",
    "filter",
    "reduce",
    "join",
    "contains",
    "find",
    "any",
    "all",
    "enumerate",
    "zip",
    "slice",
    "sorted",
    "sorted_by",
    "reversed",
    "first",
    "last",
    "has",
    "keys",
    "values",
    "entries",
    "get_or",
];

fn check(e: &HirExpr, hir: &HirProgram, diagnostics: &mut DiagnosticBag) {
    let HirExpr::Call { callee, span, .. } = e else {
        return;
    };
    if let HirExpr::Field { field, .. } = callee.as_ref() {
        if COMPUTING_METHODS.contains(&field.as_str()) && !user_named(field, hir) {
            diagnostics.push_warning(FluxWarning::UnusedResult {
                name: field.clone(),
                help: format!("`.{field}()` only computes a value: use it, or remove the call"),
                span: span_to_miette(*span),
                source_id: span.source,
            });
        }
        return;
    }
    let HirExpr::Ident { symbol, .. } = callee.as_ref() else {
        return;
    };
    // An instance nobody holds is dropped at once, with its handlers.
    if let Some(t) = hir.templates.iter().find(|t| t.ctor == *symbol) {
        let shown = crate::display_path(&t.name);
        diagnostics.push_warning(FluxWarning::UnusedResult {
            name: shown.clone(),
            help: format!("an instance lives while something holds it: write `let x = {shown}(…)`"),
            span: span_to_miette(*span),
            source_id: span.source,
        });
        return;
    }
    let Some(name) = collection_fn(*symbol, hir) else {
        return;
    };
    let help = match name {
        "push" => "it returns a new array: write `xs = push(xs, v)`".to_string(),
        "pop" => "it returns a new array: write `xs = pop(xs)`".to_string(),
        "set" => "it returns a new collection: write `xs[i] = v`".to_string(),
        "map_remove" => "it returns a new map: write `m = map_remove(m, k)`".to_string(),
        _ => format!("`{name}` only computes a value: use it, or remove the call"),
    };
    diagnostics.push_warning(FluxWarning::UnusedResult {
        name: name.to_string(),
        help,
        span: span_to_miette(*span),
        source_id: span.source,
    });
}

/// The library name of a collection function `symbol` refers to; a user
/// function of the same name is not one.
fn collection_fn(symbol: SymbolId, hir: &HirProgram) -> Option<&'static str> {
    let sym = hir.symbols.get(&symbol)?;
    if sym.kind != SymbolKind::Builtin {
        return None;
    }
    COLLECTION_FNS.iter().copied().find(|n| *n == sym.name)
}

/// Whether the program gives `name` its own meaning as a method (an `impl`)
/// or a struct field, which a call through `.` may reach instead.
fn user_named(name: &str, hir: &HirProgram) -> bool {
    hir.trait_impls.keys().any(|(_, m)| m == name)
        || hir.items.iter().any(|item| match item {
            HirItem::Struct(st) => st.fields.iter().any(|(f, _)| f == name),
            HirItem::Trait(t) => t.methods.iter().any(|m| m.0 == name),
            HirItem::Impl(i) => i.methods.iter().any(|m| {
                hir.names
                    .get(&m.name)
                    .is_some_and(|n| n == name || n.ends_with(&format!("::{name}")))
            }),
            _ => false,
        })
}
