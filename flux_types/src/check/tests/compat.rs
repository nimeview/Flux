//! Tests for [`super::super::util`] type compatibility helpers.

use crate::types::Type;

use crate::check::util::{numeric_compatible, types_compatible, unwrap_value_type, widen_numeric};

#[test]
fn int_compatible_with_int() {
    assert!(types_compatible(&Type::Int, &Type::Int));
}

#[test]
fn int_compatible_with_float() {
    assert!(types_compatible(&Type::Int, &Type::Float));
    assert!(types_compatible(&Type::Float, &Type::Int));
}

#[test]
fn bool_not_compatible_with_int() {
    assert!(!types_compatible(&Type::Bool, &Type::Int));
}

#[test]
fn cell_inner_compatibility() {
    assert!(types_compatible(
        &Type::Cell(Box::new(Type::Int)),
        &Type::Cell(Box::new(Type::Int))
    ));
}

#[test]
fn cell_compatible_with_plain_inner() {
    assert!(types_compatible(
        &Type::Cell(Box::new(Type::Int)),
        &Type::Int
    ));
}

#[test]
fn read_compatible_with_plain_inner() {
    assert!(types_compatible(
        &Type::Read(Box::new(Type::Float)),
        &Type::Float
    ));
}

#[test]
fn option_nested_compatibility() {
    assert!(types_compatible(
        &Type::Option(Box::new(Type::Int)),
        &Type::Option(Box::new(Type::Int))
    ));
    assert!(!types_compatible(
        &Type::Option(Box::new(Type::Int)),
        &Type::Option(Box::new(Type::String))
    ));
}

#[test]
fn result_compatibility() {
    let a = Type::Result {
        ok: Box::new(Type::Int),
        err: Box::new(Type::String),
    };
    let b = Type::Result {
        ok: Box::new(Type::Int),
        err: Box::new(Type::String),
    };
    assert!(types_compatible(&a, &b));
}

#[test]
fn error_type_is_compatible_with_anything() {
    assert!(types_compatible(&Type::Error, &Type::Int));
    assert!(types_compatible(&Type::Bool, &Type::Error));
}

#[test]
fn numeric_compatible_pairs() {
    assert!(numeric_compatible(&Type::Int, &Type::Int));
    assert!(numeric_compatible(&Type::Float, &Type::Int));
    assert!(!numeric_compatible(&Type::Bool, &Type::Int));
}

#[test]
fn widen_numeric_promotes_to_float() {
    assert_eq!(widen_numeric(&Type::Int, &Type::Float), Type::Float64);
    assert_eq!(widen_numeric(&Type::Int, &Type::Int), Type::Int64);
}

#[test]
fn unwrap_value_type_strips_cell() {
    assert_eq!(
        unwrap_value_type(&Type::Cell(Box::new(Type::String))),
        Type::String
    );
}

#[test]
fn unwrap_value_type_strips_read() {
    assert_eq!(
        unwrap_value_type(&Type::Read(Box::new(Type::Bool))),
        Type::Bool
    );
}

#[test]
fn unwrap_value_type_plain() {
    assert_eq!(unwrap_value_type(&Type::Int), Type::Int);
}
