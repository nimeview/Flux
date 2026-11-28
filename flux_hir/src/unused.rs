//! Post-lowering pass: emit E015 for module bindings and locals that are
//! never read.

use std::collections::HashSet;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxWarning};

use crate::nodes::*;
use crate::symbol::SymbolId;

/// The names read anywhere, and the locals declared (with their `let`).
#[derive(Default)]
struct Uses {
    read: HashSet<SymbolId>,
    locals: Vec<(SymbolId, flux_diagnostics::Span)>,
}

impl Uses {
    fn insert(&mut self, id: SymbolId) {
        self.read.insert(id);
    }
}

/// Mark module-level bindings and locals that are never referenced (E015).
pub fn check_unused_bindings(hir: &HirProgram, diagnostics: &mut DiagnosticBag) {
    let mut uses = Uses::default();
    for item in &hir.items {
        collect_uses_item(item, &mut uses);
    }
    let used = &uses.read;
    for &(id, span) in &uses.locals {
        let Some(name) = hir.names.get(&id) else {
            continue;
        };
        if used.contains(&id) || should_skip_unused_check(name) || name.starts_with("__") {
            continue;
        }
        diagnostics.push_warning(FluxWarning::UnusedLocal {
            name: name.clone(),
            span: span_to_miette(span),
            source_id: span.source,
        });
    }

    for item in &hir.items {
        let HirItem::Binding(binding) = item else {
            continue;
        };
        let Some(name) = hir.names.get(&binding.name) else {
            continue;
        };
        if should_skip_unused_check(name) {
            continue;
        }
        if used.contains(&binding.name) {
            continue;
        }
        diagnostics.push_warning(FluxWarning::UnusedBinding {
            name: name.clone(),
            span: span_to_miette(binding.span),
            source_id: binding.span.source,
        });
    }
}

fn should_skip_unused_check(name: &str) -> bool {
    name.starts_with('_')
        || name.starts_with("__mod_")
        || name.starts_with("__impl_")
        || name.starts_with("__closure_")
}

fn collect_uses_item(item: &HirItem, used: &mut Uses) {
    match item {
        HirItem::Binding(b) => collect_uses_expr(&b.value, used),
        HirItem::Fn(f) => collect_uses_block(&f.body, used),
        HirItem::Watch(w) => {
            if let Some(t) = w.target {
                used.insert(t);
            }
            if let Some(p) = w.prev {
                used.insert(p);
            }
            collect_uses_block(&w.body, used);
        }
        HirItem::On(o) => {
            used.insert(o.event);
            for &p in &o.params {
                used.insert(p);
            }
            collect_uses_block(&o.body, used);
        }
        HirItem::InitIf(i) => collect_uses_if(i, used),
        HirItem::SourceAssign(a) => {
            used.insert(a.name);
            for index in a.indices() {
                collect_uses_expr(index, used);
            }
            collect_uses_expr(&a.value, used);
        }
        HirItem::Struct(_)
        | HirItem::Enum(_)
        | HirItem::Trait(_)
        | HirItem::Impl(_)
        | HirItem::Event(_) => {}
    }
}

fn collect_uses_block(block: &HirBlock, used: &mut Uses) {
    for stmt in &block.stmts {
        collect_uses_stmt(stmt, used);
    }
}

fn collect_uses_stmt(stmt: &HirStmt, used: &mut Uses) {
    match stmt {
        HirStmt::Let(l) => {
            used.locals.push((l.name, l.span));
            collect_uses_expr(&l.value, used)
        }
        HirStmt::Assign(a) => {
            used.insert(a.name);
            for index in a.indices() {
                collect_uses_expr(index, used);
            }
            collect_uses_expr(&a.value, used);
        }
        HirStmt::Emit(e) => {
            used.insert(e.event);
            for arg in &e.args {
                collect_uses_expr(arg, used);
            }
        }
        HirStmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_uses_expr(v, used);
            }
        }
        HirStmt::Expr(e) => collect_uses_expr(e, used),
        HirStmt::If(i) => collect_uses_if(i, used),
        HirStmt::While(w) => {
            collect_uses_expr(&w.cond, used);
            collect_uses_block(&w.body, used);
        }
        HirStmt::For(f) => {
            collect_uses_expr(&f.iter, used);
            collect_uses_block(&f.body, used);
        }
        HirStmt::Watch(w) => {
            if let Some(t) = w.target {
                used.insert(t);
            }
            if let Some(p) = w.prev {
                used.insert(p);
            }
            collect_uses_block(&w.body, used);
        }
        HirStmt::On(o) => {
            used.insert(o.event);
            for &p in &o.params {
                used.insert(p);
            }
            collect_uses_block(&o.body, used);
        }
        HirStmt::Break(_) | HirStmt::Continue(_) => {}
    }
}

