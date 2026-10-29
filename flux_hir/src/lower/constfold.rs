//! Compile-time constant folding of module-level `HirExpr` values.
//!
//! Source bindings whose initializer folds to a literal are stored as constants
//! (rather than derived nodes), which lets the runtime treat them as writable
//! sources. Only pure literal arithmetic/logic folds; anything referencing names,
//! calls or reactive reads is left untouched.

use flux_diagnostics::Span;
use flux_syntax::BinaryOp;

use crate::nodes::HirExpr;

/// Compile-time values used for constant folding at module scope.
enum ConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

pub(super) fn fold_const_hir_expr(expr: &HirExpr) -> Option<HirExpr> {
    let span = expr.span();
    let value = eval_const_hir_expr(expr)?;
    Some(const_value_to_hir(value, span))
}

fn eval_const_hir_expr(expr: &HirExpr) -> Option<ConstValue> {
    match expr {
        HirExpr::Int { value, .. } => Some(ConstValue::Int(*value)),
        HirExpr::Float { value, .. } => Some(ConstValue::Float(*value)),
        HirExpr::Bool { value, .. } => Some(ConstValue::Bool(*value)),
        HirExpr::String { value, .. } => Some(ConstValue::String(value.clone())),
        HirExpr::Unary { op, expr, .. } => match op {
            flux_syntax::UnaryOp::Neg => match eval_const_hir_expr(expr)? {
                ConstValue::Int(n) => Some(ConstValue::Int(-n)),
                ConstValue::Float(f) => Some(ConstValue::Float(-f)),
                _ => None,
            },
            flux_syntax::UnaryOp::Not => match eval_const_hir_expr(expr)? {
                ConstValue::Bool(b) => Some(ConstValue::Bool(!b)),
                _ => None,
            },
        },
        HirExpr::Binary {
            op, left, right, ..
        } => {
            let left = eval_const_hir_expr(left)?;
            let right = eval_const_hir_expr(right)?;
            eval_const_binary(*op, left, right)
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            let ConstValue::Bool(b) = eval_const_hir_expr(cond)? else {
                return None;
            };
            if b {
                eval_const_hir_expr(then_expr)
            } else {
                eval_const_hir_expr(else_expr)
            }
        }
        HirExpr::Ident { .. }
        | HirExpr::Snap { .. }
        | HirExpr::Call { .. }
        | HirExpr::Field { .. }
        | HirExpr::Closure { .. }
        | HirExpr::Array { .. }
        | HirExpr::StructLit { .. }
        | HirExpr::EnumVariant { .. }
        | HirExpr::Match { .. }
        | HirExpr::Cast { .. } => None,
    }
}

fn eval_const_binary(op: BinaryOp, left: ConstValue, right: ConstValue) -> Option<ConstValue> {
    use flux_syntax::BinaryOp as Op;

    match op {
        Op::Add => match (left, right) {
            (ConstValue::String(a), ConstValue::String(b)) => {
                Some(ConstValue::String(format!("{a}{b}")))
            }
            (ConstValue::Int(x), ConstValue::Int(y)) => Some(ConstValue::Int(x + y)),
            (ConstValue::Float(x), ConstValue::Float(y)) => Some(ConstValue::Float(x + y)),
            (ConstValue::Int(x), ConstValue::Float(y)) => Some(ConstValue::Float(x as f64 + y)),
            (ConstValue::Float(x), ConstValue::Int(y)) => Some(ConstValue::Float(x + y as f64)),
            _ => None,
        },
        Op::Sub => numeric_binop(left, right, |x, y| x - y),
        Op::Mul => numeric_binop(left, right, |x, y| x * y),
        Op::Div => {
            let (x, y) = as_float_pair(left, right)?;
            if y == 0.0 {
                return None;
            }
            Some(ConstValue::Float(x / y))
        }
        Op::Eq => {
            if !const_values_comparable(&left, &right) {
                return None;
            }
            Some(ConstValue::Bool(const_cmp_eq(left, right)))
        }
        Op::Ne => {
            if !const_values_comparable(&left, &right) {
                return None;
            }
            Some(ConstValue::Bool(!const_cmp_eq(left, right)))
        }
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let (x, y) = as_float_pair(left, right)?;
            Some(ConstValue::Bool(match op {
                Op::Lt => x < y,
                Op::Le => x <= y,
                Op::Gt => x > y,
                Op::Ge => x >= y,
                _ => unreachable!(),
            }))
        }
        Op::And => match (left, right) {
            (ConstValue::Bool(a), ConstValue::Bool(b)) => Some(ConstValue::Bool(a && b)),
            _ => None,
        },
        Op::Or => match (left, right) {
            (ConstValue::Bool(a), ConstValue::Bool(b)) => Some(ConstValue::Bool(a || b)),
            _ => None,
        },
    }
}

fn numeric_binop(
    left: ConstValue,
    right: ConstValue,
    f: fn(f64, f64) -> f64,
) -> Option<ConstValue> {
    match (left, right) {
        (ConstValue::Int(x), ConstValue::Int(y)) => {
            Some(ConstValue::Int(f(x as f64, y as f64) as i64))
        }
        (left, right) => {
            let (x, y) = as_float_pair(left, right)?;
            Some(ConstValue::Float(f(x, y)))
        }
    }
}

fn as_float_pair(left: ConstValue, right: ConstValue) -> Option<(f64, f64)> {
    match (left, right) {
        (ConstValue::Int(x), ConstValue::Int(y)) => Some((x as f64, y as f64)),
        (ConstValue::Float(x), ConstValue::Float(y)) => Some((x, y)),
        (ConstValue::Int(x), ConstValue::Float(y)) => Some((x as f64, y)),
        (ConstValue::Float(x), ConstValue::Int(y)) => Some((x, y as f64)),
        _ => None,
    }
}

fn const_cmp_eq(left: ConstValue, right: ConstValue) -> bool {
    match (left, right) {
        (ConstValue::Int(x), ConstValue::Int(y)) => x == y,
        (ConstValue::Float(x), ConstValue::Float(y)) => x == y,
        (ConstValue::Bool(x), ConstValue::Bool(y)) => x == y,
        (ConstValue::String(x), ConstValue::String(y)) => x == y,
        (ConstValue::Int(x), ConstValue::Float(y)) => x as f64 == y,
        (ConstValue::Float(x), ConstValue::Int(y)) => x == y as f64,
        _ => false,
    }
}

fn const_values_comparable(left: &ConstValue, right: &ConstValue) -> bool {
    matches!(
        (left, right),
        (ConstValue::Int(_), ConstValue::Int(_))
            | (ConstValue::Float(_), ConstValue::Float(_))
            | (ConstValue::Int(_), ConstValue::Float(_))
            | (ConstValue::Float(_), ConstValue::Int(_))
            | (ConstValue::Bool(_), ConstValue::Bool(_))
            | (ConstValue::String(_), ConstValue::String(_))
    )
}

fn const_value_to_hir(value: ConstValue, span: Span) -> HirExpr {
    match value {
        ConstValue::Int(n) => HirExpr::Int { value: n, span },
        ConstValue::Float(n) => HirExpr::Float { value: n, span },
        ConstValue::Bool(b) => HirExpr::Bool { value: b, span },
        ConstValue::String(s) => HirExpr::String { value: s, span },
    }
}
