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
use flux_hir::{HirBlock, HirStmt, SymbolId};

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
                    .and_then(|t| resolve_hir_type(ctx, t, source, &mut sink))
                    .unwrap_or_else(|| {
                        infer_expr(ctx, &s.value, &scope, names, source, &mut sink)
                            .unwrap_or(Type::Error)
                    });
            scope.insert(s.name, ty);
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
