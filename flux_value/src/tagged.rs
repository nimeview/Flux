//! Compact tagged representation for JIT slots and the host boundary.

/// Tag conventions:
///   0 = unit, 1 = int, 2 = float, 3 = bool, 4 = array, 5 = string, 6 = user enum
pub const TAG_UNIT: u64 = 0;
pub const TAG_INT: u64 = 1;
pub const TAG_FLOAT: u64 = 2;
pub const TAG_BOOL: u64 = 3;
pub const TAG_ARRAY: u64 = 4;
pub const TAG_STRING: u64 = 5;
pub const TAG_ENUM: u64 = 6;
pub const TAG_CHAR: u64 = 7;

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
