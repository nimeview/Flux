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
            HirItem::Fn(f) => block(&mut f.body, calls, Some(f.name)),
            HirItem::Binding(b) => expr(&mut b.value, calls, None),
            HirItem::SourceAssign(a) => expr(&mut a.value, calls, None),
            HirItem::InitIf(i) => if_stmt(i, calls, None),
            HirItem::Watch(w) => block(&mut w.body, calls, None),
            HirItem::On(o) => block(&mut o.body, calls, None),
            HirItem::Struct(_)
            | HirItem::Enum(_)
            | HirItem::Trait(_)
            | HirItem::Impl(_)
            | HirItem::Event(_) => {}
        }
    }
}

type Calls = HashMap<(Option<SymbolId>, Span), SymbolId>;

fn block(b: &mut HirBlock, calls: &Calls, owner: Option<SymbolId>) {
    for stmt in &mut b.stmts {
        match stmt {
            HirStmt::Let(binding) => expr(&mut binding.value, calls, owner),
            HirStmt::Assign(assign) => expr(&mut assign.value, calls, owner),
            HirStmt::Emit(emit) => emit.args.iter_mut().for_each(|a| expr(a, calls, owner)),
            HirStmt::Expr(e) => expr(e, calls, owner),
            HirStmt::Return(ret) => {
                if let Some(e) = &mut ret.value {
                    expr(e, calls, owner);
                }
            }
            HirStmt::If(i) => if_stmt(i, calls, owner),
            HirStmt::While(w) => {
                expr(&mut w.cond, calls, owner);
                block(&mut w.body, calls, owner);
            }
            HirStmt::For(f) => {
                expr(&mut f.iter, calls, owner);
                block(&mut f.body, calls, owner);
            }
            HirStmt::Watch(w) => block(&mut w.body, calls, owner),
            HirStmt::On(o) => block(&mut o.body, calls, owner),
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn if_stmt(i: &mut HirIf, calls: &Calls, owner: Option<SymbolId>) {
    expr(&mut i.cond, calls, owner);
    block(&mut i.then_branch, calls, owner);
    match &mut i.else_branch {
        Some(HirElse::If(nested)) => if_stmt(nested, calls, owner),
        Some(HirElse::Block(b)) => block(b, calls, owner),
        None => {}
    }
}

fn expr(e: &mut HirExpr, calls: &Calls, owner: Option<SymbolId>) {
    match e {
        HirExpr::Call { callee, args, span } => {
            expr(callee, calls, owner);
            args.iter_mut().for_each(|a| expr(a, calls, owner));
            let Some(&impl_fn) = calls
                .get(&(owner, *span))
                .or_else(|| calls.get(&(None, *span)))
            else {
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
        | HirExpr::Try { expr: inner, .. } => expr(inner, calls, owner),
        HirExpr::Field { base, .. } => expr(base, calls, owner),
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            expr(left, calls, owner);
            expr(right, calls, owner);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            expr(cond, calls, owner);
            expr(then_expr, calls, owner);
            expr(else_expr, calls, owner);
        }
        HirExpr::Array { elems, .. } => elems.iter_mut().for_each(|x| expr(x, calls, owner)),
        HirExpr::StructLit { fields, .. } => {
            fields.iter_mut().for_each(|(_, x)| expr(x, calls, owner))
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            expr(scrutinee, calls, owner);
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    expr(guard, calls, owner);
                }
                block(&mut arm.body, calls, owner);
                expr(&mut arm.expr, calls, owner);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(p) = payload {
                expr(p, calls, owner);
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
