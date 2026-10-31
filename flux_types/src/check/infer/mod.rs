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
use super::state::TypeCheckContext;

pub(super) use expr::infer_expr;

pub(super) fn infer_block_return(
    ctx: &TypeCheckContext,
    block: &HirBlock,
    types: &HashMap<SymbolId, Type>,
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

    for stmt in block.stmts.iter().rev() {
        match stmt {
            HirStmt::Return(r) => {
                return r
                    .value
                    .as_ref()
                    .map(|v| infer_expr(ctx, v, &scope, names, source, diagnostics))
                    .unwrap_or(Some(Type::Unit));
            }
            HirStmt::Expr(expr) => {
                return infer_expr(ctx, expr, &scope, names, source, diagnostics);
            }
            _ => continue,
        }
    }
    Some(Type::Unit)
}

/// Value type of an `if` statement used as a block's tail. Both branches must
/// agree exactly (no numeric widening: the runtime inserts no conversion), and
/// an `if` without `else` is `Unit`.
fn infer_if_value(
    ctx: &TypeCheckContext,
    stmt: &HirIf,
    scope: &HashMap<SymbolId, Type>,
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
