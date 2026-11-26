//! Expression and block type inference.
//!
//! Split into:
//! - [`expr`]     — `infer_expr` and match-arm binding
//! - [`builtins`] — stdlib builtin call typing
//! - [`ops`]      — binary operator typing
//! - [`util`]     — array/callback helpers shared by builtins

mod builtins;
mod expr;
mod ops;
mod util;

use std::collections::HashMap;

use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::{HirBlock, HirElse, HirIf, HirStmt, SymbolId};

use crate::types::Type;

use super::resolve::resolve_hir_type;
use super::state::{Scope, TypeCheckContext};

pub(super) use expr::{infer_expr, infer_expr_expected, usable_context};
pub(crate) use ops::infer_binary;

pub(super) fn infer_block_return(
    ctx: &TypeCheckContext,
    block: &HirBlock,
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let mut scope = types.clone();
    let mut sink = DiagnosticBag::new();
    for stmt in &block.stmts {
        if let HirStmt::Let(s) = stmt {
            let ty =
                s.ty.as_ref()
                    .and_then(|t| resolve_hir_type(ctx, t, &mut sink))
                    .unwrap_or_else(|| {
                        infer_expr(ctx, &s.value, &scope, names, source, &mut sink)
                            .unwrap_or(Type::Error)
                    });
            scope.insert(s.name, ty);
        }
    }

    // A trailing `if … else` is the block's value: the executed branch's tail.
    if let Some(HirStmt::If(tail)) = block.stmts.last() {
        if tail.else_branch.is_some() {
            return infer_if_value(ctx, tail, &scope, names, source, diagnostics);
        }
    }

    // A trailing `match` whose every arm leaves (`return`) has no value of
    // its own: the function's result is what the arms return, as for an
    // `if … else` whose branches all return.
    if let Some(HirStmt::Expr(flux_hir::HirExpr::Match { arms, .. })) = block.stmts.last() {
        if !arms.is_empty() && arms.iter().all(|arm| arm.diverges) {
            let returned = arms.iter().find_map(|arm| match arm.body.stmts.last() {
                Some(HirStmt::Return(r)) => Some(
                    r.value
                        .as_ref()
                        .and_then(|v| infer_expr(ctx, v, &scope, names, source, &mut sink))
                        .unwrap_or(Type::Unit),
                ),
                _ => None,
            });
            return Some(returned.unwrap_or(Type::Error));
        }
    }

    // Only the last statement gives a value, as at run time.
    match block.stmts.last() {
        Some(HirStmt::Return(r)) => r
            .value
            .as_ref()
            .map(|v| infer_expr(ctx, v, &scope, names, source, diagnostics))
            .unwrap_or(Some(Type::Unit)),
        Some(HirStmt::Expr(expr)) => infer_expr(ctx, expr, &scope, names, source, diagnostics),
        // `x.push(1)` written last: if the method turns out not to change `x`
        // (a type's own `push` that returns a value), it is the block's value.
        Some(HirStmt::Assign(assign)) if ctx.mut_call_sites.contains(&assign.span) => {
            let ty = infer_expr(ctx, &assign.value, &scope, names, source, diagnostics);
            let resolved = ctx
                .method_calls
                .borrow()
                .get(&(ctx.current_function.get(), assign.span))
                .copied();
            let changes = resolved.is_some_and(|id| ctx.mut_methods.contains(&id))
                || ctx.changing_sites.borrow().contains(&assign.span);
            if changes {
                Some(Type::Unit)
            } else {
                ty
            }
        }
        _ => Some(Type::Unit),
    }
}

/// Value type of an `if` statement used as a block's tail. Both branches must
/// agree exactly (no numeric widening: the runtime inserts no conversion), and
/// an `if` without `else` is `Unit`.
fn infer_if_value(
    ctx: &TypeCheckContext,
    stmt: &HirIf,
    scope: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let then_ty = infer_block_return(ctx, &stmt.then_branch, scope, names, source, diagnostics)?;
    let else_ty = match &stmt.else_branch {
        None => return Some(Type::Unit),
        Some(HirElse::Block(b)) => infer_block_return(ctx, b, scope, names, source, diagnostics)?,
        Some(HirElse::If(inner)) => {
            if inner.else_branch.is_none() {
                Type::Unit
            } else {
                infer_if_value(ctx, inner, scope, names, source, diagnostics)?
            }
        }
    };
    let (t, e) = (
        super::util::unwrap_value_type(&then_ty),
        super::util::unwrap_value_type(&else_ty),
    );
    if t.is_error() || e.is_error() {
        return Some(t);
    }
    if super::util::types_compatible(&t, &e) {
        return Some(super::util::refine_unknown(&t, &e));
    }
    diagnostics.push(flux_diagnostics::FluxError::TypeMismatch {
        expected: t.display(),
        found: e.display(),
        span: flux_diagnostics::span_to_miette(stmt.span),
        source_id: stmt.span.source,
    });
    Some(Type::Error)
}
