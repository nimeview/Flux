//! Binary operator type inference.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_syntax::BinaryOp;

use crate::types::Type;

use super::super::util::{numeric_compatible, unwrap_value_type, widen_numeric};

fn equality_supported(ty: &Type) -> bool {
    match ty {
        Type::Bool | Type::Char | Type::String | Type::Unit => true,
        Type::Enum { variants, .. } => variants.iter().all(|(_, payload)| payload.is_none()),
        _ => false,
    }
}

pub(super) fn infer_binary(
    op: BinaryOp,
    left: &Type,
    right: &Type,
    span: flux_diagnostics::Span,
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
                    source_id: span.source,
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
                    source_id: span.source,
                });
                Some(Type::Error)
            }
        }
        BinaryOp::Eq | BinaryOp::Ne => {
            let non_numeric_comparable = l == r && equality_supported(&l);
            if numeric_compatible(&l, &r) || non_numeric_comparable {
                Some(Type::Bool)
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "compatible equality types".into(),
                    found: format!("{} and {}", l.display(), r.display()),
                    span: span_to_miette(span),
                    source_id: span.source,
                });
                Some(Type::Error)
            }
        }
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            if numeric_compatible(&l, &r) {
                Some(Type::Bool)
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "numeric types for ordered comparison".into(),
                    found: format!("{} and {}", l.display(), r.display()),
                    span: span_to_miette(span),
                    source_id: span.source,
                });
                Some(Type::Error)
            }
        }
        BinaryOp::And | BinaryOp::Or => {
            let mut valid = true;
            for operand in [&l, &r] {
                if !operand.is_error() && *operand != Type::Bool {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "Bool".into(),
                        found: operand.display(),
                        span: span_to_miette(span),
                        source_id: span.source,
                    });
                    valid = false;
                }
            }
            Some(if valid { Type::Bool } else { Type::Error })
        }
    }
}
