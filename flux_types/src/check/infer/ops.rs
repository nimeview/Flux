//! Binary operator type inference.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_syntax::BinaryOp;

use crate::types::Type;

use super::super::util::{numeric_compatible, types_compatible, unwrap_value_type, widen_numeric};

/// Whether `==` is defined on values of `ty`: primitives, and compound
/// types (arrays, `Option`, `Result`, structs, enums) of equatable parts.
pub(crate) fn equality_supported(ty: &Type) -> bool {
    equatable(ty)
}

/// Whether a value of `ty` has a text form: anything without a function in
/// it, at any depth.
pub(crate) fn has_text_form(ty: &Type) -> bool {
    equatable(ty)
}

/// Types are finite trees: a recursive user type ends in a placeholder
/// (`Enum { variants: [] }`), whose parts the enclosing type already shows,
/// so this needs no depth limit (one let `[[…[f]…]]` 17 levels deep pass).
fn equatable(ty: &Type) -> bool {
    match unwrap_value_type(ty) {
        t if t.is_error() || t.is_integer() || t.is_float() => true,
        Type::Bool
        | Type::Char
        | Type::String
        | Type::Bytes
        | Type::Unit
        | Type::Empty
        | Type::Live(_)
        | Type::Var(_) => true,
        Type::Array(inner) | Type::Option(inner) => equatable(&inner),
        Type::Result { ok, err }
        | Type::Map {
            key: ok,
            value: err,
        } => equatable(&ok) && equatable(&err),
        Type::Struct { fields, .. } => fields.iter().all(|(_, t)| equatable(t)),
        Type::Tuple(elems) => elems.iter().all(equatable),
        Type::Enum { variants, .. } => variants
            .iter()
            .all(|(_, payload)| payload.as_ref().is_none_or(equatable)),
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

pub(crate) fn infer_binary(
    op: BinaryOp,
    left: &Type,
    right: &Type,
    span: flux_diagnostics::Span,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let l = unwrap_value_type(left);
    let r = unwrap_value_type(right);
    if l.is_error() || r.is_error() {
        // Unknown template operands defer constraints on T, but cannot make a
        // known invalid operand (e.g. Bool in addition) valid at any instance.
        let known = if l.is_error() { &r } else { &l };
        if !known.is_error() {
            let numeric = known.is_integer() || known.is_float();
            let (allowed, expected) = match op {
                BinaryOp::Add => (numeric || *known == Type::String, "numeric or String type"),
                BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Rem
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge => (numeric, "numeric type"),
                BinaryOp::And | BinaryOp::Or => (*known == Type::Bool, "Bool"),
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr => (known.is_integer(), "integer type"),
                BinaryOp::Eq | BinaryOp::Ne => (equality_supported(known), "equatable type"),
            };
            if !allowed {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: expected.into(),
                    found: known.display(),
                    span: span_to_miette(span),
                    source_id: span.source,
                });
            } else if matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge
                    | BinaryOp::And
                    | BinaryOp::Or
            ) {
                return Some(Type::Bool);
            } else if op == BinaryOp::Add && *known == Type::String {
                return Some(Type::String);
            }
        }
        return Some(Type::Error);
    }
    match op {
        // Integers only: `&`, `|`, `^` in the wider type, shifts in the left's.
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            if l.is_integer() && r.is_integer() {
                Some(if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
                    l.clone()
                } else {
                    widen_numeric(&l, &r)
                })
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "integer types for a bit operation".into(),
                    found: format!("{} and {}", l.display(), r.display()),
                    span: span_to_miette(span),
                    source_id: span.source,
                });
                Some(Type::Error)
            }
        }
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
            if numeric_compatible(&l, &r) || (l == Type::String && r == Type::String) {
                Some(Type::Bool)
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "numeric types or strings for ordered comparison".into(),
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
