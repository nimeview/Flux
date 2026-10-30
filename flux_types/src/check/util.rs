//! Small, dependency-free type utilities shared across the checker.

use std::collections::HashMap;

use flux_hir::SymbolId;

use crate::types::Type;

pub(crate) fn lookup_name(names: &HashMap<SymbolId, String>, id: SymbolId) -> Option<&String> {
    names.get(&id)
}

pub(crate) fn unwrap_value_type(ty: &Type) -> Type {
    match ty {
        Type::Cell(inner) | Type::Read(inner) => *inner.clone(),
        other => other.clone(),
    }
}

/// Normalize primitive aliases for trait impl lookup (`Int64` → `Int`, etc.).
pub(crate) fn trait_dispatch_tag(ty: &Type) -> String {
    match unwrap_value_type(ty) {
        Type::Int | Type::Int64 => "Int".into(),
        Type::Float | Type::Float64 => "Float".into(),
        other => other.display(),
    }
}

fn int_rank(ty: &Type) -> u8 {
    match ty {
        Type::Int8 => 1,
        Type::UInt8 => 2,
        Type::Int16 => 3,
        Type::UInt16 => 4,
        Type::Int32 => 5,
        Type::UInt32 => 6,
        Type::Int | Type::Int64 => 7,
        Type::UInt64 => 8,
        _ => 0,
    }
}

fn type_from_int_rank(rank: u8) -> Type {
    match rank {
        1 => Type::Int8,
        2 => Type::UInt8,
        3 => Type::Int16,
        4 => Type::UInt16,
        5 => Type::Int32,
        6 => Type::UInt32,
        8 => Type::UInt64,
        _ => Type::Int64,
    }
}

pub(super) fn numeric_type(ty: &Type) -> Option<Type> {
    let ty = unwrap_value_type(ty);
    if ty.is_integer() || ty.is_float() {
        Some(ty)
    } else {
        None
    }
}

pub(crate) fn numeric_compatible(a: &Type, b: &Type) -> bool {
    let a = unwrap_value_type(a);
    let b = unwrap_value_type(b);
    (a.is_integer() && b.is_integer())
        || (a.is_float() && b.is_float())
        || (a.is_integer() && b.is_float())
        || (a.is_float() && b.is_integer())
}

pub(crate) fn widen_numeric(a: &Type, b: &Type) -> Type {
    let a = unwrap_value_type(a);
    let b = unwrap_value_type(b);
    if matches!(a, Type::Float | Type::Float64) || matches!(b, Type::Float | Type::Float64) {
        Type::Float64
    } else if matches!(a, Type::Float32) || matches!(b, Type::Float32) {
        Type::Float32
    } else {
        type_from_int_rank(int_rank(&a).max(int_rank(&b)))
    }
}

pub(crate) fn types_compatible(expected: &Type, found: &Type) -> bool {
    if expected.is_error() || found.is_error() {
        return true;
    }
    if expected == found {
        return true;
    }
    let expected = unwrap_value_type(expected);
    let found = unwrap_value_type(found);
    if expected == found {
        return true;
    }
    match (&expected, &found) {
        (Type::Int, Type::Int64) | (Type::Int64, Type::Int) => true,
        (Type::Float, Type::Float64) | (Type::Float64, Type::Float) => true,
        // Compatibility is used for assignments, arguments, and returns. The
        // runtime does not insert implicit casts, so accepting a numeric type
        // conversion here would make the checked type differ from the value.
        // Mixed numeric *operations* use `numeric_compatible` and widen their
        // result explicitly; value boundaries require an explicit `as` cast.
        (Type::Cell(e), Type::Cell(f)) | (Type::Read(e), Type::Read(f)) => types_compatible(e, f),
        (Type::Option(e), Type::Option(f)) => types_compatible(e, f),
        (
            Type::Result {
                ok: e_ok,
                err: e_err,
            },
            Type::Result {
                ok: f_ok,
                err: f_err,
            },
        ) => types_compatible(e_ok, f_ok) && types_compatible(e_err, f_err),
        (Type::Cell(e), f) | (Type::Read(e), f) if types_compatible(e, f) => true,
        (e, Type::Cell(f)) | (e, Type::Read(f)) if types_compatible(e, f) => true,
        _ => false,
    }
}
