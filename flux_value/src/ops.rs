//! Fixed-width value encoding helpers. Arithmetic semantics live in `arith`.

use crate::width::{FloatWidth, IntWidth, OverflowError};
use crate::Value;

fn sign_extend(bits: u64, width: u8) -> i64 {
    let shift = 64 - width as i32;
    ((bits << shift) as i64) >> shift
}

fn truncate_i64(n: i64, width: IntWidth) -> Result<u64, OverflowError> {
    let bits = width.bits();
    let mask = if width.is_signed() {
        (1u64 << (bits - 1)) - 1
    } else {
        (1u64 << bits) - 1
    };
    if width.is_signed() {
        let min = if bits == 64 {
            i64::MIN
        } else {
            -(1i64 << (bits - 1))
        };
        let max = if bits == 64 { i64::MAX } else { mask as i64 };
        if n < min || n > max {
            return Err(OverflowError::Overflow);
        }
        Ok(if bits == 64 {
            n as u64
        } else {
            n as u64 & ((1u64 << bits) - 1)
        })
    } else if n < 0 {
        Err(OverflowError::Underflow)
    } else if n as u64 > mask {
        Err(OverflowError::Overflow)
    } else {
        Ok(n as u64)
    }
}

pub fn int_value(width: IntWidth, bits: u64) -> Value {
    Value::Int { width, bits }
}

pub fn float_value(width: FloatWidth, bits: u64) -> Value {
    Value::Float { width, bits }
}

pub fn int_to_i64(width: IntWidth, bits: u64) -> i64 {
    if width.is_signed() {
        sign_extend(bits, width.bits())
    } else {
        bits as i64
    }
}

pub fn int_from_i64(width: IntWidth, n: i64) -> Result<Value, OverflowError> {
    let bits = truncate_i64(n, width)?;
    Ok(int_value(width, bits))
}

pub fn float_to_f64(width: FloatWidth, bits: u64) -> f64 {
    match width {
        FloatWidth::F32 => f32::from_bits(bits as u32) as f64,
        FloatWidth::F64 => f64::from_bits(bits),
    }
}

pub fn float_from_f64(width: FloatWidth, f: f64) -> Value {
    match width {
        FloatWidth::F32 => float_value(width, (f as f32).to_bits() as u64),
        FloatWidth::F64 => float_value(width, f.to_bits()),
    }
}
