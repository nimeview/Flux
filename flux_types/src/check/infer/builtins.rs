//! Builtin function call type inference.

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, SymbolId};

use crate::types::Type;

use super::super::state::{Scope, TypeCheckContext};
use super::super::util::{types_compatible, unwrap_value_type};
use super::expr::infer_expr;
use super::util::{check_callback, infer_array_elem};

#[allow(clippy::too_many_arguments)]
fn check_numeric_args(
    ctx: &TypeCheckContext,
    args: &[HirExpr],
    types: &Scope<'_>,
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
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    span: flux_diagnostics::Span,
) -> Option<Type> {
    let arity = match name {
        "range" => Some((0, 3)),
        "filter" => Some((1, 2)),
        "reduce" => Some((3, 3)),
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

    if name.starts_with('\0') {
        return infer_method_builtin(ctx, name, args, types, names, source, diagnostics, span);
    }
    match name {
        "args" => Some(Type::Array(Box::new(Type::String))),
        "print" | "println" | "eprint" | "eprintln" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !got.is_error()
                    && !matches!(got, Type::String | Type::Int64 | Type::Float64 | Type::Bool)
                {
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
                if !inner.is_error()
                    && !matches!(inner, Type::Array(_) | Type::String | Type::Map { .. })
                {
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
            if !ret_ty.is_error() && !init_ty.is_error() && !types_compatible(&init_ty, &ret_ty) {
                if let Some(cb) = args.get(2) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: init_ty.display(),
                        found: ret_ty.display(),
                        span: span_to_miette(cb.span()),
                        source_id: cb.span().source,
                    });
                }
            }
            Some(init_ty)
        }
        "str_len" => {
            if let Some(arg) = args.first() {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !types_compatible(&Type::Array(Box::new(Type::String)), &inner) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !super::ops::has_text_form(&got) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
        "abs" => {
            let args = check_numeric_args(ctx, args, types, names, source, diagnostics)?;
            Some(match args.first() {
                Some(ty) if ty.is_integer() => ty.clone(),
                _ => Type::Float64,
            })
        }
        "sqrt" | "floor" | "ceil" | "pow" => {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
                if !got.is_error() && !matches!(unwrap_value_type(&got), Type::String) {
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
            // A function with one signature: each argument has its type.
            if let Some((params, ret)) = crate::builtins::os_signature(name) {
                if params.len() != args.len() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!("{} arguments", params.len()),
                        found: format!("{} arguments", args.len()),
                        span: span_to_miette(span),
                        source_id: span.source,
                    });
                }
                for (param, arg) in params.iter().zip(args) {
                    check_arg(ctx, Some(arg), param, types, names, source, diagnostics);
                }
                return Some(ret);
            }
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
    types: &Scope<'_>,
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
    types: &Scope<'_>,
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
    types: &Scope<'_>,
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

/// `base with { x: v }`, lowered to `(base, "x", v, ...)`: the base must be a
/// struct with those fields, and each value must fit its field. The result
/// has the base's type.
pub(super) fn infer_with(
    ctx: &TypeCheckContext,
    args: &[HirExpr],
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let (base, pairs) = args.split_first()?;
    let base_ty = unwrap_value_type(&infer_expr(ctx, base, types, names, source, diagnostics)?);
    let expanded = ctx.expand(base_ty.clone());
    // The result: the base, with unknown parts that the new values fix
    // (`B { x: N } with { x: S(1) }` is a `B[O[Int]]`).
    let mut result = expanded.clone();
    // Values that fixed unknown parts, checked again against the result.
    let mut refining: Vec<(String, Type, flux_diagnostics::Span)> = Vec::new();
    let fields = match &expanded {
        Type::Struct { fields, .. } => Some(fields.clone()),
        Type::Error => None,
        other => {
            diagnostics.push(FluxError::TypeMismatch {
                expected: "a struct".into(),
                found: other.display(),
                span: span_to_miette(base.span()),
                source_id: base.span().source,
            });
            None
        }
    };
    for pair in pairs.chunks(2) {
        let [HirExpr::String {
            value: name,
            span: name_span,
        }, value] = pair
        else {
            continue;
        };
        let got = infer_expr(ctx, value, types, names, source, diagnostics)
            .map(|ty| unwrap_value_type(&ty))
            .unwrap_or(Type::Error);
        let Some(fields) = &fields else {
            continue;
        };
        match fields.iter().find(|(field, _)| field == name) {
            None => diagnostics.push(FluxError::TypeMismatch {
                expected: format!("field `{name}`"),
                found: expanded.display(),
                span: span_to_miette(*name_span),
                source_id: name_span.source,
            }),
            Some((_, want)) => {
                if !want.is_error() && !got.is_error() && !types_compatible(want, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: want.display(),
                        found: got.display(),
                        span: span_to_miette(value.span()),
                        source_id: value.span().source,
                    });
                } else if crate::check::mentions_empty(want) {
                    result = refine_field(&result, name, &got);
                    refining.push((name.clone(), got.clone(), value.span()));
                }
            }
        }
    }
    if result == expanded {
        return Some(base_ty);
    }
    // Rebind the arguments from the template's fields.
    let Type::Struct {
        name, args, fields, ..
    } = &result
    else {
        return Some(base_ty);
    };
    let Some(template) = ctx.user_type(name) else {
        return Some(base_ty);
    };
    let mut bound = HashMap::new();
    if let Type::Struct {
        args: params,
        fields: declared,
        ..
    } = template
    {
        for (param, arg) in params.iter().zip(args) {
            crate::generic::bind(param, arg, &mut bound);
        }
        for (field, ty) in fields {
            if let Some((_, want)) = declared.iter().find(|(f, _)| f == field) {
                crate::generic::bind(want, ty, &mut bound);
            }
        }
    }
    let args = crate::generic::bound_args(template, &bound);
    let instance = crate::generic::instantiate(template, &args);
    // `with { a: S(1), b: S("s") }` on a `Pair[O[_]]`: the values must agree.
    if let Type::Struct { fields, .. } = &instance {
        for (field, got, at) in &refining {
            if let Some((_, want)) = fields.iter().find(|(f, _)| f == field) {
                if !types_compatible(want, got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: want.display(),
                        found: got.display(),
                        span: span_to_miette(*at),
                        source_id: at.source,
                    });
                }
            }
        }
    }
    Some(instance)
}

