//! Lowering of checked method calls to direct calls.
//!
//! The checker resolves `recv.method(args)` from the receiver's type and
//! records the impl it chose; this pass rewrites each such call to
//! `impl_fn(recv, args)`. Code generation then calls exactly that function,
//! instead of guessing an impl from the method name alone.

use std::collections::HashMap;

use flux_diagnostics::Span;
use flux_hir::{HirBlock, HirElse, HirExpr, HirIf, HirItem, HirProgram, HirStmt, SymbolId};

use super::TypedProgram;

pub fn resolve_method_calls(program: &mut HirProgram, typed: &TypedProgram) {
    if typed.method_calls.is_empty() {
        return;
    }
    let calls = &typed.method_calls;
    for item in &mut program.items {
        match item {
            HirItem::Fn(f) => block(&mut f.body, calls),
            HirItem::Binding(b) => expr(&mut b.value, calls),
            HirItem::SourceAssign(a) => expr(&mut a.value, calls),
            HirItem::InitIf(i) => if_stmt(i, calls),
            HirItem::Watch(w) => block(&mut w.body, calls),
            HirItem::On(o) => block(&mut o.body, calls),
            HirItem::Struct(_)
            | HirItem::Enum(_)
            | HirItem::Trait(_)
            | HirItem::Impl(_)
            | HirItem::Event(_) => {}
        }
    }
}

type Calls = HashMap<Span, SymbolId>;

fn block(b: &mut HirBlock, calls: &Calls) {
    for stmt in &mut b.stmts {
        match stmt {
            HirStmt::Let(binding) => expr(&mut binding.value, calls),
            HirStmt::Assign(assign) => expr(&mut assign.value, calls),
            HirStmt::Emit(emit) => emit.args.iter_mut().for_each(|a| expr(a, calls)),
            HirStmt::Expr(e) => expr(e, calls),
            HirStmt::Return(ret) => {
                if let Some(e) = &mut ret.value {
                    expr(e, calls);
                }
            }
            HirStmt::If(i) => if_stmt(i, calls),
            HirStmt::While(w) => {
                expr(&mut w.cond, calls);
                block(&mut w.body, calls);
            }
            HirStmt::For(f) => {
                expr(&mut f.iter, calls);
                block(&mut f.body, calls);
            }
            HirStmt::Watch(w) => block(&mut w.body, calls),
            HirStmt::On(o) => block(&mut o.body, calls),
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn if_stmt(i: &mut HirIf, calls: &Calls) {
    expr(&mut i.cond, calls);
    block(&mut i.then_branch, calls);
    match &mut i.else_branch {
        Some(HirElse::If(nested)) => if_stmt(nested, calls),
        Some(HirElse::Block(b)) => block(b, calls),
        None => {}
    }
}

fn expr(e: &mut HirExpr, calls: &Calls) {
    match e {
        HirExpr::Call { callee, args, span } => {
            expr(callee, calls);
            args.iter_mut().for_each(|a| expr(a, calls));
            let Some(&impl_fn) = calls.get(span) else {
                return;
            };
            if !matches!(callee.as_ref(), HirExpr::Field { .. }) {
                return;
            }
            let direct = HirExpr::Ident {
                symbol: impl_fn,
                span: *span,
            };
            if let HirExpr::Field { base, .. } = std::mem::replace(callee.as_mut(), direct) {
                args.insert(0, *base);
            }
        }
        HirExpr::Snap { expr: inner, .. }
        | HirExpr::Unary { expr: inner, .. }
        | HirExpr::Cast { expr: inner, .. }
        | HirExpr::ToText { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => expr(inner, calls),
        HirExpr::Field { base, .. } => expr(base, calls),
        HirExpr::Binary { left, right, .. } => {
            expr(left, calls);
            expr(right, calls);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            expr(cond, calls);
            expr(then_expr, calls);
            expr(else_expr, calls);
        }
        HirExpr::Array { elems, .. } => elems.iter_mut().for_each(|x| expr(x, calls)),
        HirExpr::StructLit { fields, .. } => fields.iter_mut().for_each(|(_, x)| expr(x, calls)),
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            expr(scrutinee, calls);
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    expr(guard, calls);
                }
                block(&mut arm.body, calls);
                expr(&mut arm.expr, calls);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(p) = payload {
                expr(p, calls);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::Ident { .. }
        | HirExpr::Closure { .. } => {}
    }
}
