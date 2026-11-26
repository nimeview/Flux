//! Lowering of functions, effects (`watch` / `on`), statements and blocks to IR.

use std::collections::HashMap;

use flux_diagnostics::Span;
use flux_hir::{HirBlock, HirElse, HirExpr, HirFn, HirIf, HirOn, HirProgram, HirStmt, HirWatch};

use crate::{
    CompiledFunction, CompiledOnHandler, CompiledWatch, Inst, IrStmt, IrValue, SourceLoc, StmtBlock,
};

use super::codegen::lower_expr;
use super::reads::collect_block_reads;
use super::runtime_symbol_name;

pub(super) fn lower_fn(f: &HirFn, name: &str, program: &HirProgram) -> CompiledFunction {
    CompiledFunction {
        name: name.to_string(),
        reactive: f.reactive,
        act: f.act,
        params: f
            .params
            .iter()
            .filter_map(|p| runtime_symbol_name(program, &p.name))
            .chain(
                f.captures
                    .iter()
                    .filter_map(|id| runtime_symbol_name(program, id)),
            )
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
        prev: w.prev.and_then(|p| runtime_symbol_name(program, &p)),
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
        params: o
            .params
            .iter()
            .filter_map(|p| runtime_symbol_name(program, p))
            .collect(),
        body: lower_block(&o.body, program),
    }
}

pub(super) fn lower_block(block: &HirBlock, program: &HirProgram) -> StmtBlock {
    let mut out = StmtBlock::default();
    for stmt in &block.stmts {
        if let Some(ir) = lower_stmt(stmt, program) {
            out.stmts.push(ir);
            out.locs.push(source_loc(stmt_span(stmt)));
        }
    }
    out
}

pub(super) fn source_loc(span: Span) -> SourceLoc {
    SourceLoc {
        source: span.source.0,
        offset: span.start as u32,
    }
}

fn stmt_span(stmt: &HirStmt) -> Span {
    match stmt {
        HirStmt::Let(s) => s.span,
        HirStmt::Assign(s) => s.span,
        HirStmt::Emit(s) => s.span,
        HirStmt::Return(s) => s.span,
        HirStmt::Break(span) | HirStmt::Continue(span) => *span,
        HirStmt::Expr(e) => e.span(),
        HirStmt::If(s) => s.span,
        HirStmt::While(s) => s.span,
        HirStmt::For(s) => s.span,
        HirStmt::Watch(s) => s.span,
        HirStmt::On(s) => s.span,
    }
}

