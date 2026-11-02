mod arith;
mod convert;
mod display;
mod heap;
mod ops;
mod tagged;
mod width;

pub use arith::{
    binary, binary_i64, cast, compare, int_from_i128, int_to_i128, negate, promote_int,
    values_equal, ArithError, NumOp,
};
pub use convert::{is_tag_representable, tagged_to_value, value_to_tagged};
pub use display::render;
pub use heap::{
    array_finish, array_get, array_len, array_push, clear_for_tests, concat_strings, enum_payload,
    enum_variant_is, get_array, get_enum, get_string, heap_scope, heap_stats, int_range,
    intern_string, make_int_range_array, store_enum, HeapScope, HeapStats, MAX_CALL_DEPTH,
    MAX_COLLECTION_LEN, MAX_STRING_LEN,
};
pub use ops::{float_from_f64, float_to_f64, int_from_i64, int_to_i64};
pub use tagged::{
    float_tag, int_tag, tag_float_width, tag_int_width, tag_kind, TaggedVal, TAG_ARRAY, TAG_BOOL,
    TAG_CHAR, TAG_ENUM, TAG_ERROR, TAG_FLOAT, TAG_INT, TAG_STRING, TAG_UNIT,
};
pub use width::{FloatWidth, IntWidth, OverflowError};

use std::collections::HashMap;

/// Canonical runtime value representation shared across VM, stdlib, and CLI output.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int {
        width: IntWidth,
        bits: u64,
    },
    Float {
        width: FloatWidth,
        bits: u64,
    },
    Char(u32),
    Bool(bool),
    String(String),
    /// Shared and copied on write: reading or passing an array is O(1);
    /// mutation (`Arc::make_mut`) copies only if another value still holds it.
    Array(std::sync::Arc<Vec<Value>>),
    /// Structs and enums are behind a pointer so that `Value` stays 32
    /// bytes (inline, they made it 72, and every stack push/pop a slow block
    /// copy). They are immutable once built, so copies share them, like arrays.
    UserStruct(std::sync::Arc<StructValue>),
    UserEnum(std::sync::Arc<EnumValue>),
    Option(Option<Box<Value>>),
    Result(Result<Box<Value>, Box<Value>>),
    /// `Map[K, V]`, ordered by key; each entry keeps the key's own value
    /// (with its integer width). Shared and copied on write, like arrays.
    Map(std::sync::Arc<std::collections::BTreeMap<MapKey, (Value, Value)>>),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructValue {
    pub type_name: String,
    pub fields: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    pub type_name: String,
    pub variant: String,
    pub payload: Option<Value>,
}

/// A map key: integers (any width, by value), `String`, `Bool`, or `Char`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapKey {
    Bool(bool),
    Int(i128),
    Char(u32),
    String(String),
}

impl MapKey {
    /// The key for `v`, or `None` for a value that cannot be a key.
    pub fn of(v: &Value) -> Option<MapKey> {
        match v {
            Value::Bool(b) => Some(MapKey::Bool(*b)),
            Value::Int { width, bits } => Some(MapKey::Int(int_to_i128(*width, *bits))),
            Value::Char(c) => Some(MapKey::Char(*c)),
            Value::String(s) => Some(MapKey::String(s.clone())),
            _ => None,
        }
    }
}

impl Value {
    pub fn user_struct(type_name: String, fields: HashMap<String, Value>) -> Self {
        Self::UserStruct(std::sync::Arc::new(StructValue { type_name, fields }))
    }

    pub fn user_enum(type_name: String, variant: String, payload: Option<Value>) -> Self {
        Self::UserEnum(std::sync::Arc::new(EnumValue {
            type_name,
            variant,
            payload,
        }))
    }

    pub fn array(items: Vec<Value>) -> Self {
        Self::Array(std::sync::Arc::new(items))
    }

    pub fn int_i64(n: i64) -> Self {
        Self::Int {
            width: IntWidth::I64,
            bits: n as u64,
        }
    }

    pub fn float_f64(f: f64) -> Self {
        Self::Float {
            width: FloatWidth::F64,
            bits: f.to_bits(),
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float { width, bits } => Some(float_to_f64(*width, *bits)),
            Self::Int { width, bits } => Some(int_to_i128(*width, *bits) as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int { width, bits } => i64::try_from(int_to_i128(*width, *bits)).ok(),
            Self::Float { width, bits } => Some(float_to_f64(*width, *bits) as i64),
            _ => None,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int { .. } | Self::Float { .. })
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Int { width, .. } => match width {
                IntWidth::I64 => "Int",
                other => {
                    let _ = other;
                    "Int"
                }
            },
            Self::Float { width, .. } => match width {
                FloatWidth::F64 => "Float",
                FloatWidth::F32 => "Float32",
            },
            Self::Char(_) => "Char",
            Self::Bool(_) => "Bool",
            Self::String(_) => "String",
            Self::Array(_) => "Array",
            Self::UserStruct(_) => "Struct",
            Self::UserEnum(_) => "Enum",
            Self::Option(_) => "Option",
            Self::Result(_) => "Result",
            Self::Map(_) => "Map",
            Self::Unit => "Unit",
        }
    }

    /// Human-readable representation for print/display builtins and CLI output.
    pub fn display_string(&self) -> String {
        render(self)
    }
}

#[cfg(test)]
mod size_tests {
    /// Every operand push/pop moves a `Value`; past 64 bytes the move
    /// became a block copy that dominated the VM loop.
    #[test]
    fn value_stays_small() {
        assert!(std::mem::size_of::<super::Value>() <= 32);
    }
}
