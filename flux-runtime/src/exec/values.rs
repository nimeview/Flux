//! Pure value helpers used by the VM interpreter: binary operators, variant
//! inspection, IR value conversions and small predicates.

use std::collections::HashSet;

use flux_ir::{CompiledWatch, IrValue};
use flux_value::{ArithError, NumOp};

use crate::graph::RuntimeValue;
use crate::vm::{debug_assert_vm, stack_pop};

pub(super) fn variant_tag_eq(val: Option<&RuntimeValue>, variant: &str) -> bool {
    match val {
        Some(RuntimeValue::UserEnum { variant: v, .. }) => v == variant,
        Some(RuntimeValue::Option(Some(_))) => variant == "Some",
        Some(RuntimeValue::Option(None)) => variant == "None",
        Some(RuntimeValue::Result(Ok(_))) => variant == "Ok",
        Some(RuntimeValue::Result(Err(_))) => variant == "Err",
        _ => false,
    }
}

pub(super) fn extract_variant_payload(
    val: Option<&RuntimeValue>,
    field: Option<&str>,
) -> RuntimeValue {
    match val {
        None => {
            debug_assert_vm(false, "VariantPayloadGet: missing value on stack");
            RuntimeValue::Unit
        }
        Some(RuntimeValue::UserEnum { payload, .. }) => {
            if let Some(field) = field {
                if let Some(RuntimeValue::UserStruct { fields, .. }) = payload.as_deref() {
                    return match fields.get(field) {
                        Some(v) => v.clone(),
                        None => {
                            debug_assert_vm(
                                false,
                                &format!("VariantPayloadGet: field `{field}` not found"),
                            );
                            RuntimeValue::Unit
                        }
                    };
                }
                debug_assert_vm(false, "VariantPayloadGet: expected struct payload");
                RuntimeValue::Unit
            } else {
                payload.as_deref().cloned().unwrap_or_else(|| {
                    debug_assert_vm(false, "VariantPayloadGet: missing enum payload");
                    RuntimeValue::Unit
                })
            }
        }
        Some(RuntimeValue::Option(Some(inner))) => *inner.clone(),
        Some(RuntimeValue::Result(Ok(inner))) => *inner.clone(),
        Some(RuntimeValue::Result(Err(inner))) => *inner.clone(),
        other => {
            debug_assert_vm(
                false,
                &format!("VariantPayloadGet: expected variant value, got {other:?}"),
            );
            RuntimeValue::Unit
        }
    }
}

/// Ordered comparison; NaN compares false, matching IEEE.
pub(super) fn cmp<F>(stack: &mut Vec<RuntimeValue>, f: F)
where
    F: FnOnce(std::cmp::Ordering) -> bool,
{
    let b = stack_pop(stack, "comparison rhs");
    let a = stack_pop(stack, "comparison lhs");
    debug_assert_vm(
        a.is_numeric() && b.is_numeric(),
        "comparison: expected numeric operands on stack",
    );
    stack.push(RuntimeValue::Bool(
        flux_value::compare(&a, &b).is_some_and(f),
    ));
}

pub(super) fn eq_binop(stack: &mut Vec<RuntimeValue>, negate: bool) {
    let b = stack_pop(stack, "equality rhs");
    let a = stack_pop(stack, "equality lhs");
    stack.push(RuntimeValue::Bool(
        flux_value::values_equal(&a, &b) != negate,
    ));
}

/// `+ - * / %`; `+` also concatenates strings.
pub(super) fn arith_binop(stack: &mut Vec<RuntimeValue>, op: NumOp) -> Result<(), ArithError> {
    let b = stack_pop(stack, "arithmetic rhs");
    let a = stack_pop(stack, "arithmetic lhs");
    if let (NumOp::Add, RuntimeValue::String(a), RuntimeValue::String(b)) = (op, &a, &b) {
        stack.push(RuntimeValue::String(format!("{a}{b}")));
        return Ok(());
    }
    stack.push(flux_value::binary(op, &a, &b)?);
    Ok(())
}

pub(super) fn watch_should_run(watch: &CompiledWatch, changed: &HashSet<String>) -> bool {
    if let Some(target) = &watch.target {
        changed.contains(target)
    } else {
        watch.deps.iter().any(|dep| changed.contains(dep))
    }
}

pub(super) fn to_runtime(v: &IrValue) -> RuntimeValue {
    match v {
        IrValue::Int(n) => RuntimeValue::int_i64(*n),
        IrValue::Float(n) => RuntimeValue::float_f64(*n),
        IrValue::Bool(b) => RuntimeValue::Bool(*b),
        IrValue::String(s) => RuntimeValue::String(s.clone()),
        IrValue::Unit => RuntimeValue::Unit,
    }
}

pub(super) fn code_has_lazy_if(code: &[flux_ir::Inst]) -> bool {
    use flux_ir::Inst;
    code.iter().any(|inst| match inst {
        Inst::LazyIf { .. } => true,
        Inst::ShortAnd(inner) | Inst::ShortOr(inner) => code_has_lazy_if(inner),
        _ => false,
    })
}

pub(super) fn eval_const(expr: &Option<IrValue>) -> Option<RuntimeValue> {
    expr.as_ref().map(to_runtime)
}
