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
pub fn trait_dispatch_tag(ty: &Type) -> String {
    match unwrap_value_type(ty) {
        Type::Int | Type::Int64 => "Int".into(),
        Type::Float | Type::Float64 => "Float".into(),
        // Impls are indexed by the type's name, not its structural display.
        Type::Struct { name, .. } | Type::Enum { name, .. } => name,
        other => other.key(),
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

/// `a` with each unknown (`Empty`) part replaced by the matching part of `b`:
/// joining `Option<_>` with `Option<Int>` gives `Option<Int>`.
pub(crate) fn refine_unknown(a: &Type, b: &Type) -> Type {
    match (a, b) {
        (Type::Empty, other) => other.clone(),
        (Type::Array(x), Type::Array(y)) => Type::Array(Box::new(refine_unknown(x, y))),
        (Type::Option(x), Type::Option(y)) => Type::Option(Box::new(refine_unknown(x, y))),
        (Type::Tuple(xs), Type::Tuple(ys)) if xs.len() == ys.len() => Type::Tuple(
            xs.iter()
                .zip(ys)
                .map(|(x, y)| refine_unknown(x, y))
                .collect(),
        ),
        (Type::Map { key: ak, value: av }, Type::Map { key: bk, value: bv }) => Type::Map {
            key: Box::new(refine_unknown(ak, bk)),
            value: Box::new(refine_unknown(av, bv)),
        },
        (Type::Result { ok: ao, err: ae }, Type::Result { ok: bo, err: be }) => Type::Result {
            ok: Box::new(refine_unknown(ao, bo)),
            err: Box::new(refine_unknown(ae, be)),
        },
        // `[|| N, || S(1)]`: closures' unknown parts.
        (
            Type::Func {
                params: ap,
                ret: ar,
            },
            Type::Func {
                params: bp,
                ret: br,
            },
        ) if ap.len() == bp.len() => Type::Func {
            params: ap
                .iter()
                .zip(bp)
                .map(|(x, y)| refine_unknown(x, y))
                .collect(),
            ret: Box::new(refine_unknown(ar, br)),
        },
        // A generic type's unknown arguments (`Tree[_]` with `Tree[Int]`).
        (
            Type::Struct {
                name,
                args: aa,
                fields: af,
            },
            Type::Struct {
                name: bn,
                args: ba,
                fields: bf,
            },
        ) if name == bn && aa.len() == ba.len() && af.len() == bf.len() && mentions_empty(a) => {
            Type::Struct {
                name: name.clone(),
                args: aa
                    .iter()
                    .zip(ba)
                    .map(|(x, y)| refine_unknown(x, y))
                    .collect(),
                fields: af
                    .iter()
                    .zip(bf)
                    .map(|((n, x), (_, y))| (n.clone(), refine_unknown(x, y)))
                    .collect(),
            }
        }
        (
            Type::Enum {
                name,
                args: aa,
                variants: av,
            },
            Type::Enum {
                name: bn,
                args: ba,
                variants: bv,
            },
        ) if name == bn && aa.len() == ba.len() && av.len() == bv.len() && mentions_empty(a) => {
            Type::Enum {
                name: name.clone(),
                args: aa
                    .iter()
                    .zip(ba)
                    .map(|(x, y)| refine_unknown(x, y))
                    .collect(),
                variants: av
                    .iter()
                    .zip(bv)
                    .map(|((n, x), (_, y))| {
                        let p = match (x, y) {
                            (Some(x), Some(y)) => Some(refine_unknown(x, y)),
                            (x, _) => x.clone(),
                        };
                        (n.clone(), p)
                    })
                    .collect(),
            }
        }
        // One side is a placeholder: refine the arguments; `expand` gives
        // the rest.
        (
            Type::Struct { name, args: aa, .. } | Type::Enum { name, args: aa, .. },
            Type::Struct {
                name: bn, args: ba, ..
            }
            | Type::Enum {
                name: bn, args: ba, ..
            },
        ) if !name.is_empty() && name == bn && aa.len() == ba.len() && mentions_empty(a) => {
            Type::Enum {
                name: name.clone(),
                args: aa
                    .iter()
                    .zip(ba)
                    .map(|(x, y)| refine_unknown(x, y))
                    .collect(),
                variants: vec![],
            }
        }
        _ => a.clone(),
    }
}

/// Unknown `Option`/`Result` parts left in an unannotated binding take their
/// historical defaults (`Option<Unit>`, `Ok` side `Unit`, `Err` side `String`).
/// Array elements are not defaulted: a bare `[]` binding stays an error.
pub(crate) fn default_unknown_variants(ty: &Type) -> Type {
    let fill = |t: &Type, default: Type| match t {
        Type::Empty => default,
        other => default_unknown_variants(other),
    };
    match ty {
        Type::Option(x) => Type::Option(Box::new(fill(x, Type::Unit))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(fill(ok, Type::Unit)),
            err: Box::new(fill(err, Type::String)),
        },
        Type::Array(x) => Type::Array(Box::new(match x.as_ref() {
            Type::Empty => Type::Empty,
            other => default_unknown_variants(other),
        })),
        Type::Cell(x) => Type::Cell(Box::new(default_unknown_variants(x))),
        Type::Read(x) => Type::Read(Box::new(default_unknown_variants(x))),
        other => other.clone(),
    }
}

/// Whether `ty` still contains the element type of a bare `[]`.
pub(crate) fn mentions_empty(ty: &Type) -> bool {
    match ty {
        Type::Empty => true,
        Type::Array(e) | Type::Option(e) | Type::Cell(e) | Type::Read(e) => mentions_empty(e),
        Type::Result { ok, err } => mentions_empty(ok) || mentions_empty(err),
        Type::Map { key, value } => mentions_empty(key) || mentions_empty(value),
        Type::Tuple(elems) => elems.iter().any(mentions_empty),
        Type::Struct { args, .. } | Type::Enum { args, .. } if args.iter().any(mentions_empty) => {
            true
        }
        // A struct variant's fields.
        Type::Struct { name, fields, .. } if name.is_empty() => {
            fields.iter().any(|(_, t)| mentions_empty(t))
        }
        _ => false,
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
        // A named type is itself at any depth: a recursive type's expansion
        // ends in a placeholder (see `TypeCheckContext::expand`).
        // A generic type's arguments must agree; an unknown one fits.
        (
            Type::Struct {
                name: e, args: ea, ..
            }
            | Type::Enum {
                name: e, args: ea, ..
            },
            Type::Struct {
                name: f, args: fa, ..
            }
            | Type::Enum {
                name: f, args: fa, ..
            },
        ) if !e.is_empty() && e == f => {
            ea.len() != fa.len() || ea.iter().zip(fa).all(|(e, f)| types_compatible(e, f))
        }
        (Type::Float, Type::Float64) | (Type::Float64, Type::Float) => true,
        // Compatibility is used for assignments, arguments, and returns. The
        // runtime does not insert implicit casts, so accepting a numeric type
        // conversion here would make the checked type differ from the value.
        // Mixed numeric *operations* use `numeric_compatible` and widen their
        // result explicitly; value boundaries require an explicit `as` cast.
        (Type::Cell(e), Type::Cell(f)) | (Type::Read(e), Type::Read(f)) => types_compatible(e, f),
        (Type::Option(e), Type::Option(f)) => types_compatible(e, f),
        (
            Type::Func {
                params: e_params,
                ret: e_ret,
            },
            Type::Func {
                params: f_params,
                ret: f_ret,
            },
        ) => {
            e_params.len() == f_params.len()
                && e_params
                    .iter()
                    .zip(f_params)
                    .all(|(e, f)| types_compatible(e, f))
                && types_compatible(e_ret, f_ret)
        }
        // A part no constructor could know (`[]`, `None`, `Ok`'s error, …)
        // fits any type in either direction; `refine_unknown` fills it in.
        (_, Type::Empty) | (Type::Empty, _) => true,
        (Type::Array(e), Type::Array(f)) => types_compatible(e, f),
        (Type::Map { key: ek, value: ev }, Type::Map { key: fk, value: fv }) => {
            types_compatible(ek, fk) && types_compatible(ev, fv)
        }
        (Type::Tuple(es), Type::Tuple(fs)) => {
            es.len() == fs.len() && es.iter().zip(fs).all(|(e, f)| types_compatible(e, f))
        }
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