/// A generic struct `ty` refined by `got` for its field `name`: the
/// parameters behind the field's unknown parts take `got`'s.
fn refine_field(ty: &Type, name: &str, got: &Type) -> Type {
    let Type::Struct {
        name: type_name,
        args,
        fields,
    } = ty
    else {
        return ty.clone();
    };
    let Some((_, current)) = fields.iter().find(|(f, _)| f == name) else {
        return ty.clone();
    };
    let refined = crate::check::refine_unknown(current, got);
    Type::Struct {
        name: type_name.clone(),
        args: args.clone(),
        fields: fields
            .iter()
            .map(|(f, t)| {
                (
                    f.clone(),
                    if f == name {
                        refined.clone()
                    } else {
                        t.clone()
                    },
                )
            })
            .collect(),
    }
}

/// The method-only library functions (`flux_std::methods`); `args[0]` is the
/// receiver.
#[allow(clippy::too_many_arguments)]
fn infer_method_builtin(
    ctx: &TypeCheckContext,
    name: &str,
    args: &[HirExpr],
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    span: flux_diagnostics::Span,
) -> Option<Type> {
    let infer = |e: &HirExpr, diagnostics: &mut DiagnosticBag| {
        infer_expr(ctx, e, types, names, source, diagnostics)
            .map(|t| unwrap_value_type(&t))
            .unwrap_or(Type::Error)
    };
    let mismatch = |expected: String,
                    found: &Type,
                    at: flux_diagnostics::Span,
                    diagnostics: &mut DiagnosticBag| {
        if !found.is_error() {
            diagnostics.push(FluxError::TypeMismatch {
                expected,
                found: found.display(),
                span: span_to_miette(at),
                source_id: at.source,
            });
        }
    };
    let expect = |want: &Type, e: &HirExpr, got: &Type, diagnostics: &mut DiagnosticBag| {
        if !want.is_error() && !got.is_error() && !types_compatible(want, got) {
            diagnostics.push(FluxError::TypeMismatch {
                expected: want.display(),
                found: got.display(),
                span: span_to_miette(e.span()),
                source_id: e.span().source,
            });
        }
    };
    let callback = |cb: Option<&HirExpr>, params: &[Type], diagnostics: &mut DiagnosticBag| {
        cb.and_then(|cb| check_callback(ctx, cb, params, types, names, source, diagnostics))
            .map(|t| unwrap_value_type(&t))
            .unwrap_or(Type::Error)
    };
    let sortable = |t: &Type| t.is_error() || t.is_integer() || t.is_float() || *t == Type::String;
    let receiver = args.first()?;
    let recv = infer(receiver, diagnostics);
    let elem = match &recv {
        Type::Array(e) => (**e).clone(),
        _ => Type::Error,
    };
    let rest = &args[1..];
    let want_args = |n: usize, diagnostics: &mut DiagnosticBag| {
        if rest.len() != n {
            diagnostics.push(FluxError::TypeMismatch {
                expected: format!("{n} arguments"),
                found: format!("{} arguments", rest.len()),
                span: span_to_miette(span),
                source_id: span.source,
            });
        }
    };
    let array = |t: Type| Type::Array(Box::new(t));
    Some(match name {
        "\0contains" => {
            want_args(1, diagnostics);
            if let Some(v) = rest.first() {
                let got = infer(v, diagnostics);
                expect(&elem, v, &got, diagnostics);
                // `contains` compares with `==`, which functions do not have.
                if !super::ops::equality_supported(&elem) {
                    mismatch(
                        "elements that `==` can compare (no function in them)".into(),
                        &elem,
                        v.span(),
                        diagnostics,
                    );
                }
            }
            Type::Bool
        }
        "\0enumerate" => {
            want_args(0, diagnostics);
            array(Type::Tuple(vec![Type::Int64, elem]))
        }
        "\0zip" => {
            want_args(1, diagnostics);
            let other = rest
                .first()
                .map(|e| infer(e, diagnostics))
                .unwrap_or(Type::Error);
            let other_elem = match other {
                Type::Array(e) => *e,
                ref t => {
                    if let Some(e) = rest.first() {
                        mismatch("an array".into(), t, e.span(), diagnostics);
                    }
                    Type::Error
                }
            };
            array(Type::Tuple(vec![elem, other_elem]))
        }
        "\0slice" => {
            want_args(2, diagnostics);
            for e in rest {
                let got = infer(e, diagnostics);
                if !got.is_integer() {
                    mismatch("an integer index".into(), &got, e.span(), diagnostics);
                }
            }
            array(elem)
        }
        "\0sorted" => {
            want_args(0, diagnostics);
            if !sortable(&elem) {
                mismatch(
                    "an array of numbers or strings".into(),
                    &recv,
                    receiver.span(),
                    diagnostics,
                );
            }
            array(elem)
        }
        "\0reversed" => {
            want_args(0, diagnostics);
            array(elem)
        }
        "\0first" | "\0last" => {
            want_args(0, diagnostics);
            Type::Option(Box::new(elem))
        }
        "\0concat" => {
            let other = rest
                .first()
                .map(|e| infer(e, diagnostics))
                .unwrap_or(Type::Error);
            if let Some(e) = rest.first() {
                expect(&recv, e, &other, diagnostics);
            }
            recv
        }
        "\0unwrap_or" => {
            want_args(1, diagnostics);
            let inner = match &recv {
                Type::Option(t) => (**t).clone(),
                Type::Result { ok, .. } => (**ok).clone(),
                _ => Type::Error,
            };
            if let Some(d) = rest.first() {
                let got = infer(d, diagnostics);
                expect(&inner, d, &got, diagnostics);
                return Some(super::super::util::refine_unknown(&inner, &got));
            }
            inner
        }
        "\0ok_or" => {
            want_args(1, diagnostics);
            let inner = match &recv {
                Type::Option(t) => (**t).clone(),
                _ => Type::Error,
            };
            let err = rest
                .first()
                .map(|e| infer(e, diagnostics))
                .unwrap_or(Type::Error);
            Type::Result {
                ok: Box::new(inner),
                err: Box::new(err),
            }
        }
        "\0is_some" | "\0is_none" | "\0is_ok" | "\0is_err" => {
            want_args(0, diagnostics);
            Type::Bool
        }
        "\0get_or" => {
            want_args(2, diagnostics);
            let (key, value) = match &recv {
                Type::Map { key, value } => ((**key).clone(), (**value).clone()),
                _ => (Type::Error, Type::Error),
            };
            if let (Some(k), Some(d)) = (rest.first(), rest.get(1)) {
                let got = infer(k, diagnostics);
                expect(&key, k, &got, diagnostics);
                let got = infer(d, diagnostics);
                expect(&value, d, &got, diagnostics);
                return Some(super::super::util::refine_unknown(&value, &got));
            }
            value
        }
        "\0entries" => {
            want_args(0, diagnostics);
            match &recv {
                Type::Map { key, value } => {
                    array(Type::Tuple(vec![(**key).clone(), (**value).clone()]))
                }
                _ => Type::Error,
            }
        }
        "\0split_once" => {
            want_args(1, diagnostics);
            if let Some(sep) = rest.first() {
                let got = infer(sep, diagnostics);
                expect(&Type::String, sep, &got, diagnostics);
            }
            Type::Option(Box::new(Type::Tuple(vec![Type::String, Type::String])))
        }
        "\0lines" | "\0words" => {
            want_args(0, diagnostics);
            array(Type::String)
        }
        "\0find" | "\0any" | "\0all" => {
            want_args(1, diagnostics);
            let ret = callback(rest.first(), std::slice::from_ref(&elem), diagnostics);
            if !ret.is_error() && ret != Type::Bool {
                if let Some(cb) = rest.first() {
                    mismatch(
                        "a function returning Bool".into(),
                        &ret,
                        cb.span(),
                        diagnostics,
                    );
                }
            }
            if name == "\0find" {
                Type::Option(Box::new(elem))
            } else {
                Type::Bool
            }
        }
        "\0sorted_by" => {
            want_args(1, diagnostics);
            let key = callback(rest.first(), std::slice::from_ref(&elem), diagnostics);
            if !sortable(&key) {
                if let Some(cb) = rest.first() {
                    mismatch(
                        "a key that is a number or a string".into(),
                        &key,
                        cb.span(),
                        diagnostics,
                    );
                }
            }
            array(elem)
        }
        "\0map_value" => {
            want_args(1, diagnostics);
            match &recv {
                Type::Option(t) => Type::Option(Box::new(callback(
                    rest.first(),
                    &[(**t).clone()],
                    diagnostics,
                ))),
                Type::Result { ok, err } => Type::Result {
                    ok: Box::new(callback(rest.first(), &[(**ok).clone()], diagnostics)),
                    err: err.clone(),
                },
                _ => Type::Error,
            }
        }
        "\0and_then" => {
            want_args(1, diagnostics);
            let (inner, kind) = match &recv {
                Type::Option(t) => ((**t).clone(), "an Option"),
                Type::Result { ok, .. } => ((**ok).clone(), "a Result with the same error type"),
                _ => (Type::Error, ""),
            };
            let ret = callback(rest.first(), &[inner], diagnostics);
            let fits = match (&recv, &ret) {
                (Type::Option(_), Type::Option(_)) => true,
                (Type::Result { err: e1, .. }, Type::Result { err: e2, .. }) => {
                    types_compatible(e1, e2)
                }
                _ => ret.is_error() || recv.is_error(),
            };
            if !fits {
                if let Some(cb) = rest.first() {
                    mismatch(
                        format!("a function returning {kind}"),
                        &ret,
                        cb.span(),
                        diagnostics,
                    );
                }
            }
            // The error type is the receiver's: an `Err` passes through.
            match (&recv, ret) {
                (Type::Result { err, .. }, Type::Result { ok, err: ret_err }) => Type::Result {
                    ok,
                    err: Box::new(super::super::util::refine_unknown(err, &ret_err)),
                },
                (_, ret) => ret,
            }
        }
        "\0map_err" => {
            want_args(1, diagnostics);
            match &recv {
                Type::Result { ok, err } => Type::Result {
                    ok: ok.clone(),
                    err: Box::new(callback(rest.first(), &[(**err).clone()], diagnostics)),
                },
                _ => Type::Error,
            }
        }
        _ => Type::Error,
    })
}
