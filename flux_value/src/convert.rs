//! Conversions between rich [`Value`] and compact [`TaggedVal`].

use crate::heap::{get_array, get_enum, get_string, intern_string, store_enum};
use crate::ops::{float_to_f64, int_to_i64};
use crate::width::{FloatWidth, IntWidth};
use crate::{
    TaggedVal, Value, TAG_ARRAY, TAG_BOOL, TAG_CHAR, TAG_ENUM, TAG_FLOAT, TAG_INT, TAG_STRING,
};

/// Encode a runtime value into the JIT slot representation.
pub fn value_to_tagged(v: &Value) -> TaggedVal {
    match v {
        Value::Int { width, bits } => {
            let _ = width;
            TaggedVal::from_int(int_to_i64(*width, *bits))
        }
        Value::Float { width, bits } => TaggedVal::from_float(float_to_f64(*width, *bits)),
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
        Value::UserEnum {
            type_name,
            variant,
            payload,
        } => {
            let p = payload.as_ref().map(|inner| value_to_tagged(inner));
            TaggedVal::from_enum_id(store_enum(type_name, variant, p))
        }
        Value::Unit => TaggedVal::UNIT,
        Value::UserStruct { .. } | Value::Option(_) | Value::Result(_) => TaggedVal::UNIT,
    }
}

/// Decode a tagged slot value into a runtime value.
pub fn tagged_to_value(tv: TaggedVal) -> Value {
    match tv.tag {
        TAG_INT => Value::Int {
            width: IntWidth::I64,
            bits: tv.data,
        },
        TAG_FLOAT => Value::Float {
            width: FloatWidth::F64,
            bits: tv.data,
        },
        TAG_CHAR => Value::Char(tv.data as u32),
        TAG_BOOL => Value::Bool(tv.data != 0),
        TAG_ARRAY => get_array(tv.data)
            .map(|elems| Value::Array(elems.into_iter().map(tagged_to_value).collect()))
            .unwrap_or(Value::Unit),
        TAG_STRING => get_string(tv.data)
            .map(Value::String)
            .unwrap_or(Value::Unit),
        TAG_ENUM => get_enum(tv.data)
            .map(|(type_name, variant, payload)| Value::UserEnum {
                type_name,
                variant,
                payload: payload.map(|p| Box::new(tagged_to_value(p))),
            })
            .unwrap_or(Value::Unit),
        _ => Value::Unit,
    }
}
