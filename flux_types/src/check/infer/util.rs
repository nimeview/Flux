//! Shared helpers for expression inference.

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, SymbolId};

use crate::types::Type;

use super::super::state::TypeCheckContext;
use super::super::util::{types_compatible, unwrap_value_type};
use super::expr::infer_expr;

/// Infers the element type of an array argument, reporting a mismatch if the
/// argument isn't an array. Returns `Type::Error` when the type can't be determined.
pub(super) fn infer_array_elem(
    ctx: &TypeCheckContext,
    arg: Option<&HirExpr>,
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Type {
    let Some(arg) = arg else { return Type::Error };
    let Some(got) = infer_expr(ctx, arg, types, names, source, diagnostics) else {
        return Type::Error;
    };
    match unwrap_value_type(&got) {
        Type::Array(elem) => *elem,
        other => {
            diagnostics.push(FluxError::TypeMismatch {
                expected: "Array".into(),
                found: other.display(),
                span: span_to_miette(arg.span()),
                source_id: arg.span().source,
            });
            Type::Error
        }
    }
}

/// Validates a HOF callback argument against the parameter types expected at the
/// call site, allowing the callback to declare fewer params than expected.
pub(super) fn check_callback(
    ctx: &TypeCheckContext,
    cb: &HirExpr,
    expected_params: &[Type],
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let cb_ty = infer_expr(ctx, cb, types, names, source, diagnostics)?;
    match unwrap_value_type(&cb_ty) {
        Type::Func { params, ret } => {
            if params.len() > expected_params.len() {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: format!("at most {} parameter(s)", expected_params.len()),
                    found: format!("{} parameter(s)", params.len()),
                    span: span_to_miette(cb.span()),
                    source_id: cb.span().source,
                });
            }
            for (p, expected) in params.iter().zip(expected_params.iter()) {
                if !p.is_error() && !expected.is_error() && !types_compatible(expected, p) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected.display(),
                        found: p.display(),
                        span: span_to_miette(cb.span()),
                        source_id: cb.span().source,
                    });
                }
            }
            Some(*ret)
        }
        _ => None,
    }
}
