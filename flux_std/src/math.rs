//! Math intrinsics for Flux stdlib.

use flux_value::Value;

pub fn abs(v: &Value) -> Option<Value> {
    match v.as_float() {
        Some(n) => Some(Value::float_f64(n.abs())),
        None => v.as_int().map(|n| Value::int_i64(n.abs())),
    }
}

pub fn min(a: &Value, b: &Value) -> Option<Value> {
    match (a.as_float(), b.as_float()) {
        (Some(x), Some(y)) => Some(Value::float_f64(x.min(y))),
        _ => {
            let x = a.as_int()?;
            let y = b.as_int()?;
            Some(Value::int_i64(x.min(y)))
        }
    }
}

pub fn max(a: &Value, b: &Value) -> Option<Value> {
    match (a.as_float(), b.as_float()) {
        (Some(x), Some(y)) => Some(Value::float_f64(x.max(y))),
        _ => {
            let x = a.as_int()?;
            let y = b.as_int()?;
            Some(Value::int_i64(x.max(y)))
        }
    }
}

pub fn clamp(v: &Value, lo: &Value, hi: &Value) -> Option<Value> {
    max(lo, &min(v, hi)?)
}

pub fn sqrt(v: &Value) -> Option<Value> {
    Some(Value::float_f64(v.as_float()?.sqrt()))
}

pub fn floor(v: &Value) -> Option<Value> {
    Some(Value::float_f64(v.as_float()?.floor()))
}

pub fn ceil(v: &Value) -> Option<Value> {
    Some(Value::float_f64(v.as_float()?.ceil()))
}

pub fn pow(base: &Value, exp: &Value) -> Option<Value> {
    Some(Value::float_f64(base.as_float()?.powf(exp.as_float()?)))
}

pub fn assert(cond: &Value) -> Option<Value> {
    if cond.as_bool() == Some(true) {
        Some(Value::Unit)
    } else {
        panic!("assertion failed");
    }
}

pub fn assert_eq(a: &Value, b: &Value) -> Option<Value> {
    if a == b {
        Some(Value::Unit)
    } else {
        panic!(
            "assert_eq failed: {} != {}",
            a.display_string(),
            b.display_string()
        );
    }
}
