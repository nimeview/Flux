//! Language-level numeric semantics shared by the VM and the JIT slow path.
//!
//! Both engines call into this module for every case outside the JIT's
//! `Int64`/`Float64` fast path, so they cannot disagree on edge cases:
//!
//! - Integer `+ - * / %` compute exactly and fail with [`ArithError::Overflow`]
//!   when the result does not fit the promoted width ([`promote_int`]).
//! - Integer `/` truncates toward zero; `%` takes the sign of the dividend.
//!   A zero divisor fails with [`ArithError::DivByZero`].
//! - Any float operand makes the operation IEEE 754 (`x / 0.0` is `inf`/`NaN`);
//!   the result is `Float32` only when no operand is `Float64`.
//! - Integers compare exactly; comparisons involving a float use IEEE rules.
//! - Numeric casts fail with [`ArithError::CastOutOfRange`] instead of wrapping.

use std::cmp::Ordering;

use crate::ops::{float_from_f64, float_to_f64, int_to_i64};
use crate::width::{FloatWidth, IntWidth};
use crate::Value;

/// Binary arithmetic operator. The discriminant is part of the JIT host ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NumOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
    Rem = 4,
}

impl NumOp {
    pub fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::Div,
            4 => Self::Rem,
            _ => return None,
        })
    }
}

/// A language-level runtime error raised by arithmetic or casts.
/// The discriminant is carried in JIT error values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArithError {
    DivByZero = 1,
    Overflow = 2,
    CastOutOfRange = 3,
    InvalidCast = 4,
    /// Operand kinds the type checker should have rejected.
    InvalidOperands = 5,
}

impl ArithError {
    pub fn from_code(code: u64) -> Self {
        match code {
            1 => Self::DivByZero,
            2 => Self::Overflow,
            3 => Self::CastOutOfRange,
            4 => Self::InvalidCast,
            _ => Self::InvalidOperands,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::DivByZero => "integer division by zero",
            Self::Overflow => "integer overflow",
            Self::CastOutOfRange => "numeric cast out of range",
            Self::InvalidCast => "invalid cast",
            Self::InvalidOperands => "invalid operands for arithmetic",
        }
    }
}

impl std::fmt::Display for ArithError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ArithError {}

/// Result width of an integer operation on mixed widths: the higher-ranked operand.
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

pub fn int_to_i128(width: IntWidth, bits: u64) -> i128 {
    if width.is_signed() {
        int_to_i64(width, bits) as i128
    } else {
        bits as i128
    }
}

