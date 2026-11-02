//! Static reactive-dependency collection.
//!
//! Walks derived-binding / effect bodies to find which reactive symbols are read,
//! inlining the bodies of reactive functions and closures so that captured sources
//! become part of the caller's dependency set.

use std::collections::{HashMap, HashSet};

use flux_hir::{
    HirBlock, HirElse, HirExpr, HirFn, HirIf, HirProgram, HirStmt, SymbolId, SymbolKind,
};

thread_local! {
    /// Function bodies being inlined by the current read walk.
    static INLINING: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
}

pub(super) fn collect_reads_expr(
    expr: &HirExpr,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
) -> HashSet<SymbolId> {
    let mut set = HashSet::new();
    collect_expr_reads(expr, program, reactive_fns, &mut set);
    set
}

pub(super) fn collect_block_reads(
    block: &HirBlock,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
) -> Vec<String> {
    let mut ids = HashSet::new();
    for stmt in &block.stmts {
        collect_stmt_reads(stmt, program, reactive_fns, &mut ids);
    }
    ids.iter()
        .filter(|id| {
            matches!(
                program.symbols.get(id).map(|symbol| symbol.kind),
                Some(SymbolKind::Source | SymbolKind::Derived)
            )
        })
        .filter_map(|id| program.names.get(id).cloned())
        .collect()
}

fn collect_stmt_reads(
    stmt: &HirStmt,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
    set: &mut HashSet<SymbolId>,
) {
    match stmt {
        HirStmt::Assign(a) => collect_expr_reads(&a.value, program, reactive_fns, set),
        HirStmt::Emit(e) => {
            for a in &e.args {
                collect_expr_reads(a, program, reactive_fns, set);
            }
        }
        HirStmt::Expr(e) => collect_expr_reads(e, program, reactive_fns, set),
        HirStmt::Let(l) => collect_expr_reads(&l.value, program, reactive_fns, set),
        HirStmt::If(i) => collect_if_reads(i, program, reactive_fns, set),
        HirStmt::While(w) => {
            collect_expr_reads(&w.cond, program, reactive_fns, set);
            for s in &w.body.stmts {
                collect_stmt_reads(s, program, reactive_fns, set);
            }
        }
        HirStmt::For(f) => {
            collect_expr_reads(&f.iter, program, reactive_fns, set);
            for s in &f.body.stmts {
                collect_stmt_reads(s, program, reactive_fns, set);
            }
        }
        HirStmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_expr_reads(v, program, reactive_fns, set);
            }
        }
        HirStmt::Watch(w) => {
            for s in &w.body.stmts {
                collect_stmt_reads(s, program, reactive_fns, set);
            }
        }
        HirStmt::On(o) => {
            for s in &o.body.stmts {
                collect_stmt_reads(s, program, reactive_fns, set);
            }
        }
        HirStmt::Break(_) | HirStmt::Continue(_) => {}
    }
}

fn collect_if_reads(
    stmt: &HirIf,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
    set: &mut HashSet<SymbolId>,
) {
    collect_expr_reads(&stmt.cond, program, reactive_fns, set);
    for s in &stmt.then_branch.stmts {
        collect_stmt_reads(s, program, reactive_fns, set);
    }
    if let Some(else_branch) = &stmt.else_branch {
        match else_branch {
            HirElse::If(inner) => collect_if_reads(inner, program, reactive_fns, set),
            HirElse::Block(b) => {
                for s in &b.stmts {
                    collect_stmt_reads(s, program, reactive_fns, set);
                }
            }
        }
    }
}

fn collect_expr_reads(
    expr: &HirExpr,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
    set: &mut HashSet<SymbolId>,
) {
    match expr {
        HirExpr::Ident { symbol, .. } => {
            set.insert(*symbol);
            // An `rfn` used as a value may run anywhere its value goes.
            collect_rfn_reads(symbol, program, reactive_fns, set);
        }
        HirExpr::Snap { expr, .. } => collect_expr_reads(expr, program, reactive_fns, set),
        HirExpr::Call { callee, args, .. } => {
            for a in args {
                collect_expr_reads(a, program, reactive_fns, set);
            }
            match callee.as_ref() {
                HirExpr::Field { base, .. } => {
                    collect_expr_reads(base, program, reactive_fns, set);
                }
                // A function value in a binding is a read of that binding,
                // which depends on what its closure reads.
                callee => collect_expr_reads(callee, program, reactive_fns, set),
            }
        }
        // Closures passed as HOF callbacks (e.g. `filter(items, |x| x > threshold)`)
        // are reactive functions too — inline their captured reads the same way as a
        // direct `rfn` call, so the caller's dependency set includes captured sources.
        HirExpr::Closure { fn_symbol, .. } => {
            if let Some(name) = program.names.get(fn_symbol) {
                if let Some(closure_fn) = reactive_fns.get(name) {
                    for stmt in &closure_fn.body.stmts {
                        collect_stmt_reads(stmt, program, reactive_fns, set);
                    }
                }
            }
        }
        HirExpr::Unary { expr, .. } => collect_expr_reads(expr, program, reactive_fns, set),
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            collect_expr_reads(left, program, reactive_fns, set);
            collect_expr_reads(right, program, reactive_fns, set);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_expr_reads(cond, program, reactive_fns, set);
            collect_expr_reads(then_expr, program, reactive_fns, set);
            collect_expr_reads(else_expr, program, reactive_fns, set);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                collect_expr_reads(elem, program, reactive_fns, set);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, e) in fields {
                collect_expr_reads(e, program, reactive_fns, set);
            }
        }
        HirExpr::EnumVariant {
            payload: Some(value),
            ..
        } => collect_expr_reads(value, program, reactive_fns, set),
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_expr_reads(scrutinee, program, reactive_fns, set);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_expr_reads(guard, program, reactive_fns, set);
                }
                for stmt in &arm.body.stmts {
                    collect_stmt_reads(stmt, program, reactive_fns, set);
                }
                collect_expr_reads(&arm.expr, program, reactive_fns, set);
            }
        }
        HirExpr::Field { base, .. } => {
            collect_expr_reads(base, program, reactive_fns, set);
        }
        _ => {}
    }
}

fn collect_rfn_reads(
    symbol: &SymbolId,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
    set: &mut HashSet<SymbolId>,
) {
    let Some(name) = program.names.get(symbol) else {
        return;
    };
    let Some(rfn) = reactive_fns.get(name) else {
        return;
    };
    // Recursive functions: inline each body once per walk.
    if INLINING.with(|s| s.borrow_mut().insert(name.clone())) {
        for stmt in &rfn.body.stmts {
            collect_stmt_reads(stmt, program, reactive_fns, set);
        }
        INLINING.with(|s| s.borrow_mut().remove(name));
    }
}
