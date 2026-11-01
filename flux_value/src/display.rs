use crate::ops::float_to_f64;
use crate::{int_to_i128, Value};

/// Fully renders a value, including nested arrays and struct/enum fields.
pub fn render(value: &Value) -> String {
    match value {
        Value::Int { width, bits } => int_to_i128(*width, *bits).to_string(),
        Value::Float { width, bits } => float_text(float_to_f64(*width, *bits)),
        Value::Char(c) => format!("'{c}'"),
        Value::Bool(v) => v.to_string(),
        Value::String(v) => format!("\"{v}\""),
        Value::Unit => "()".to_string(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(render).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::UserStruct { type_name, fields } => {
            let mut names: Vec<&String> = fields.keys().collect();
            names.sort();
            let parts: Vec<String> = names
                .iter()
                .map(|name| format!("{name}: {}", render(&fields[*name])))
                .collect();
            format!("{type_name} {{ {} }}", parts.join(", "))
        }
        Value::UserEnum {
            type_name,
            variant,
            payload,
        } => match payload {
            Some(p) => format!("{type_name}::{variant}({})", render(p)),
            None => format!("{type_name}::{variant}"),
        },
        Value::Option(None) => "None".to_string(),
        Value::Option(Some(v)) => format!("Some({})", render(v)),
        Value::Result(Ok(v)) => format!("Ok({})", render(v)),
        Value::Result(Err(v)) => format!("Err({})", render(v)),
        Value::Map(entries) => {
            let parts: Vec<String> = entries
                .values()
                .map(|(k, v)| format!("{}: {}", render(k), render(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// A finite float always shows a decimal point (`5.0`, not `5`), so its type
/// is visible in output; `inf`, `-inf`, and `NaN` print as themselves.
fn float_text(f: f64) -> String {
    let text = f.to_string();
    if f.is_finite() && !text.contains(['.', 'e', 'E']) {
        format!("{text}.0")
    } else {
        text
    }
}
