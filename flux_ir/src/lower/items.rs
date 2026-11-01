//! Lowering of functions, effects (`watch` / `on`), statements and blocks to IR.

use std::collections::HashMap;

use flux_hir::{HirBlock, HirElse, HirExpr, HirFn, HirIf, HirOn, HirProgram, HirStmt, HirWatch};

use crate::{CompiledFunction, CompiledOnHandler, CompiledWatch, Inst, IrStmt, IrValue};

use super::codegen::lower_expr;
use super::reads::collect_block_reads;
use super::runtime_symbol_name;

pub(super) fn lower_fn(f: &HirFn, name: &str, program: &HirProgram) -> CompiledFunction {
    CompiledFunction {
        name: name.to_string(),
        reactive: f.reactive,
        params: f
            .params
            .iter()
            .filter_map(|p| runtime_symbol_name(program, &p.name))
            .collect(),
        body: lower_block(&f.body, program),
    }
}

pub(super) fn lower_watch(
    w: &HirWatch,
    program: &HirProgram,
    reactive_fns: &HashMap<String, &HirFn>,
) -> CompiledWatch {
    let deps = collect_block_reads(&w.body, program, reactive_fns);
    CompiledWatch {
        target: w.target.and_then(|t| program.names.get(&t).cloned()),
        deps,
        body: lower_block(&w.body, program),
    }
}

pub(super) fn lower_on(o: &HirOn, program: &HirProgram) -> CompiledOnHandler {
    CompiledOnHandler {
        event: program
            .names
            .get(&o.event)
            .cloned()
            .unwrap_or_else(|| "?".into()),
        param: o.param.and_then(|p| runtime_symbol_name(program, &p)),
        body: lower_block(&o.body, program),
    }
}

pub(super) fn lower_block(block: &HirBlock, program: &HirProgram) -> Vec<IrStmt> {
    block
        .stmts
        .iter()
        .filter_map(|s| lower_stmt(s, program))
        .collect()
}

fn lower_stmt(stmt: &HirStmt, program: &HirProgram) -> Option<IrStmt> {
    match stmt {
        HirStmt::Assign(a) => {
            let target = runtime_symbol_name(program, &a.name)?;
            let mut expr = lower_expr(&a.value, program);
            take_for_push(&target, &mut expr);
            Some(IrStmt::Assign { target, expr })
        }
        HirStmt::Emit(e) => Some(IrStmt::Emit {
            event: program.names.get(&e.event)?.clone(),
            arg: e.args.first().map(|a| lower_expr(a, program)),
        }),
        HirStmt::Expr(e) => Some(IrStmt::Expr {
            code: lower_expr(e, program),
        }),
        HirStmt::Let(l) => Some(IrStmt::Let {
            name: runtime_symbol_name(program, &l.name)?,
            expr: lower_expr(&l.value, program),
        }),
        HirStmt::If(i) => Some(lower_if_stmt(i, program)),
        HirStmt::While(w) => Some(IrStmt::While {
            cond: lower_expr(&w.cond, program),
            body: lower_block(&w.body, program),
        }),
        HirStmt::For(f) => Some(IrStmt::For {
            var: runtime_symbol_name(program, &f.name)?,
            iter: lower_expr(&f.iter, program),
            body: lower_block(&f.body, program),
        }),
        HirStmt::Return(r) => Some(IrStmt::Return {
            expr: r.value.as_ref().map(|v| lower_expr(v, program)),
        }),
        HirStmt::Break(_) => Some(IrStmt::Break),
        HirStmt::Continue(_) => Some(IrStmt::Continue),
        _ => None,
    }
}

pub(super) fn lower_if_stmt(stmt: &HirIf, program: &HirProgram) -> IrStmt {
    let else_body = stmt.else_branch.as_ref().map(|e| match e {
        HirElse::If(inner) => vec![lower_if_stmt(inner, program)],
        HirElse::Block(b) => lower_block(b, program),
    });
    IrStmt::If {
        cond: lower_expr(&stmt.cond, program),
        then_body: lower_block(&stmt.then_branch, program),
        else_body,
    }
}

pub(super) fn const_expr(expr: &HirExpr) -> Option<IrValue> {
    match expr {
        HirExpr::Int { value, .. } => Some(IrValue::Int(*value)),
        HirExpr::Float { value, .. } => Some(IrValue::Float(*value)),
        HirExpr::Bool { value, .. } => Some(IrValue::Bool(*value)),
        HirExpr::String { value, .. } => Some(IrValue::String(value.clone())),
        _ => None,
    }
}

/// `x = push(x, v)` on a local lowers to `[Load x, <v>, Call push]`. When `<v>`
/// does not touch `x`, loading `x` by move leaves the array unshared, so the
/// VM's `push` appends in place instead of copying it (O(1) rather than O(n)).
fn take_for_push(target: &str, code: &mut [Inst]) {
    if !target.starts_with("\0flux_local_") {
        return;
    }
    let [Inst::Load(first), value @ .., Inst::Call { name, argc: 2 }] = code else {
        return;
    };
    if first != target || name != "push" || mentions_name(value, target) || can_leave(value) {
        return;
    }
    code[0] = Inst::Take(target.to_string());
}

/// Whether `code` may leave before the assignment through `break`/`continue`
/// in a match arm block; a moved-out local would then stay unbound in the
/// loop. (`return`, `?`, and runtime errors end the function: no harm.)
fn can_leave(code: &[Inst]) -> bool {
    code.iter().any(|inst| match inst {
        Inst::Block(_) => true,
        Inst::ShortAnd(inner) | Inst::ShortOr(inner) => can_leave(inner),
        Inst::LazyIf {
            then_branch,
            else_branch,
        } => can_leave(then_branch) || can_leave(else_branch),
        _ => false,
    })
}

fn mentions_name(code: &[Inst], target: &str) -> bool {
    code.iter().any(|inst| match inst {
        Inst::Load(n) | Inst::Take(n) | Inst::BindLocal(n) => n == target,
        Inst::ShortAnd(inner) | Inst::ShortOr(inner) => mentions_name(inner, target),
        Inst::LazyIf {
            then_branch,
            else_branch,
        } => mentions_name(then_branch, target) || mentions_name(else_branch, target),
        _ => false,
    })
}
