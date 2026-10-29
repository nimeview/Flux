//! Checked arithmetic on fixed-width values.

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

pub fn add_int(a: IntWidth, av: u64, b: IntWidth, bv: u64) -> Result<Value, OverflowError> {
    let wa = promote_int(a, b);
    let ai = int_to_i64(
        wa,
        if wa == a {
            av
        } else {
            sign_extend(av, a.bits()) as u64
        },
    );
    let bi = int_to_i64(
        wa,
        if wa == b {
            bv
        } else {
            sign_extend(bv, b.bits()) as u64
        },
    );
    int_from_i64(wa, ai.checked_add(bi).ok_or(OverflowError::Overflow)?)
}

pub fn sub_int(a: IntWidth, av: u64, b: IntWidth, bv: u64) -> Result<Value, OverflowError> {
    let wa = promote_int(a, b);
    let ai = int_to_i64(wa, av);
    let bi = int_to_i64(wa, bv);
    int_from_i64(wa, ai.checked_sub(bi).ok_or(OverflowError::Overflow)?)
}

pub fn mul_int(a: IntWidth, av: u64, b: IntWidth, bv: u64) -> Result<Value, OverflowError> {
    let wa = promote_int(a, b);
    let ai = int_to_i64(wa, av);
    let bi = int_to_i64(wa, bv);
    int_from_i64(wa, ai.checked_mul(bi).ok_or(OverflowError::Overflow)?)
}

pub fn div_int(a: IntWidth, av: u64, b: IntWidth, bv: u64) -> Result<Value, OverflowError> {
    let wa = promote_int(a, b);
    let ai = int_to_i64(wa, av);
    let bi = int_to_i64(wa, bv);
    if bi == 0 {
        return Err(OverflowError::DivByZero);
    }
    int_from_i64(wa, ai / bi)
}

pub fn promote_int(a: IntWidth, b: IntWidth) -> IntWidth {
    use IntWidth::*;
    let rank = |w: IntWidth| -> u8 {
        match w {
            I8 => 1,
            U8 => 2,
            I16 => 3,
            U16 => 4,
            I32 => 5,
            U32 => 6,
            I64 => 7,
            U64 => 8,
        }
    };
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
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

/// Cast a runtime value to the named Flux type (`Int32`, `Float64`, `Char`, …).
pub fn cast_value(v: &Value, ty: &str) -> Value {
    use FloatWidth::*;
    use IntWidth::*;

    match ty {
        "Int" | "Int64" => match v {
            Value::Int { width, bits } => {
                int_from_i64(I64, int_to_i64(*width, *bits)).unwrap_or(Value::Unit)
            }
            Value::Float { width, bits } => {
                int_from_i64(I64, float_to_f64(*width, *bits) as i64).unwrap_or(Value::Unit)
            }
            Value::Char(c) => int_from_i64(I64, *c as i64).unwrap_or(Value::Unit),
            Value::Bool(b) => int_from_i64(I64, if *b { 1 } else { 0 }).unwrap_or(Value::Unit),
            _ => Value::Unit,
        },
        "Int8" => cast_to_int(v, I8),
        "Int16" => cast_to_int(v, I16),
        "Int32" => cast_to_int(v, I32),
        "UInt8" => cast_to_int(v, U8),
        "UInt16" => cast_to_int(v, U16),
        "UInt32" => cast_to_int(v, U32),
        "UInt64" => cast_to_int(v, U64),
        "Float" | "Float64" => cast_to_float(v, F64),
        "Float32" => cast_to_float(v, F32),
        "Char" => match v {
            Value::Int { width, bits } => Value::Char(int_to_i64(*width, *bits) as u32),
            Value::Char(c) => Value::Char(*c),
            _ => Value::Unit,
        },
        "Bool" => Value::Bool(
            v.as_bool()
                .unwrap_or_else(|| v.as_int().map(|n| n != 0).unwrap_or(false)),
        ),
        _ => v.clone(),
    }
}

fn cast_to_int(v: &Value, target: IntWidth) -> Value {
    let n = match v {
        Value::Int { width, bits } => int_to_i64(*width, *bits),
        Value::Float { width, bits } => float_to_f64(*width, *bits) as i64,
        Value::Char(c) => *c as i64,
        Value::Bool(b) => i64::from(*b),
        _ => return Value::Unit,
    };
    int_from_i64(target, n).unwrap_or(Value::Unit)
}

fn cast_to_float(v: &Value, target: FloatWidth) -> Value {
    let f = match v {
        Value::Float { width, bits } => float_to_f64(*width, *bits),
        Value::Int { width, bits } => int_to_i64(*width, *bits) as f64,
        Value::Char(c) => *c as f64,
        Value::Bool(b) => f64::from(*b),
        _ => return Value::Unit,
    };
    float_from_f64(target, f)
}
