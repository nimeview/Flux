//! Builtin function call type inference.

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, SymbolId};

use crate::types::Type;

use super::super::state::TypeCheckContext;
use super::super::util::{types_compatible, unwrap_value_type};
use super::expr::infer_expr;
use super::util::{check_callback, infer_array_elem};

#[allow(clippy::too_many_arguments)]
fn check_numeric_args(
    ctx: &TypeCheckContext,
    args: &[HirExpr],
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Vec<Type>> {
    let mut result = Vec::with_capacity(args.len());
    for arg in args {
        let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
        let got = unwrap_value_type(&got);
        if !got.is_error() && !got.is_integer() && !got.is_float() {
            diagnostics.push(FluxError::TypeMismatch {
                expected: "numeric type".into(),
                found: got.display(),
                span: span_to_miette(arg.span()),
                source_id: arg.span().source,
            });
        }
        result.push(got);
    }
    Some(result)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_builtin_call(
    ctx: &TypeCheckContext,
    name: &str,
    args: &[HirExpr],
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    span: flux_diagnostics::Span,
) -> Option<Type> {
    let arity = match name {
        "range" => Some((0, 3)),
        "filter" => Some((1, 2)),
        "reduce" => Some((2, 3)),
        "print" | "println" | "eprint" | "eprintln" => Some((0, 1)),
        "assert" => Some((1, 1)),
        "assert_eq" | "min" | "max" | "pow" | "str_split" | "str_contains" | "str_starts_with"
        | "str_ends_with" | "str_join" | "write_file" | "append_file" | "push" | "map" | "get" => {
            Some((2, 2))
        }
        "clamp" | "str_replace" | "set" => Some((3, 3)),
        "map_new" => Some((0, 0)),
        "map_has" | "map_remove" => Some((2, 2)),
        "map_keys" | "map_values" => Some((1, 1)),
        "read_line" | "None" | "args" => Some((0, 0)),
        "len" | "Some" | "Ok" | "Err" | "pop" | "str_len" | "str_trim" | "str_chars"
        | "to_string" | "parse_int" | "parse_float" | "str_to_lower" | "str_to_upper" | "abs"
        | "sqrt" | "floor" | "ceil" | "read_file" | "file_exists" => Some((1, 1)),
        _ => None,
    };
    if let Some((min, max)) = arity {
        if !(min..=max).contains(&args.len()) {
            let expected = if min == max {
                format!("{min} arguments")
            } else {
                format!("{min} to {max} arguments")
            };
            diagnostics.push(FluxError::TypeMismatch {
                expected,
                found: format!("{} arguments", args.len()),
                span: span_to_miette(span),
                source_id: span.source,
            });
        }
    }

    match name {
        "args" => Some(Type::Array(Box::new(Type::String))),
        "print" | "println" | "eprint" | "eprintln" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !matches!(got, Type::String | Type::Int64 | Type::Float64 | Type::Bool) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "printable value".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Unit)
        }
        "len" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let inner = unwrap_value_type(&got);
                if !matches!(inner, Type::Array(_) | Type::String | Type::Map { .. }) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "Array, String, or Map".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Int64)
        }
        "range" => {
            let int_ty = Type::Int64;
            match args.len() {
                0 => {}
                1 => {
                    if let Some(arg) = args.first() {
                        let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                        if !matches!(
                            unwrap_value_type(&got),
                            Type::Int64 | Type::Int32 | Type::Int
                        ) {
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: "Int".into(),
                                found: got.display(),
                                span: span_to_miette(arg.span()),
                                source_id: arg.span().source,
                            });
                        }
                    }
                }
                2 | 3 => {
                    for arg in args.iter().take(3) {
                        let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                        if !matches!(
                            unwrap_value_type(&got),
                            Type::Int64 | Type::Int32 | Type::Int
                        ) {
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: "Int".into(),
                                found: got.display(),
                                span: span_to_miette(arg.span()),
                                source_id: arg.span().source,
                            });
                        }
                    }
                }
                _ => {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "range(end), range(start, end), or range(start, end, step)"
                            .into(),
                        found: format!("{} arguments", args.len()),
                        span: span_to_miette(span),
                        source_id: span.source,
                    });
                }
            }
            let _ = int_ty;
            Some(Type::Array(Box::new(Type::Int64)))
        }
        "Some" => {
            let inner = match args.first() {
                Some(a) => {
                    unwrap_value_type(&infer_expr(ctx, a, types, names, source, diagnostics)?)
                }
                None => Type::Unit,
            };
            Some(Type::Option(Box::new(inner)))
        }
        // Parts a constructor cannot know are `Empty`; context fills them in.
        "None" => Some(Type::Option(Box::new(Type::Empty))),
        "Ok" => {
            let ok = match args.first() {
                Some(a) => {
                    unwrap_value_type(&infer_expr(ctx, a, types, names, source, diagnostics)?)
                }
                None => Type::Unit,
            };
            Some(Type::Result {
                ok: Box::new(ok),
                err: Box::new(Type::Empty),
            })
        }
        "Err" => {
            let err = match args.first() {
                Some(a) => {
                    unwrap_value_type(&infer_expr(ctx, a, types, names, source, diagnostics)?)
                }
                None => Type::String,
            };
            Some(Type::Result {
                ok: Box::new(Type::Empty),
                err: Box::new(err),
            })
        }
        "push" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            if let Some(arg) = args.get(1) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if elem_ty == Type::Empty {
                    // `push([], x)` fixes the element type to `x`'s.
                    return Some(Type::Array(Box::new(got)));
                }
                if !elem_ty.is_error() && !got.is_error() && !types_compatible(&elem_ty, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: elem_ty.display(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Array(Box::new(elem_ty)))
        }
        // On a map: `set(m, k, v)` / `get(m, k)`; otherwise the array forms.
        "set" | "get" if first_is_map(ctx, args, types, names, source) => {
            let (key_ty, value_ty) =
                infer_map_arg(ctx, args.first(), types, names, source, diagnostics);
            let got_key = check_arg(ctx, args.get(1), &key_ty, types, names, source, diagnostics);
            if name == "set" {
                let got_value = check_arg(
                    ctx,
                    args.get(2),
                    &value_ty,
                    types,
                    names,
                    source,
                    diagnostics,
                );
                // `set(map_new(), k, v)` fixes the unknown key/value types,
                // as `push([], x)` does for an array.
                Some(Type::Map {
                    key: Box::new(super::super::util::refine_unknown(&key_ty, &got_key)),
                    value: Box::new(super::super::util::refine_unknown(&value_ty, &got_value)),
                })
            } else {
                Some(Type::Option(Box::new(value_ty)))
            }
        }
        "map_new" => Some(Type::Map {
            key: Box::new(Type::Empty),
            value: Box::new(Type::Empty),
        }),
        "map_has" | "map_remove" => {
            let (key_ty, value_ty) =
                infer_map_arg(ctx, args.first(), types, names, source, diagnostics);
            let _ = check_arg(ctx, args.get(1), &key_ty, types, names, source, diagnostics);
            Some(if name == "map_has" {
                Type::Bool
            } else {
                Type::Map {
                    key: Box::new(key_ty),
                    value: Box::new(value_ty),
                }
            })
        }
        "map_keys" | "map_values" => {
            let (key_ty, value_ty) =
                infer_map_arg(ctx, args.first(), types, names, source, diagnostics);
            Some(Type::Array(Box::new(if name == "map_keys" {
                key_ty
            } else {
                value_ty
            })))
        }
        "set" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            if let Some(index) = args.get(1) {
                let got = infer_expr(ctx, index, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !got.is_error() && !got.is_integer() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "an integer index".into(),
                        found: got.display(),
                        span: span_to_miette(index.span()),
                        source_id: index.span().source,
                    });
                }
            }
            if let Some(value) = args.get(2) {
                let got = infer_expr(ctx, value, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !elem_ty.is_error() && !got.is_error() && !types_compatible(&elem_ty, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: elem_ty.display(),
                        found: got.display(),
                        span: span_to_miette(value.span()),
                        source_id: value.span().source,
                    });
                }
            }
            Some(Type::Array(Box::new(elem_ty)))
        }
        "pop" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            Some(Type::Array(Box::new(elem_ty)))
        }
        "get" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            if let Some(index) = args.get(1) {
                let got = infer_expr(ctx, index, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !got.is_error() && !got.is_integer() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "an integer index".into(),
                        found: got.display(),
                        span: span_to_miette(index.span()),
                        source_id: index.span().source,
                    });
                }
            }
            Some(Type::Option(Box::new(elem_ty)))
        }
        "map" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            let ret_ty = args
                .get(1)
                .and_then(|cb| {
                    check_callback(
                        ctx,
                        cb,
                        std::slice::from_ref(&elem_ty),
                        types,
                        names,
                        source,
                        diagnostics,
                    )
                })
                .unwrap_or_else(|| elem_ty.clone());
            Some(Type::Array(Box::new(ret_ty)))
        }
        "filter" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            if let Some(cb) = args.get(1) {
                if let Some(ret_ty) = check_callback(
                    ctx,
                    cb,
                    std::slice::from_ref(&elem_ty),
                    types,
                    names,
                    source,
                    diagnostics,
                ) {
                    if !ret_ty.is_error() && !matches!(ret_ty, Type::Bool) {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: "Bool".into(),
                            found: ret_ty.display(),
                            span: span_to_miette(cb.span()),
                            source_id: cb.span().source,
                        });
                    }
                }
            }
            Some(Type::Array(Box::new(elem_ty)))
        }
        "reduce" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            let init_ty = match args.get(1) {
                Some(a) => {
                    unwrap_value_type(&infer_expr(ctx, a, types, names, source, diagnostics)?)
                }
                None => Type::Error,
            };
            let ret_ty = args
                .get(2)
                .and_then(|cb| {
                    check_callback(
                        ctx,
                        cb,
                        &[init_ty.clone(), elem_ty],
                        types,
                        names,
                        source,
                        diagnostics,
                    )
                })
                .unwrap_or_else(|| init_ty.clone());
            Some(ret_ty)
        }
        "str_len" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Int64)
        }
        "str_split" => {
            for arg in args.iter().take(2) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Array(Box::new(Type::String)))
        }
        "str_join" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let inner = unwrap_value_type(&got);
                if !matches!(inner, Type::Array(element) if *element == Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "[String]".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            if let Some(arg) = args.get(1) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::String)
        }
        "str_contains" => {
            for arg in args.iter().take(2) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Bool)
        }
        "str_chars" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Array(Box::new(Type::String)))
        }
        "str_trim" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::String)
        }
        "read_line" => Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::String),
        }),
        "to_string" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if matches!(got, Type::Func { .. } | Type::Event(_)) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "a value with a text form".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::String)
        }
        "parse_int" | "parse_float" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            let ok = if name == "parse_int" {
                Type::Int64
            } else {
                Type::Float64
            };
            Some(Type::Result {
                ok: Box::new(ok),
                err: Box::new(Type::String),
            })
        }
        "read_file" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::String),
            })
        }
        "write_file" | "append_file" => {
            for arg in args.iter().take(2) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Result {
                ok: Box::new(Type::Unit),
                err: Box::new(Type::String),
            })
        }
        "file_exists" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Bool)
        }
        "min" | "max" | "clamp" => {
            let arg_types = check_numeric_args(ctx, args, types, names, source, diagnostics)?;
            let Some(first) = arg_types.first() else {
                return Some(Type::Error);
            };
            Some(
                arg_types
                    .iter()
                    .skip(1)
                    .fold(first.clone(), |current, next| {
                        super::super::util::widen_numeric(&current, next)
                    }),
            )
        }
        "abs" | "sqrt" | "floor" | "ceil" | "pow" => {
            check_numeric_args(ctx, args, types, names, source, diagnostics)?;
            Some(Type::Float64)
        }
        "assert" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !got.is_error() && got != Type::Bool {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "Bool".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Unit)
        }
        "assert_eq" => {
            if args.len() >= 2 {
                let left = infer_expr(ctx, &args[0], types, names, source, diagnostics)?;
                let right = infer_expr(ctx, &args[1], types, names, source, diagnostics)?;
                let left = unwrap_value_type(&left);
                let right = unwrap_value_type(&right);
                // The same rule as `==`.
                if !super::ops::equality_allowed(&left, &right) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "compatible equality types".into(),
                        found: format!("{} and {}", left.display(), right.display()),
                        span: span_to_miette(args[1].span()),
                        source_id: args[1].span().source,
                    });
                }
            } else {
                for arg in args {
                    let _ = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                }
            }
            Some(Type::Unit)
        }
        "str_starts_with" | "str_ends_with" => {
            for arg in args.iter().take(2) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::Bool)
        }
        "str_replace" => {
            for arg in args.iter().take(3) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::String)
        }
        "str_to_lower" | "str_to_upper" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !matches!(unwrap_value_type(&got), Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "String".into(),
                        found: got.display(),
                        span: span_to_miette(arg.span()),
                        source_id: arg.span().source,
                    });
                }
            }
            Some(Type::String)
        }
        _ => {
            diagnostics.push(FluxError::UndefinedName {
                name: name.into(),
                span: span_to_miette(span),
                source_id: span.source,
            });
            Some(Type::Error)
        }
    }
}

