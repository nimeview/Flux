//! Fixed-width integer and float metadata.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntWidth {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

impl IntWidth {
    pub fn is_signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    pub fn bits(self) -> u8 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 => 64,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Int8" => Some(Self::I8),
            "Int16" => Some(Self::I16),
            "Int32" => Some(Self::I32),
            "Int64" | "Int" => Some(Self::I64),
            "UInt8" => Some(Self::U8),
            "UInt16" => Some(Self::U16),
            "UInt32" => Some(Self::U32),
            "UInt64" => Some(Self::U64),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::I8 => "Int8",
            Self::I16 => "Int16",
            Self::I32 => "Int32",
            Self::I64 => "Int64",
            Self::U8 => "UInt8",
            Self::U16 => "UInt16",
            Self::U32 => "UInt32",
            Self::U64 => "UInt64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatWidth {
    F32,
    F64,
}

impl FloatWidth {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Float32" => Some(Self::F32),
            "Float64" | "Float" => Some(Self::F64),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::F32 => "Float32",
            Self::F64 => "Float64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowError {
    Overflow,
    Underflow,
    DivByZero,
}
