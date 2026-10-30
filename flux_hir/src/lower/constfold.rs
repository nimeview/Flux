//! Compile-time constant folding of module-level `HirExpr` values.
//!
//! Source bindings whose initializer folds to a literal are stored as constants
//! (rather than derived nodes), which lets the runtime treat them as writable
//! sources. Only pure literal arithmetic/logic folds; anything referencing names,
//! calls or reactive reads is left untouched.
//!
//! Arithmetic uses the runtime's own semantics (`flux_value`). An operation that
//! would raise a runtime error (overflow, division by zero) is not folded, so the
//! program raises the same error when it runs.

use flux_diagnostics::Span;
use flux_syntax::BinaryOp;
use flux_value::{FloatWidth, IntWidth, NumOp, Value};

use crate::nodes::HirExpr;

pub(super) fn fold_const_hir_expr(expr: &HirExpr) -> Option<HirExpr> {
    let span = expr.span();
    let value = eval_const_hir_expr(expr)?;
    const_value_to_hir(value, span)
}

fn eval_const_hir_expr(expr: &HirExpr) -> Option<Value> {
    match expr {
        HirExpr::Int { value, .. } => Some(Value::int_i64(*value)),
        HirExpr::Float { value, .. } => Some(Value::float_f64(*value)),
        HirExpr::Bool { value, .. } => Some(Value::Bool(*value)),
        HirExpr::String { value, .. } => Some(Value::String(value.clone())),
        HirExpr::Unary { op, expr, .. } => match op {
            flux_syntax::UnaryOp::Neg => flux_value::negate(&eval_const_hir_expr(expr)?).ok(),
            flux_syntax::UnaryOp::Not => match eval_const_hir_expr(expr)? {
                Value::Bool(b) => Some(Value::Bool(!b)),
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
            let Value::Bool(b) = eval_const_hir_expr(cond)? else {
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

fn eval_const_binary(op: BinaryOp, left: Value, right: Value) -> Option<Value> {
    use flux_syntax::BinaryOp as Op;

    let num_op = match op {
        Op::Add => NumOp::Add,
        Op::Sub => NumOp::Sub,
        Op::Mul => NumOp::Mul,
        Op::Div => NumOp::Div,
        Op::Rem => NumOp::Rem,
        Op::Eq | Op::Ne => {
            if std::mem::discriminant(&left) != std::mem::discriminant(&right)
                && !(left.is_numeric() && right.is_numeric())
            {
                return None;
            }
            let eq = flux_value::values_equal(&left, &right);
            return Some(Value::Bool(if op == Op::Eq { eq } else { !eq }));
        }
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            if !(left.is_numeric() && right.is_numeric()) {
                return None;
            }
            let ord = flux_value::compare(&left, &right);
            return Some(Value::Bool(ord.is_some_and(|o| match op {
                Op::Lt => o.is_lt(),
                Op::Le => o.is_le(),
                Op::Gt => o.is_gt(),
                _ => o.is_ge(),
            })));
        }
        Op::And | Op::Or => {
            return match (left, right) {
                (Value::Bool(a), Value::Bool(b)) => {
                    Some(Value::Bool(if op == Op::And { a && b } else { a || b }))
                }
                _ => None,
            };
        }
    };
    if let (NumOp::Add, Value::String(a), Value::String(b)) = (num_op, &left, &right) {
        return Some(Value::String(format!("{a}{b}")));
    }
    flux_value::binary(num_op, &left, &right).ok()
}

fn const_value_to_hir(value: Value, span: Span) -> Option<HirExpr> {
    Some(match value {
        Value::Int {
            width: IntWidth::I64,
            ..
        } => HirExpr::Int {
            value: value.as_int()?,
            span,
        },
        Value::Float {
            width: FloatWidth::F64,
            ..
        } => HirExpr::Float {
            value: value.as_float()?,
            span,
        },
        Value::Bool(b) => HirExpr::Bool { value: b, span },
        Value::String(s) => HirExpr::String { value: s, span },
        _ => return None,
    })
}