/// Whether the first argument is a map (without reporting anything).
fn first_is_map(
    ctx: &TypeCheckContext,
    args: &[HirExpr],
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
) -> bool {
    let mut quiet = DiagnosticBag::new();
    args.first()
        .and_then(|a| infer_expr(ctx, a, types, names, source, &mut quiet))
        .is_some_and(|t| matches!(unwrap_value_type(&t), Type::Map { .. }))
}

/// Key and value types of a `Map` argument (an error elsewhere).
fn infer_map_arg(
    ctx: &TypeCheckContext,
    arg: Option<&HirExpr>,
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> (Type, Type) {
    let Some(arg) = arg else {
        return (Type::Error, Type::Error);
    };
    match infer_expr(ctx, arg, types, names, source, diagnostics).map(|t| unwrap_value_type(&t)) {
        Some(Type::Map { key, value }) => (*key, *value),
        Some(other) => {
            if !other.is_error() {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "Map".into(),
                    found: other.display(),
                    span: span_to_miette(arg.span()),
                    source_id: arg.span().source,
                });
            }
            (Type::Error, Type::Error)
        }
        None => (Type::Error, Type::Error),
    }
}

/// Type-check `arg` against `expected` (a map key or value); its type.
fn check_arg(
    ctx: &TypeCheckContext,
    arg: Option<&HirExpr>,
    expected: &Type,
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Type {
    let Some(arg) = arg else { return Type::Error };
    let Some(got) = infer_expr(ctx, arg, types, names, source, diagnostics) else {
        return Type::Error;
    };
    let got = unwrap_value_type(&got);
    if !expected.is_error() && !got.is_error() && !types_compatible(expected, &got) {
        diagnostics.push(FluxError::TypeMismatch {
            expected: expected.display(),
            found: got.display(),
            span: span_to_miette(arg.span()),
            source_id: arg.span().source,
        });
    }
    got
}
