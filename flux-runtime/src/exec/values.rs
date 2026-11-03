//! Pure value helpers used by the VM interpreter: binary operators, variant
//! inspection, IR value conversions and small predicates.

use flux_ir::IrValue;
use flux_value::{ArithError, NumOp};

use crate::graph::RuntimeValue;
use crate::vm::{debug_assert_vm, stack_pop};

pub(super) fn variant_tag_eq(val: Option<&RuntimeValue>, variant: &str) -> bool {
    match val {
        Some(RuntimeValue::UserEnum(e)) => e.variant == variant,
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
        Some(RuntimeValue::UserEnum(e)) => {
            let payload = &e.payload;
            if let Some(field) = field {
                if let Some(RuntimeValue::UserStruct(s)) = payload {
                    return match s.fields.get(field) {
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
                payload.clone().unwrap_or_else(|| {
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

/// Both top operands as `Int64`: the common case, computed in place
/// instead of moving two values off the stack and one back.
fn top_i64_pair(stack: &[RuntimeValue]) -> Option<(i64, i64)> {
    use flux_value::IntWidth::I64;
    match stack {
        [.., RuntimeValue::Int {
            width: I64,
            bits: a,
        }, RuntimeValue::Int {
            width: I64,
            bits: b,
        }] => Some((*a as i64, *b as i64)),
        _ => None,
    }
}

/// Ordered comparison; NaN compares false, matching IEEE.
pub(super) fn cmp<F>(stack: &mut Vec<RuntimeValue>, f: F)
where
    F: FnOnce(std::cmp::Ordering) -> bool,
{
    if let Some((x, y)) = top_i64_pair(stack) {
        stack.pop();
        if let Some(top) = stack.last_mut() {
            *top = RuntimeValue::Bool(f(x.cmp(&y)));
        }
        return;
    }
    let b = stack_pop(stack, "comparison rhs");
    let a = stack_pop(stack, "comparison lhs");
    debug_assert_vm(
        (a.is_numeric() && b.is_numeric())
            || (matches!(a, RuntimeValue::String(_)) && matches!(b, RuntimeValue::String(_))),
        "comparison: expected numbers or strings on stack",
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
    if let Some((x, y)) = top_i64_pair(stack) {
        let result = flux_value::binary_i64(op, x, y)?;
        stack.pop();
        if let Some(top) = stack.last_mut() {
            *top = result;
        }
        return Ok(());
    }
    let b = stack_pop(stack, "arithmetic rhs");
    let a = stack_pop(stack, "arithmetic lhs");
    if let (NumOp::Add, RuntimeValue::String(a), RuntimeValue::String(b)) = (op, &a, &b) {
        stack.push(RuntimeValue::String(flux_value::concat_strings(a, b)?));
        return Ok(());
    }
    stack.push(flux_value::binary(op, &a, &b)?);
    Ok(())
}

pub(super) fn to_runtime(v: &IrValue) -> RuntimeValue {
    match v {
        IrValue::Int(n) => i64::try_from(*n)
            .map(RuntimeValue::int_i64)
            .unwrap_or_else(|_| {
                flux_value::int_from_i128(flux_value::IntWidth::U64, *n)
                    .unwrap_or(RuntimeValue::Unit)
            }),
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

/// A function's name as the source wrote it (`__mod_1__f`, `__closure_0`).
pub(super) fn display_fn_name(name: &str) -> String {
    if name.starts_with("__closure_") {
        return "<closure>".to_string();
    }
    match name
        .strip_prefix("__mod_")
        .and_then(|rest| rest.split_once("__"))
    {
        Some((_, base)) => base.to_string(),
        None => name.to_string(),
    }
}
