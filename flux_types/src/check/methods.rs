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
    program
        .changing_calls
        .extend(typed.changing_sites.iter().copied());
    program
        .member_reads
        .extend(typed.member_reads.iter().map(|(s, id)| (*s, *id)));
    program
        .member_calls
        .extend(typed.member_calls.iter().copied());
    program
        .member_events
        .extend(typed.member_events.iter().map(|(s, id)| (*s, *id)));
    if typed.method_calls.is_empty()
        && typed.field_calls.is_empty()
        && typed.bit_not_sites.is_empty()
        && typed.numeric_widenings.is_empty()
    {
        return;
    }
    let calls = &Calls {
        methods: &typed.method_calls,
        fields: &typed.field_calls,
        bit_nots: &typed.bit_not_sites,
        widenings: &typed.numeric_widenings,
        grouped: program.builtin_ids.get(flux_hir::GROUPED).copied(),
    };
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

/// What the checker resolved: method calls, and calls of a function in a field.
struct Calls<'a> {
    methods: &'a HashMap<(Option<SymbolId>, Span), SymbolId>,
    fields: &'a std::collections::HashSet<(Option<SymbolId>, Span)>,
    bit_nots: &'a std::collections::HashSet<Span>,
    widenings: &'a HashMap<Span, crate::Type>,
    grouped: Option<SymbolId>,
}

/// `e` as `to` when the checker found it narrower than the value it joins
/// (`if c { 3 } else { 2.5 }`): wrapped in a conversion.
fn widen(e: &mut HirExpr, calls: &Calls<'_>) {
    let span = e.span();
    if let Some(to) = calls.widenings.get(&span) {
        let inner = std::mem::replace(e, HirExpr::Bool { value: false, span });
        *e = HirExpr::Cast {
            expr: Box::new(inner),
            ty: to.key(),
            span,
        };
    }
}

impl Calls<'_> {
    fn get(&self, key: &(Option<SymbolId>, Span)) -> Option<&SymbolId> {
        self.methods.get(key)
    }
}

fn block(b: &mut HirBlock, calls: &Calls<'_>, owner: Option<SymbolId>) {
    for stmt in &mut b.stmts {
        match stmt {
            HirStmt::Let(binding) => expr(&mut binding.value, calls, owner),
            HirStmt::Assign(assign) => {
                for index in assign.indices_mut() {
                    expr(index, calls, owner);
                }
                expr(&mut assign.value, calls, owner);
            }
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

fn if_stmt(i: &mut HirIf, calls: &Calls<'_>, owner: Option<SymbolId>) {
    expr(&mut i.cond, calls, owner);
    block(&mut i.then_branch, calls, owner);
    match &mut i.else_branch {
        Some(HirElse::If(nested)) => if_stmt(nested, calls, owner),
        Some(HirElse::Block(b)) => block(b, calls, owner),
        None => {}
    }
}

fn expr(e: &mut HirExpr, calls: &Calls<'_>, owner: Option<SymbolId>) {
    match e {
        HirExpr::Call { callee, args, span } => {
            expr(callee, calls, owner);
            args.iter_mut().for_each(|a| expr(a, calls, owner));
            // `op.f(x)`: the function in field `f`, as `(op.f)(x)`.
            let field_call =
                calls.fields.contains(&(owner, *span)) || calls.fields.contains(&(None, *span));
            if let (true, Some(grouped)) = (field_call, calls.grouped) {
                if matches!(callee.as_ref(), HirExpr::Field { .. }) {
                    let span = *span;
                    let field =
                        std::mem::replace(callee.as_mut(), HirExpr::Bool { value: false, span });
                    **callee = HirExpr::Call {
                        callee: Box::new(HirExpr::Ident {
                            symbol: grouped,
                            span,
                        }),
                        args: vec![field],
                        span,
                    };
                }
                return;
            }
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
        HirExpr::Unary {
            op,
            expr: inner,
            span,
        } => {
            if *op == flux_syntax::UnaryOp::Not && calls.bit_nots.contains(span) {
                *op = flux_syntax::UnaryOp::BitNot;
            }
            expr(inner, calls, owner)
        }
        HirExpr::Snap { expr: inner, .. }
        | HirExpr::Cast { expr: inner, .. }
        | HirExpr::ToText { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => expr(inner, calls, owner),
        HirExpr::Field { base, .. } => expr(base, calls, owner),
        // `a + b` on arrays: the library's `concat(a, b)`.
        HirExpr::Binary {
            left, right, span, ..
        } => {
            expr(left, calls, owner);
            expr(right, calls, owner);
            let found = calls
                .get(&(owner, *span))
                .or_else(|| calls.get(&(None, *span)))
                .copied();
            if let Some(concat) = found {
                let span = *span;
                let (l, r) = (
                    std::mem::replace(left.as_mut(), HirExpr::Bool { value: false, span }),
                    std::mem::replace(right.as_mut(), HirExpr::Bool { value: false, span }),
                );
                *e = HirExpr::Call {
                    callee: Box::new(HirExpr::Ident {
                        symbol: concat,
                        span,
                    }),
                    args: vec![l, r],
                    span,
                };
            }
        }
        HirExpr::Index {
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
            widen(then_expr, calls);
            widen(else_expr, calls);
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
                widen(&mut arm.expr, calls);
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
