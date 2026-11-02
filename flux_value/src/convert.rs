//! Conversions between rich [`Value`] and compact [`TaggedVal`].

use crate::heap::{get_array, get_enum, get_string, intern_string, store_enum};
use crate::ops::{float_to_f64, int_to_i64};
use crate::tagged::{float_tag, int_tag, tag_float_width, tag_int_width, tag_kind};
use crate::{TaggedVal, Value, TAG_ARRAY, TAG_BOOL, TAG_CHAR, TAG_ENUM, TAG_STRING};

/// Encode a runtime value into the JIT slot representation.
pub fn value_to_tagged(v: &Value) -> TaggedVal {
    match v {
        // Ints keep their sign-extended value in `data` so hosts can read it as i64.
        Value::Int { width, bits } => TaggedVal {
            tag: int_tag(*width),
            data: int_to_i64(*width, *bits) as u64,
        },
        Value::Float { width, bits } => TaggedVal {
            tag: float_tag(*width),
            data: float_to_f64(*width, *bits).to_bits(),
        },
        Value::Char(c) => TaggedVal {
            tag: TAG_CHAR,
            data: *c as u64,
        },
        Value::Bool(b) => TaggedVal::from_bool(*b),
        Value::String(s) => TaggedVal::from_string_id(intern_string(s)),
        Value::Array(elems) => {
            let items: Vec<TaggedVal> = elems.iter().map(value_to_tagged).collect();
            TaggedVal::from_array_id(crate::heap::store_array(items))
        }
        Value::UserEnum(e) => {
            let p = e.payload.as_ref().map(value_to_tagged);
            TaggedVal::from_enum_id(store_enum(&e.type_name, &e.variant, p))
        }
        Value::Unit => TaggedVal::UNIT,
        Value::UserStruct(_) | Value::Option(_) | Value::Result(_) | Value::Map(_) => {
            TaggedVal::UNIT
        }
    }
}

/// Whether [`value_to_tagged`] keeps `v` intact. Structs, `Option`, and
/// `Result` have no tagged form (they become `Unit`), so code that receives
/// one must run on the VM.
pub fn is_tag_representable(v: &Value) -> bool {
    match v {
        Value::Int { .. }
        | Value::Float { .. }
        | Value::Char(_)
        | Value::Bool(_)
        | Value::String(_)
        | Value::Unit => true,
        Value::Array(items) => items.iter().all(is_tag_representable),
        Value::UserEnum(e) => e.payload.as_ref().is_none_or(is_tag_representable),
        Value::UserStruct(_) | Value::Option(_) | Value::Result(_) | Value::Map(_) => false,
    }
}

/// Decode a tagged slot value into a runtime value.
pub fn tagged_to_value(tv: TaggedVal) -> Value {
    if let Some(width) = tag_int_width(tv.tag) {
        let mask = if width.bits() == 64 {
            u64::MAX
        } else {
            (1u64 << width.bits()) - 1
        };
        return Value::Int {
            width,
            bits: tv.data & mask,
        };
    }
    if let Some(width) = tag_float_width(tv.tag) {
        return crate::ops::float_from_f64(width, f64::from_bits(tv.data));
    }
    match tag_kind(tv.tag) {
        TAG_CHAR => Value::Char(tv.data as u32),
        TAG_BOOL => Value::Bool(tv.data != 0),
        TAG_ARRAY => get_array(tv.data)
            .map(|elems| Value::array(elems.into_iter().map(tagged_to_value).collect()))
            .unwrap_or(Value::Unit),
        TAG_STRING => get_string(tv.data)
            .map(Value::String)
            .unwrap_or(Value::Unit),
        TAG_ENUM => get_enum(tv.data)
            .map(|(type_name, variant, payload)| {
                Value::user_enum(type_name, variant, payload.map(tagged_to_value))
            })
            .unwrap_or(Value::Unit),
        _ => Value::Unit,
    }
}
