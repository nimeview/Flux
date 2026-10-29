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
