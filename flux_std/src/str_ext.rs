//! Extended string intrinsics.

use flux_value::Value;

pub fn starts_with(hay: &Value, needle: &Value) -> Option<Value> {
    let h = match hay {
        Value::String(s) => s,
        _ => return Some(Value::Bool(false)),
    };
    let n = match needle {
        Value::String(s) => s,
        _ => return Some(Value::Bool(false)),
    };
    Some(Value::Bool(h.starts_with(n.as_str())))
}

pub fn ends_with(hay: &Value, needle: &Value) -> Option<Value> {
    let h = match hay {
        Value::String(s) => s,
        _ => return Some(Value::Bool(false)),
    };
    let n = match needle {
        Value::String(s) => s,
        _ => return Some(Value::Bool(false)),
    };
    Some(Value::Bool(h.ends_with(n.as_str())))
}

pub fn replace(s: &Value, from: &Value, to: &Value) -> Option<Value> {
    let src = match s {
        Value::String(s) => s,
        _ => return Some(Value::String(String::new())),
    };
    let from = match from {
        Value::String(s) => s.as_str(),
        _ => return Some(Value::String(src.clone())),
    };
    let to = match to {
        Value::String(s) => s.as_str(),
        _ => return Some(Value::String(src.clone())),
    };
    Some(Value::String(src.replace(from, to)))
}

pub fn to_lower(s: &Value) -> Option<Value> {
    Some(Value::String(match s {
        Value::String(v) => v.to_lowercase(),
        _ => String::new(),
    }))
}

pub fn to_upper(s: &Value) -> Option<Value> {
    Some(Value::String(match s {
        Value::String(v) => v.to_uppercase(),
        _ => String::new(),
    }))
}

fn ok(v: Value) -> Value {
    Value::Result(Ok(Box::new(v)))
}

fn err(msg: String) -> Value {
    Value::Result(Err(Box::new(Value::String(msg))))
}

/// `parse_int(s)`: an optional `+`/`-` then ASCII digits, within `Int64`.
/// No whitespace is trimmed.
pub fn parse_int(s: &Value) -> Value {
    let Value::String(text) = s else {
        return err("parse_int expects a String".into());
    };
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return err(format!("invalid integer: `{text}`"));
    }
    match text.parse::<i64>() {
        Ok(n) => ok(Value::int_i64(n)),
        Err(_) => err(format!("integer out of range: `{text}`")),
    }
}

/// `parse_float(s)`: decimal or exponent notation (`1`, `-2.5`, `1e3`). The
/// result must be finite; `inf`/`NaN` spellings are rejected. No trimming.
pub fn parse_float(s: &Value) -> Value {
    let Value::String(text) = s else {
        return err("parse_float expects a String".into());
    };
    let plain = text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'));
    match text.parse::<f64>() {
        Ok(f) if plain && f.is_finite() => ok(Value::float_f64(f)),
        _ => err(format!("invalid number: `{text}`")),
    }
}
