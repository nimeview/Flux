//! Pure value helpers used by the VM interpreter: binary operators, variant
//! inspection, IR value conversions and small predicates.

use std::collections::HashSet;

use flux_ir::{CompiledWatch, IrValue};
use flux_value::{
    add_int, div_int, float_from_f64, mul_int, sub_int, FloatWidth, IntWidth, OverflowError,
};

use crate::graph::RuntimeValue;
use crate::vm::{debug_assert_vm, stack_pop};

fn int_pair(a: &RuntimeValue, b: &RuntimeValue) -> Option<(IntWidth, u64, IntWidth, u64)> {
    match (a, b) {
        (
            RuntimeValue::Int {
                width: wa,
                bits: av,
            },
            RuntimeValue::Int {
                width: wb,
                bits: bv,
            },
        ) => Some((*wa, *av, *wb, *bv)),
        _ => None,
    }
}

fn push_overflow(stack: &mut Vec<RuntimeValue>, result: Result<RuntimeValue, OverflowError>) {
    match result {
        Ok(v) => stack.push(v),
        Err(OverflowError::DivByZero) => {
            stack.push(RuntimeValue::Result(Err(Box::new(RuntimeValue::String(
                "div by zero".into(),
            )))));
        }
        Err(_) => {
            stack.push(RuntimeValue::Result(Err(Box::new(RuntimeValue::String(
                "overflow".into(),
            )))));
        }
    }
}

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

pub(super) fn cmp<F>(stack: &mut Vec<RuntimeValue>, f: F)
where
    F: FnOnce(f64, f64) -> bool,
{
    let b = stack_pop(stack, "comparison rhs").as_float();
    let a = stack_pop(stack, "comparison lhs").as_float();
    if let (Some(a), Some(b)) = (a, b) {
        stack.push(RuntimeValue::Bool(f(a, b)));
    } else {
        debug_assert_vm(false, "comparison: expected numeric operands on stack");
    }
}

pub(super) fn add_binop(stack: &mut Vec<RuntimeValue>) {
    let b = stack_pop(stack, "Add rhs");
    let a = stack_pop(stack, "Add lhs");
    match (&a, &b) {
        (RuntimeValue::String(a), RuntimeValue::String(b)) => {
            stack.push(RuntimeValue::String(format!("{a}{b}")));
        }
        _ if int_pair(&a, &b).is_some() => {
            let (wa, av, wb, bv) = int_pair(&a, &b).unwrap();
            push_overflow(stack, add_int(wa, av, wb, bv));
        }
        _ if a.as_float().is_some() && b.as_float().is_some() => {
            stack.push(float_from_f64(
                FloatWidth::F64,
                a.as_float().unwrap() + b.as_float().unwrap(),
            ));
        }
        _ => {
            debug_assert_vm(
                false,
                &format!("Add: incompatible operands {a:?} and {b:?}"),
            );
        }
    }
}

pub(super) fn sub_binop(stack: &mut Vec<RuntimeValue>) {
    let b = stack_pop(stack, "Sub rhs");
    let a = stack_pop(stack, "Sub lhs");
    if let Some((wa, av, wb, bv)) = int_pair(&a, &b) {
        push_overflow(stack, sub_int(wa, av, wb, bv));
    } else if a.as_float().is_some() && b.as_float().is_some() {
        stack.push(float_from_f64(
            FloatWidth::F64,
            a.as_float().unwrap() - b.as_float().unwrap(),
        ));
    } else {
        debug_assert_vm(
            false,
            &format!("Sub: incompatible operands {a:?} and {b:?}"),
        );
    }
}

pub(super) fn mul_binop(stack: &mut Vec<RuntimeValue>) {
    let b = stack_pop(stack, "Mul rhs");
    let a = stack_pop(stack, "Mul lhs");
    if let Some((wa, av, wb, bv)) = int_pair(&a, &b) {
        push_overflow(stack, mul_int(wa, av, wb, bv));
    } else if a.as_float().is_some() && b.as_float().is_some() {
        stack.push(float_from_f64(
            FloatWidth::F64,
            a.as_float().unwrap() * b.as_float().unwrap(),
        ));
    } else {
        debug_assert_vm(
            false,
            &format!("Mul: incompatible operands {a:?} and {b:?}"),
        );
    }
}

pub(super) fn div_binop(stack: &mut Vec<RuntimeValue>) {
    let b = stack_pop(stack, "Div rhs");
    let a = stack_pop(stack, "Div lhs");
    if let Some((wa, av, wb, bv)) = int_pair(&a, &b) {
        push_overflow(stack, div_int(wa, av, wb, bv));
    } else if a.as_float().is_some() && b.as_float().is_some() {
        stack.push(float_from_f64(
            FloatWidth::F64,
            a.as_float().unwrap() / b.as_float().unwrap(),
        ));
    } else {
        debug_assert_vm(
            false,
            &format!("Div: incompatible operands {a:?} and {b:?}"),
        );
    }
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
