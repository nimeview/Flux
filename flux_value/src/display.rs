use crate::ops::float_to_f64;
use crate::{int_to_i128, Value};

/// Fully renders a value, including nested arrays and struct/enum fields.
pub fn render(value: &Value) -> String {
    match value {
        Value::Int { width, bits } => int_to_i128(*width, *bits).to_string(),
        Value::Float { width, bits } => float_to_f64(*width, *bits).to_string(),
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
    }
}