fn lower_stmt(stmt: &HirStmt, program: &HirProgram) -> Option<IrStmt> {
    match stmt {
        // `x.m(…)` on a place where `m` does not change its receiver: the call.
        HirStmt::Assign(a)
            if program.mut_call_sites.contains(&a.span)
                && !program.changing_calls.contains(&a.span)
                && !calls_mut_method(&a.value, program) =>
        {
            Some(IrStmt::Expr {
                code: lower_expr(&a.value, program),
            })
        }
        HirStmt::Assign(a) if !a.path.is_empty() => Some(IrStmt::AssignPlace {
            target: runtime_symbol_name(program, &a.name)?,
            slot: None,
            path: a
                .path
                .iter()
                .map(|step| match step {
                    flux_hir::HirPlaceStep::Index(index) => {
                        crate::PlaceStep::Index(lower_expr(index, program))
                    }
                    flux_hir::HirPlaceStep::Field { name, .. } => {
                        crate::PlaceStep::Field(name.clone())
                    }
                })
                .collect(),
            op: a.op.map(|op| match op {
                flux_syntax::BinaryOp::Sub => crate::PlaceOp::Sub,
                flux_syntax::BinaryOp::Mul => crate::PlaceOp::Mul,
                flux_syntax::BinaryOp::Div => crate::PlaceOp::Div,
                flux_syntax::BinaryOp::Rem => crate::PlaceOp::Rem,
                _ => crate::PlaceOp::Add,
            }),
            expr: lower_expr(&a.value, program),
        }),
        // `xs = set(xs, i, v)` (also how `xs[i] = v` is checked) updates the
        // element in place even when `v` reads `xs`; `set` would copy then.
        HirStmt::Assign(a) if set_of_self(a, program).is_some() => {
            let [_, index, value] = set_of_self(a, program)?;
            Some(IrStmt::AssignPlace {
                target: runtime_symbol_name(program, &a.name)?,
                slot: None,
                path: vec![crate::PlaceStep::Index(lower_expr(index, program))],
                op: None,
                expr: lower_expr(value, program),
            })
        }
        HirStmt::Assign(a) => {
            let target = runtime_symbol_name(program, &a.name)?;
            let mut expr = lower_expr(&a.value, program);
            take_for_push(&target, &mut expr);
            Some(IrStmt::Assign {
                target,
                slot: None,
                expr,
            })
        }
        HirStmt::Emit(e) if e.member.is_some() => Some(IrStmt::EmitMember {
            event: program
                .names
                .get(program.member_events.get(&e.span)?)?
                .clone(),
            args: e.args.iter().map(|a| lower_expr(a, program)).collect(),
        }),
        HirStmt::Emit(e) => Some(IrStmt::Emit {
            event: program.names.get(&e.event)?.clone(),
            args: e.args.iter().map(|a| lower_expr(a, program)).collect(),
        }),
        HirStmt::Expr(e) => Some(IrStmt::Expr {
            code: lower_expr(e, program),
        }),
        HirStmt::Let(l) => Some(IrStmt::Let {
            name: runtime_symbol_name(program, &l.name)?,
            slot: 0,
            expr: lower_expr(&l.value, program),
        }),
        HirStmt::If(i) => Some(lower_if_stmt(i, program)),
        HirStmt::While(w) => Some(IrStmt::While {
            cond: lower_expr(&w.cond, program),
            body: lower_block(&w.body, program),
        }),
        HirStmt::For(f) => Some(IrStmt::For {
            var: runtime_symbol_name(program, &f.name)?,
            slot: 0,
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
        HirElse::If(inner) => StmtBlock {
            stmts: vec![lower_if_stmt(inner, program)],
            locs: vec![source_loc(inner.span)],
        },
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

/// `x = push(x, v)` on a local lowers to `[Load x, <v>, Call push]` (and
/// `x = set(x, i, v)` likewise). When the other arguments do not touch `x`,
/// loading `x` by move leaves the array unshared, so the VM updates it in
/// place instead of copying it (O(1) rather than O(n)).
/// Whether `call` (a resolved method call) calls a `mut self` method.
fn calls_mut_method(call: &HirExpr, program: &HirProgram) -> bool {
    match call {
        HirExpr::Call { callee, .. } => match callee.as_ref() {
            HirExpr::Ident { symbol, .. } => program.mut_methods.contains(symbol),
            _ => false,
        },
        _ => false,
    }
}

/// The arguments of `x = set(x, i, v)`: the builtin `set` applied to the
/// assigned binding itself.
fn set_of_self<'a>(a: &'a flux_hir::HirAssign, program: &HirProgram) -> Option<&'a [HirExpr; 3]> {
    if !a.path.is_empty() {
        return None;
    }
    let HirExpr::Call { callee, args, .. } = &a.value else {
        return None;
    };
    let HirExpr::Ident { symbol, .. } = callee.as_ref() else {
        return None;
    };
    let builtin = program
        .symbols
        .get(symbol)
        .is_some_and(|s| s.kind == flux_hir::SymbolKind::Builtin && s.name == "set");
    let args: &[HirExpr; 3] = args.as_slice().try_into().ok()?;
    // `set` reads `x` before its other arguments; only a match-arm block in
    // them could assign `x` in between, so those keep the call.
    if has_arm_block(&args[1]) || has_arm_block(&args[2]) {
        return None;
    }
    match &args[0] {
        HirExpr::Ident { symbol, .. } if builtin && *symbol == a.name => Some(args),
        _ => None,
    }
}

/// Whether `e` contains a `match` arm with statements.
fn has_arm_block(e: &HirExpr) -> bool {
    match e {
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            has_arm_block(scrutinee)
                || arms.iter().any(|arm| {
                    !arm.body.stmts.is_empty()
                        || arm.guard.as_ref().is_some_and(has_arm_block)
                        || has_arm_block(&arm.expr)
                })
        }
        HirExpr::Snap { expr, .. }
        | HirExpr::Field { base: expr, .. }
        | HirExpr::Unary { expr, .. }
        | HirExpr::Cast { expr, .. }
        | HirExpr::ToText { expr, .. }
        | HirExpr::Try { expr, .. } => has_arm_block(expr),
        HirExpr::Call { callee, args, .. } => {
            has_arm_block(callee) || args.iter().any(has_arm_block)
        }
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => has_arm_block(left) || has_arm_block(right),
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => has_arm_block(cond) || has_arm_block(then_expr) || has_arm_block(else_expr),
        HirExpr::Array { elems, .. } => elems.iter().any(has_arm_block),
        HirExpr::StructLit { fields, .. } => fields.iter().any(|(_, v)| has_arm_block(v)),
        HirExpr::EnumVariant { payload, .. } => payload.as_deref().is_some_and(has_arm_block),
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::Ident { .. }
        | HirExpr::Closure { .. } => false,
    }
}

fn take_for_push(target: &str, code: &mut [Inst]) {
    if !target.starts_with("\0flux_local_") {
        return;
    }
    let [Inst::Load(first), value @ .., Inst::Call { name, argc }] = code else {
        return;
    };
    let in_place = (name == "push" && *argc == 2) || (name == "set" && *argc == 3);
    if first != target || !in_place || mentions_name(value, target) || can_leave(value) {
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
        Inst::Load(n) | Inst::Take(n) | Inst::BindLocal { name: n, .. } => n == target,
        Inst::ShortAnd(inner) | Inst::ShortOr(inner) => mentions_name(inner, target),
        Inst::LazyIf {
            then_branch,
            else_branch,
        } => mentions_name(then_branch, target) || mentions_name(else_branch, target),
        _ => false,
    })
}
