mod arith;
mod convert;
mod display;
mod heap;
mod ops;
mod tagged;
mod width;

pub use arith::{
    binary, cast, compare, int_from_i128, int_to_i128, negate, promote_int, values_equal,
    ArithError, NumOp,
};
pub use convert::{tagged_to_value, value_to_tagged};
pub use display::render;
pub use heap::{
    array_finish, array_get, array_len, array_push, clear_for_tests, get_array, get_enum,
    get_string, heap_scope, heap_stats, int_range, intern_string, make_int_range_array, store_enum,
    HeapScope, HeapStats, MAX_COLLECTION_LEN,
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
    Array(Vec<Value>),
    UserStruct {
        type_name: String,
        fields: HashMap<String, Value>,
    },
    UserEnum {
        type_name: String,
        variant: String,
        payload: Option<Box<Value>>,
    },
    Option(Option<Box<Value>>),
    Result(Result<Box<Value>, Box<Value>>),
    Unit,
}

impl Value {
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
            Self::UserStruct { .. } => "Struct",
            Self::UserEnum { .. } => "Enum",
            Self::Option(_) => "Option",
            Self::Result(_) => "Result",
            Self::Unit => "Unit",
        }
    }

    /// Human-readable representation for print/display builtins and CLI output.
    pub fn display_string(&self) -> String {
        render(self)
    }
}