fn int_range(width: IntWidth) -> (i128, i128) {
    let bits = width.bits() as u32;
    if width.is_signed() {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// Build an integer of `width` from an exact value, or `None` if it does not fit.
pub fn int_from_i128(width: IntWidth, n: i128) -> Option<Value> {
    let (min, max) = int_range(width);
    if n < min || n > max {
        return None;
    }
    let mask = if width.bits() == 64 {
        u64::MAX
    } else {
        (1u64 << width.bits()) - 1
    };
    Some(Value::Int {
        width,
        bits: (n as u64) & mask,
    })
}

enum Num {
    Int(IntWidth, i128),
    Float(FloatWidth, f64),
}

fn num(v: &Value) -> Option<Num> {
    match v {
        Value::Int { width, bits } => Some(Num::Int(*width, int_to_i128(*width, *bits))),
        Value::Float { width, bits } => Some(Num::Float(*width, float_to_f64(*width, *bits))),
        _ => None,
    }
}

fn float_parts(n: &Num) -> (Option<FloatWidth>, f64) {
    match n {
        Num::Int(_, i) => (None, *i as f64),
        Num::Float(w, f) => (Some(*w), *f),
    }
}

fn float_result_width(a: Option<FloatWidth>, b: Option<FloatWidth>) -> FloatWidth {
    match (a, b) {
        (Some(FloatWidth::F64), _) | (_, Some(FloatWidth::F64)) => FloatWidth::F64,
        (Some(FloatWidth::F32), _) | (_, Some(FloatWidth::F32)) => FloatWidth::F32,
        (None, None) => FloatWidth::F64,
    }
}

/// Apply a binary arithmetic operator to two numeric values.
pub fn binary(op: NumOp, a: &Value, b: &Value) -> Result<Value, ArithError> {
    let (na, nb) = match (num(a), num(b)) {
        (Some(na), Some(nb)) => (na, nb),
        _ => return Err(ArithError::InvalidOperands),
    };
    if let (Num::Int(wa, x), Num::Int(wb, y)) = (&na, &nb) {
        let width = promote_int(*wa, *wb);
        let exact = match op {
            NumOp::Add => x + y,
            NumOp::Sub => x - y,
            NumOp::Mul => x.checked_mul(*y).ok_or(ArithError::Overflow)?,
            NumOp::Div | NumOp::Rem if *y == 0 => return Err(ArithError::DivByZero),
            NumOp::Div => x / y,
            NumOp::Rem => x % y,
        };
        return int_from_i128(width, exact).ok_or(ArithError::Overflow);
    }
    let (wa, x) = float_parts(&na);
    let (wb, y) = float_parts(&nb);
    let r = match op {
        NumOp::Add => x + y,
        NumOp::Sub => x - y,
        NumOp::Mul => x * y,
        NumOp::Div => x / y,
        NumOp::Rem => x % y,
    };
    Ok(float_from_f64(float_result_width(wa, wb), r))
}

/// Unary minus.
pub fn negate(v: &Value) -> Result<Value, ArithError> {
    match num(v) {
        Some(Num::Int(w, n)) => int_from_i128(w, -n).ok_or(ArithError::Overflow),
        Some(Num::Float(w, f)) => Ok(float_from_f64(w, -f)),
        None => Err(ArithError::InvalidOperands),
    }
}

/// Numeric ordering; `None` when either side is NaN or not numeric.
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (num(a)?, num(b)?) {
        (Num::Int(_, x), Num::Int(_, y)) => Some(x.cmp(&y)),
        (na, nb) => float_parts(&na).1.partial_cmp(&float_parts(&nb).1),
    }
}

/// Language equality (`==`): numeric values compare by value across widths,
/// floats follow IEEE (`NaN != NaN`, `0.0 == -0.0`), everything else structurally.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (num(a), num(b)) {
        (Some(_), Some(_)) => compare(a, b) == Some(Ordering::Equal),
        _ => a == b,
    }
}

/// Cast a runtime value to the named Flux type (`Int32`, `Float64`, `Char`, …).
pub fn cast(v: &Value, ty: &str) -> Result<Value, ArithError> {
    if let Some(width) = IntWidth::from_name(ty) {
        let n: i128 = match v {
            Value::Int { width, bits } => int_to_i128(*width, *bits),
            Value::Float { width, bits } => {
                let f = float_to_f64(*width, *bits).trunc();
                // 1.7e38 < 2^127 bounds every Flux integer width, so this keeps
                // the `as i128` conversion exact; NaN and inf are rejected here.
                if f.is_nan() || f.abs() >= 1.7e38 {
                    return Err(ArithError::CastOutOfRange);
                }
                f as i128
            }
            Value::Char(c) => *c as i128,
            Value::Bool(b) => *b as i128,
            _ => return Err(ArithError::InvalidCast),
        };
        return int_from_i128(width, n).ok_or(ArithError::CastOutOfRange);
    }
    if let Some(width) = FloatWidth::from_name(ty) {
        let f = match v {
            Value::Int { width, bits } => int_to_i128(*width, *bits) as f64,
            Value::Float { width, bits } => float_to_f64(*width, *bits),
            Value::Char(c) => *c as f64,
            Value::Bool(b) => f64::from(*b),
            _ => return Err(ArithError::InvalidCast),
        };
        return Ok(float_from_f64(width, f));
    }
    match ty {
        "Char" => match v {
            Value::Int { width, bits } => u32::try_from(int_to_i128(*width, *bits))
                .ok()
                .and_then(char::from_u32)
                .map(|c| Value::Char(c as u32))
                .ok_or(ArithError::CastOutOfRange),
            Value::Char(c) => Ok(Value::Char(*c)),
            _ => Err(ArithError::InvalidCast),
        },
        "Bool" => match v {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            Value::Int { width, bits } => Ok(Value::Bool(int_to_i128(*width, *bits) != 0)),
            _ => Err(ArithError::InvalidCast),
        },
        "String" if matches!(v, Value::String(_)) => Ok(v.clone()),
        "Unit" if matches!(v, Value::Unit) => Ok(v.clone()),
        _ => Err(ArithError::InvalidCast),
    }
}
