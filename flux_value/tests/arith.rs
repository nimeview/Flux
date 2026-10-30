use std::cmp::Ordering;

use flux_value::{
    binary, cast, compare, int_from_i128, negate, tagged_to_value, value_to_tagged, values_equal,
    ArithError, FloatWidth, IntWidth, NumOp, Value,
};

fn int(width: IntWidth, n: i128) -> Value {
    int_from_i128(width, n).expect("in range")
}

fn f32v(f: f32) -> Value {
    Value::Float {
        width: FloatWidth::F32,
        bits: f.to_bits() as u64,
    }
}

#[test]
fn uint64_above_i64_max_is_exact() {
    let big = int(IntWidth::U64, u64::MAX as i128 - 1);
    let one = int(IntWidth::U64, 1);
    assert_eq!(
        binary(NumOp::Add, &big, &one),
        Ok(int(IntWidth::U64, u64::MAX as i128))
    );
    assert_eq!(
        binary(NumOp::Add, &int(IntWidth::U64, u64::MAX as i128), &one),
        Err(ArithError::Overflow)
    );
    assert_eq!(compare(&big, &one), Some(Ordering::Greater));
}

#[test]
fn unsigned_underflow_is_an_error() {
    let zero = int(IntWidth::U8, 0);
    let one = int(IntWidth::U8, 1);
    assert_eq!(binary(NumOp::Sub, &zero, &one), Err(ArithError::Overflow));
    assert_eq!(negate(&one), Err(ArithError::Overflow));
    assert_eq!(negate(&zero), Ok(zero));
}

#[test]
fn mixed_widths_promote_to_higher_rank() {
    let a = int(IntWidth::I8, 100);
    let b = int(IntWidth::I32, 100);
    assert_eq!(binary(NumOp::Add, &a, &b), Ok(int(IntWidth::I32, 200)));
    assert_eq!(
        binary(NumOp::Add, &a, &int(IntWidth::I8, 100)),
        Err(ArithError::Overflow)
    );
}

#[test]
fn float32_stays_float32_unless_mixed_with_float64() {
    let a = f32v(1.5);
    assert_eq!(binary(NumOp::Mul, &a, &f32v(2.0)), Ok(f32v(3.0)));
    assert_eq!(
        binary(NumOp::Add, &a, &int(IntWidth::I64, 1)),
        Ok(f32v(2.5))
    );
    assert_eq!(
        binary(NumOp::Add, &a, &Value::float_f64(1.0)),
        Ok(Value::float_f64(2.5))
    );
}

#[test]
fn equality_is_numeric_across_widths_and_ieee_for_floats() {
    assert!(values_equal(&int(IntWidth::I8, 7), &Value::int_i64(7)));
    assert!(values_equal(
        &Value::float_f64(0.0),
        &Value::float_f64(-0.0)
    ));
    let nan = Value::float_f64(f64::NAN);
    assert!(!values_equal(&nan, &nan));
    assert_eq!(compare(&nan, &Value::float_f64(1.0)), None);
}

#[test]
fn casts_are_checked() {
    assert_eq!(
        cast(&Value::int_i64(255), "UInt8"),
        Ok(int(IntWidth::U8, 255))
    );
    assert_eq!(
        cast(&Value::int_i64(256), "UInt8"),
        Err(ArithError::CastOutOfRange)
    );
    assert_eq!(
        cast(&Value::float_f64(f64::INFINITY), "Int"),
        Err(ArithError::CastOutOfRange)
    );
    assert_eq!(
        cast(&Value::float_f64(9.3e18), "Int"),
        Err(ArithError::CastOutOfRange)
    );
    assert_eq!(cast(&Value::int_i64(0x41), "Char"), Ok(Value::Char(0x41)));
    assert_eq!(
        cast(&Value::int_i64(0xD800), "Char"),
        Err(ArithError::CastOutOfRange)
    );
    assert_eq!(
        cast(&Value::String("1".into()), "Int"),
        Err(ArithError::InvalidCast)
    );
}

#[test]
fn tagged_round_trip_keeps_width() {
    for v in [
        int(IntWidth::I8, -5),
        int(IntWidth::U32, u32::MAX as i128),
        int(IntWidth::U64, u64::MAX as i128),
        Value::int_i64(i64::MIN),
        f32v(1.25),
        Value::float_f64(-0.5),
    ] {
        assert_eq!(tagged_to_value(value_to_tagged(&v)), v);
    }
}