fn collect_uses_if(if_: &HirIf, used: &mut Uses) {
    collect_uses_expr(&if_.cond, used);
    collect_uses_block(&if_.then_branch, used);
    if let Some(el) = &if_.else_branch {
        match el {
            HirElse::If(inner) => collect_uses_if(inner, used),
            HirElse::Block(b) => collect_uses_block(b, used),
        }
    }
}

fn collect_uses_expr(expr: &HirExpr, used: &mut Uses) {
    match expr {
        HirExpr::Ident { symbol, .. } => {
            used.insert(*symbol);
        }
        HirExpr::Snap { expr, .. } => collect_uses_expr(expr, used),
        HirExpr::Call { callee, args, .. } => {
            collect_uses_expr(callee, used);
            for a in args {
                collect_uses_expr(a, used);
            }
        }
        HirExpr::Field { base, .. } => collect_uses_expr(base, used),
        HirExpr::Closure { fn_symbol, .. } => {
            used.insert(*fn_symbol);
        }
        HirExpr::Unary { expr, .. } => collect_uses_expr(expr, used),
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            collect_uses_expr(left, used);
            collect_uses_expr(right, used);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_uses_expr(cond, used);
            collect_uses_expr(then_expr, used);
            collect_uses_expr(else_expr, used);
        }
        HirExpr::Array { elems, .. } => {
            for e in elems {
                collect_uses_expr(e, used);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_uses_expr(v, used);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(p) = payload {
                collect_uses_expr(p, used);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_uses_expr(scrutinee, used);
            for arm in arms {
                for b in &arm.bindings {
                    used.insert(*b);
                }
                if let Some(g) = &arm.guard {
                    collect_uses_expr(g, used);
                }
                collect_uses_block(&arm.body, used);
                collect_uses_expr(&arm.expr, used);
            }
        }
        HirExpr::Cast { expr, .. } | HirExpr::ToText { expr, .. } | HirExpr::Try { expr, .. } => {
            collect_uses_expr(expr, used)
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use flux_diagnostics::{DiagnosticBag, SourceId};
    use flux_syntax::parse_program;

    use crate::lower_program;

    use super::check_unused_bindings;

    #[test]
    fn warns_on_unread_module_binding() {
        let src = "let used = 1\nlet dead = 2\nlet sum = used + 1\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        check_unused_bindings(&hir, &mut diags);
        assert!(diags.warnings.iter().any(|w| {
            matches!(
                w,
                flux_diagnostics::FluxWarning::UnusedBinding { name, .. } if name == "dead"
            )
        }));
    }

    #[test]
    fn skips_underscore_bindings() {
        let src = "let _scratch = 99\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        assert!(!diags.has_warnings());
        check_unused_bindings(&hir, &mut diags);
        assert!(!diags.has_warnings());
    }

    #[test]
    fn assignment_in_watch_counts_as_use() {
        let src = "let a = 1\nlet mut side = 0\nwatch { side = a * 2 }\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        check_unused_bindings(&hir, &mut diags);
        assert!(!diags
            .warnings
            .iter()
            .any(|w| matches!(w, flux_diagnostics::FluxWarning::UnusedBinding { name, .. } if name == "side")));
    }

    fn unused_locals(src: &str) -> Vec<String> {
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        lower_program(&ast, &mut diags).expect("lower");
        diags
            .warnings
            .iter()
            .filter_map(|w| match w {
                flux_diagnostics::FluxWarning::UnusedLocal { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// 0.5: E015 covers locals too, in functions, handlers, and nested blocks.
    #[test]
    fn warns_on_unread_locals() {
        let src = "event Tick

fn main() {
    let dead = 5
    let used = 1
    if used > 0 {
        let inner = 2
    }
}

on Tick {
    let also = 3
}
";
        let mut got = unused_locals(src);
        got.sort();
        assert_eq!(got, ["also", "dead", "inner"]);
    }

    #[test]
    fn read_or_underscore_locals_are_not_reported() {
        let src = "fn f(x: Int) -> Int {
    let _keep = 1
    let y = x + 1
    let ys = [y].map(|v| v * 2)
    ys[0]
}
";
        assert!(unused_locals(src).is_empty());
    }
}
