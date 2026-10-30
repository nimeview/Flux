//! Transitive side effects of user functions, used to keep derived values pure.

use std::collections::{HashMap, HashSet};

use flux_hir::{
    HirBlock, HirElse, HirExpr, HirIf, HirItem, HirProgram, HirStmt, SymbolId, SymbolKind,
};

pub(super) fn effectful_functions(
    program: &HirProgram,
    builtin_symbols: &HashSet<SymbolId>,
) -> HashSet<SymbolId> {
    let mut calls = HashMap::<SymbolId, HashSet<SymbolId>>::new();
    let mut effectful = HashSet::new();
    for item in &program.items {
        let HirItem::Fn(function) = item else {
            continue;
        };
        let mut dependencies = HashSet::new();
        let mut has_effect = false;
        scan_block(
            &function.body,
            program,
            builtin_symbols,
            &mut dependencies,
            &mut has_effect,
        );
        calls.insert(function.name, dependencies);
        if has_effect {
            effectful.insert(function.name);
        }
    }
    loop {
        let newly_effectful: Vec<_> = calls
            .iter()
            .filter(|(function, dependencies)| {
                !effectful.contains(function)
                    && dependencies.iter().any(|callee| effectful.contains(callee))
            })
            .map(|(function, _)| *function)
            .collect();
        if newly_effectful.is_empty() {
            return effectful;
        }
        effectful.extend(newly_effectful);
    }
}

fn scan_block(
    block: &HirBlock,
    program: &HirProgram,
    builtin_symbols: &HashSet<SymbolId>,
    calls: &mut HashSet<SymbolId>,
    has_effect: &mut bool,
) {
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let(binding) => {
                scan_expr(&binding.value, program, builtin_symbols, calls, has_effect)
            }
            HirStmt::Assign(assign) => {
                if program.symbols.get(&assign.name).is_some_and(|symbol| {
                    matches!(symbol.kind, SymbolKind::Source | SymbolKind::Derived)
                }) {
                    *has_effect = true;
                }
                scan_expr(&assign.value, program, builtin_symbols, calls, has_effect);
            }
            HirStmt::Emit(emit) => {
                *has_effect = true;
                for arg in &emit.args {
                    scan_expr(arg, program, builtin_symbols, calls, has_effect);
                }
            }
            HirStmt::Return(ret) => {
                if let Some(expr) = &ret.value {
                    scan_expr(expr, program, builtin_symbols, calls, has_effect);
                }
            }
            HirStmt::Expr(expr) => scan_expr(expr, program, builtin_symbols, calls, has_effect),
            HirStmt::If(branch) => scan_if(branch, program, builtin_symbols, calls, has_effect),
            HirStmt::While(loop_stmt) => {
                scan_expr(&loop_stmt.cond, program, builtin_symbols, calls, has_effect);
                scan_block(&loop_stmt.body, program, builtin_symbols, calls, has_effect);
            }
            HirStmt::For(loop_stmt) => {
                scan_expr(&loop_stmt.iter, program, builtin_symbols, calls, has_effect);
                scan_block(&loop_stmt.body, program, builtin_symbols, calls, has_effect);
            }
            HirStmt::Watch(_) | HirStmt::On(_) => *has_effect = true,
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn scan_if(
    branch: &HirIf,
    program: &HirProgram,
    builtin_symbols: &HashSet<SymbolId>,
    calls: &mut HashSet<SymbolId>,
    has_effect: &mut bool,
) {
    scan_expr(&branch.cond, program, builtin_symbols, calls, has_effect);
    scan_block(
        &branch.then_branch,
        program,
        builtin_symbols,
        calls,
        has_effect,
    );
    if let Some(other) = &branch.else_branch {
        match other {
            HirElse::If(nested) => scan_if(nested, program, builtin_symbols, calls, has_effect),
            HirElse::Block(block) => scan_block(block, program, builtin_symbols, calls, has_effect),
        }
    }
}

fn scan_expr(
    expr: &HirExpr,
    program: &HirProgram,
    builtin_symbols: &HashSet<SymbolId>,
    calls: &mut HashSet<SymbolId>,
    has_effect: &mut bool,
) {
    match expr {
        HirExpr::Call { callee, args, .. } => {
            if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                if builtin_symbols.contains(symbol)
                    && program
                        .names
                        .get(symbol)
                        .is_some_and(|name| flux_std::SIDE_EFFECT_BUILTINS.contains(&name.as_str()))
                {
                    *has_effect = true;
                } else {
                    calls.insert(*symbol);
                }
            } else if let HirExpr::Field { field, .. } = callee.as_ref() {
                for ((_, method), symbol) in &program.trait_impls {
                    if method == field {
                        calls.insert(*symbol);
                    }
                }
            }
            scan_expr(callee, program, builtin_symbols, calls, has_effect);
            for arg in args {
                scan_expr(arg, program, builtin_symbols, calls, has_effect);
            }
        }
        HirExpr::Closure { fn_symbol, .. } => {
            calls.insert(*fn_symbol);
        }
        HirExpr::Snap { expr, .. } | HirExpr::Unary { expr, .. } | HirExpr::Cast { expr, .. } => {
            scan_expr(expr, program, builtin_symbols, calls, has_effect)
        }
        HirExpr::Field { base, .. } => scan_expr(base, program, builtin_symbols, calls, has_effect),
        HirExpr::Binary { left, right, .. } => {
            scan_expr(left, program, builtin_symbols, calls, has_effect);
            scan_expr(right, program, builtin_symbols, calls, has_effect);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            scan_expr(cond, program, builtin_symbols, calls, has_effect);
            scan_expr(then_expr, program, builtin_symbols, calls, has_effect);
            scan_expr(else_expr, program, builtin_symbols, calls, has_effect);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                scan_expr(elem, program, builtin_symbols, calls, has_effect);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, value) in fields {
                scan_expr(value, program, builtin_symbols, calls, has_effect);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(value) = payload {
                scan_expr(value, program, builtin_symbols, calls, has_effect);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            scan_expr(scrutinee, program, builtin_symbols, calls, has_effect);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    scan_expr(guard, program, builtin_symbols, calls, has_effect);
                }
                scan_expr(&arm.expr, program, builtin_symbols, calls, has_effect);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::Ident { .. } => {}
    }
}
