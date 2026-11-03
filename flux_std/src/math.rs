//! Math intrinsics for Flux stdlib.

use flux_value::Value;

fn promote_numeric(values: &[&Value]) -> Option<Vec<Value>> {
    if values
        .iter()
        .all(|value| matches!(value, Value::Int { .. }))
    {
        let mut width = None;
        for value in values {
            let Value::Int { width: next, .. } = value else {
                return None;
            };
            width = Some(width.map_or(*next, |current| flux_value::promote_int(current, *next)));
        }
        let width = width?;
        return values
            .iter()
            .map(|value| {
                let Value::Int {
                    width: source_width,
                    bits,
                } = value
                else {
                    return None;
                };
                let exact = flux_value::int_to_i128(*source_width, *bits);
                flux_value::int_from_i128(width, exact)
            })
            .collect();
    }

    let width =
        values
            .iter()
            .try_fold(flux_value::FloatWidth::F32, |width, value| match value {
                Value::Float {
                    width: flux_value::FloatWidth::F64,
                    ..
                } => Some(flux_value::FloatWidth::F64),
                Value::Float { .. } | Value::Int { .. } => Some(width),
                _ => None,
            })?;
    values
        .iter()
        .map(|value| Some(flux_value::float_from_f64(width, value.as_float()?)))
        .collect()
}

fn minmax(a: &Value, b: &Value, choose_min: bool) -> Option<Value> {
    let values = promote_numeric(&[a, b])?;
    if let (Value::Float { width, bits: left }, Value::Float { bits: right, .. }) =
        (&values[0], &values[1])
    {
        let left = flux_value::float_to_f64(*width, *left);
        let right = flux_value::float_to_f64(*width, *right);
        return Some(flux_value::float_from_f64(
            *width,
            if choose_min {
                left.min(right)
            } else {
                left.max(right)
            },
        ));
    }
    let order = flux_value::compare(&values[0], &values[1])?;
    let take_right = if choose_min {
        order.is_gt()
    } else {
        order.is_lt()
    };
    Some(if take_right {
        values[1].clone()
    } else {
        values[0].clone()
    })
}

pub fn abs(v: &Value) -> Option<Value> {
    v.as_float().map(|n| Value::float_f64(n.abs()))
}

/// An integer's magnitude at its own width; the signed minimum's does not fit.
pub fn abs_int(v: &Value) -> Result<Value, flux_value::ArithError> {
    match flux_value::compare(v, &Value::int_i64(0)) {
        Some(std::cmp::Ordering::Less) => flux_value::negate(v),
        _ => Ok(v.clone()),
    }
}

pub fn min(a: &Value, b: &Value) -> Option<Value> {
    minmax(a, b, true)
}

pub fn max(a: &Value, b: &Value) -> Option<Value> {
    minmax(a, b, false)
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

pub fn assert(cond: &Value) -> Result<Value, flux_value::ArithError> {
    if cond.as_bool() == Some(true) {
        Ok(Value::Unit)
    } else {
        Err(flux_value::ArithError::AssertionFailed)
    }
}

pub fn assert_eq(a: &Value, b: &Value) -> Result<Value, flux_value::ArithError> {
    if flux_value::values_equal(a, b) {
        Ok(Value::Unit)
    } else {
        crate::set_failure_detail(format!("{} != {}", a.display_string(), b.display_string()));
        Err(flux_value::ArithError::AssertionFailed)
    }
}
