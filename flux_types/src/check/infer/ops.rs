//! Binary operator type inference.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_syntax::BinaryOp;

use crate::types::Type;

use super::super::util::{numeric_compatible, types_compatible, unwrap_value_type, widen_numeric};

/// Whether `==` is defined on values of `ty`: primitives, and compound
/// types (arrays, `Option`, `Result`, structs, enums) of equatable parts.
pub(crate) fn equality_supported(ty: &Type) -> bool {
    equatable(ty, 0)
}

fn equatable(ty: &Type, depth: u32) -> bool {
    // Recursive user types (a list enum) are equatable if their known parts
    // are; stop descending rather than loop.
    if depth > 16 {
        return true;
    }
    let d = depth + 1;
    match unwrap_value_type(ty) {
        t if t.is_error() || t.is_integer() || t.is_float() => true,
        Type::Bool | Type::Char | Type::String | Type::Unit | Type::Empty | Type::Var(_) => true,
        Type::Array(inner) | Type::Option(inner) => equatable(&inner, d),
        Type::Result { ok, err }
        | Type::Map {
            key: ok,
            value: err,
        } => equatable(&ok, d) && equatable(&err, d),
        Type::Struct { fields, .. } => fields.iter().all(|(_, t)| equatable(t, d)),
        Type::Enum { variants, .. } => variants
            .iter()
            .all(|(_, payload)| payload.as_ref().is_none_or(|t| equatable(t, d))),
        _ => false,
    }
}

/// Whether `l == r` type-checks: numbers of any widths, or one equatable
/// type on both sides (inference placeholders such as `Result<Int, _>` vs
/// `Result<_, String>` unify).
pub(crate) fn equality_allowed(l: &Type, r: &Type) -> bool {
    let (l, r) = (unwrap_value_type(l), unwrap_value_type(r));
    l.is_error()
        || r.is_error()
        || numeric_compatible(&l, &r)
        || (types_compatible(&l, &r) && types_compatible(&r, &l) && equality_supported(&l))
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
            if equality_allowed(&l, &r) {
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
