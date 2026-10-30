//! Binary operator type inference.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_syntax::BinaryOp;

use crate::types::Type;

use super::super::util::{numeric_compatible, unwrap_value_type, widen_numeric};

pub(super) fn infer_binary(
    op: BinaryOp,
    left: &Type,
    right: &Type,
    span: flux_diagnostics::Span,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let l = unwrap_value_type(left);
    let r = unwrap_value_type(right);
    match op {
        BinaryOp::Add => {
            if matches!(l, Type::String) && matches!(r, Type::String) {
                Some(Type::String)
            } else if numeric_compatible(&l, &r) {
                Some(widen_numeric(&l, &r))
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "numeric or String type".into(),
                    found: format!("{} and {}", l.display(), r.display()),
                    span: span_to_miette(span),
                    source_id: source,
                });
                Some(Type::Error)
            }
        }
        // Int op Int stays Int (`/` truncates); any float operand widens to Float.
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            if numeric_compatible(&l, &r) {
                Some(widen_numeric(&l, &r))
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "numeric type".into(),
                    found: format!("{} and {}", l.display(), r.display()),
                    span: span_to_miette(span),
                    source_id: source,
                });
                Some(Type::Error)
            }
        }
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            if numeric_compatible(&l, &r) {
                Some(Type::Bool)
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "compatible numeric types".into(),
                    found: format!("{} and {}", l.display(), r.display()),
                    span: span_to_miette(span),
                    source_id: source,
                });
                Some(Type::Bool)
            }
        }
        BinaryOp::And | BinaryOp::Or => Some(Type::Bool),
    }
}
