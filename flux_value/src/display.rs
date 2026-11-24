use crate::ops::float_to_f64;
use crate::{deep_recursion, int_to_i128, Value};

/// Fully renders a value, including nested arrays and struct/enum fields.
pub fn render(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

/// Writes into one buffer: rendering each level into its own string copied
/// every inner level again, quadratic on a long recursive value.
fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Int { width, bits } => out.push_str(&int_to_i128(*width, *bits).to_string()),
        Value::Float { width, bits } => out.push_str(&float_text(float_to_f64(*width, *bits))),
        Value::Char(c) => {
            out.push('\'');
            out.push_str(&c.to_string());
            out.push('\'');
        }
        Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Value::String(v) => {
            out.push('"');
            out.push_str(v);
            out.push('"');
        }
        Value::Unit => out.push_str("()"),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(out, item);
            }
            out.push(']');
        }
        // A tuple: a struct of this name with fields "0", "1", ….
        Value::UserStruct(s) if s.type_name == TUPLE_TYPE => deep_recursion(|| {
            out.push('(');
            for i in 0..s.fields.len() {
                if i > 0 {
                    out.push_str(", ");
                }
                if let Some(value) = s.fields.get(&i.to_string()) {
                    write_value(out, value);
                }
            }
            out.push(')');
        }),
        Value::UserStruct(s) => deep_recursion(|| {
            let mut names: Vec<&String> = s.fields.keys().collect();
            names.sort();
            out.push_str(&s.type_name);
            out.push_str(" { ");
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                out.push_str(": ");
                write_value(out, &s.fields[*name]);
            }
            out.push_str(" }");
        }),
        Value::UserEnum(e) => deep_recursion(|| {
            out.push_str(&e.type_name);
            out.push_str("::");
            out.push_str(&e.variant);
            // A struct variant: `Shape::Rect { h: 3, w: 2 }`.
            if let Some(p @ Value::UserStruct(s)) = &e.payload {
                if s.type_name == VARIANT_FIELDS_TYPE {
                    let mut text = String::new();
                    write_value(&mut text, p);
                    out.push(' ');
                    out.push_str(text.trim_start_matches(VARIANT_FIELDS_TYPE).trim_start());
                    return;
                }
            }
            if let Some(p) = &e.payload {
                out.push('(');
                write_value(out, p);
                out.push(')');
            }
        }),
        Value::Closure(_) => out.push_str("<closure>"),
        Value::Option(None) => out.push_str("None"),
        Value::Option(Some(v)) => write_wrapped(out, "Some", v),
        Value::Result(Ok(v)) => write_wrapped(out, "Ok", v),
        Value::Result(Err(v)) => write_wrapped(out, "Err", v),
        Value::Map(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.values().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(out, k);
                out.push_str(": ");
                write_value(out, v);
            }
            out.push('}');
        }
    }
}

fn write_wrapped(out: &mut String, name: &str, value: &Value) {
    out.push_str(name);
    out.push('(');
    write_value(out, value);
    out.push(')');
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

/// The type name a tuple value carries (the compiler's `flux_hir::TUPLE`).
pub const TUPLE_TYPE: &str = "\0tuple";

/// The struct name a struct variant's fields carry (the compiler's
/// `flux_hir::VARIANT_FIELDS`).
pub const VARIANT_FIELDS_TYPE: &str = "\0fields";
