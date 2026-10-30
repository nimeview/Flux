//! Compact tagged representation for JIT slots and the host boundary.

use crate::width::{FloatWidth, IntWidth};

/// Tag conventions (low byte = kind):
///   0 = unit, 1 = int, 2 = float, 3 = bool, 4 = array, 5 = string, 6 = user enum,
///   7 = char, 0xFF = runtime error (data = [`crate::ArithError`] code)
///
/// Ints and floats carry their width in bits 8..16. `Int64` and `Float64` use
/// width code 0, so their tags are exactly [`TAG_INT`] / [`TAG_FLOAT`]; JIT fast
/// paths only match those and send every other width to the shared slow path.
pub const TAG_UNIT: u64 = 0;
pub const TAG_INT: u64 = 1;
pub const TAG_FLOAT: u64 = 2;
pub const TAG_BOOL: u64 = 3;
pub const TAG_ARRAY: u64 = 4;
pub const TAG_STRING: u64 = 5;
pub const TAG_ENUM: u64 = 6;
pub const TAG_CHAR: u64 = 7;
pub const TAG_ERROR: u64 = 0xFF;

const WIDTH_SHIFT: u64 = 8;

pub fn int_tag(width: IntWidth) -> u64 {
    let code = match width {
        IntWidth::I64 => 0,
        IntWidth::I8 => 1,
        IntWidth::I16 => 2,
        IntWidth::I32 => 3,
        IntWidth::U8 => 4,
        IntWidth::U16 => 5,
        IntWidth::U32 => 6,
        IntWidth::U64 => 7,
    };
    TAG_INT | (code << WIDTH_SHIFT)
}

pub fn float_tag(width: FloatWidth) -> u64 {
    let code = match width {
        FloatWidth::F64 => 0,
        FloatWidth::F32 => 1,
    };
    TAG_FLOAT | (code << WIDTH_SHIFT)
}

/// Kind byte of a tag, without width bits.
pub fn tag_kind(tag: u64) -> u64 {
    tag & 0xFF
}

pub fn tag_int_width(tag: u64) -> Option<IntWidth> {
    if tag_kind(tag) != TAG_INT {
        return None;
    }
    Some(match tag >> WIDTH_SHIFT {
        0 => IntWidth::I64,
        1 => IntWidth::I8,
        2 => IntWidth::I16,
        3 => IntWidth::I32,
        4 => IntWidth::U8,
        5 => IntWidth::U16,
        6 => IntWidth::U32,
        7 => IntWidth::U64,
        _ => return None,
    })
}

pub fn tag_float_width(tag: u64) -> Option<FloatWidth> {
    if tag_kind(tag) != TAG_FLOAT {
        return None;
    }
    Some(match tag >> WIDTH_SHIFT {
        0 => FloatWidth::F64,
        1 => FloatWidth::F32,
        _ => return None,
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaggedVal {
    pub tag: u64,
    pub data: u64,
}

impl TaggedVal {
    pub const UNIT: Self = Self {
        tag: TAG_UNIT,
        data: 0,
    };

    pub fn from_int(n: i64) -> Self {
        Self {
            tag: TAG_INT,
            data: n as u64,
        }
    }

    pub fn from_float(f: f64) -> Self {
        Self {
            tag: TAG_FLOAT,
            data: f.to_bits(),
        }
    }

    pub fn error(err: crate::ArithError) -> Self {
        Self {
            tag: TAG_ERROR,
            data: err as u64,
        }
    }

    pub fn is_error(&self) -> bool {
        self.tag == TAG_ERROR
    }

    pub fn from_bool(b: bool) -> Self {
        Self {
            tag: TAG_BOOL,
            data: b as u64,
        }
    }

    pub fn from_string_id(id: u64) -> Self {
        Self {
            tag: TAG_STRING,
            data: id,
        }
    }

    pub fn from_array_id(id: u64) -> Self {
        Self {
            tag: TAG_ARRAY,
            data: id,
        }
    }

    pub fn from_enum_id(id: u64) -> Self {
        Self {
            tag: TAG_ENUM,
            data: id,
        }
    }
}
