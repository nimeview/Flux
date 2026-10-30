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
        "print" | "println" => Some((0, 1)),
        "assert" => Some((1, 1)),
        "assert_eq" | "min" | "max" | "pow" | "str_split" | "str_contains" | "str_starts_with"
        | "str_ends_with" | "str_join" | "write_file" | "append_file" | "push" | "map" => {
            Some((2, 2))
        }
        "clamp" | "str_replace" => Some((3, 3)),
        "read_line" | "None" => Some((0, 0)),
        "len" | "Some" | "Ok" | "Err" | "pop" | "str_len" | "str_trim" | "str_to_lower"
        | "str_to_upper" | "abs" | "sqrt" | "floor" | "ceil" | "read_file" | "file_exists" => {
            Some((1, 1))
        }
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
        "print" | "println" => {
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
                if !matches!(inner, Type::Array(_) | Type::String) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "Array or String".into(),
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
        "None" => Some(Type::Option(Box::new(Type::Unit))),
        "Ok" => {
            let ok = match args.first() {
                Some(a) => {
                    unwrap_value_type(&infer_expr(ctx, a, types, names, source, diagnostics)?)
                }
                None => Type::Unit,
            };
            Some(Type::Result {
                ok: Box::new(ok),
                err: Box::new(Type::String),
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
                ok: Box::new(Type::Unit),
                err: Box::new(err),
            })
        }
        "push" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            if let Some(arg) = args.get(1) {
                let got = infer_expr(ctx, arg, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
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
        "pop" => {
            let elem_ty = infer_array_elem(ctx, args.first(), types, names, source, diagnostics);
            Some(Type::Array(Box::new(elem_ty)))
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
                let same_comparable_type = left == right
                    && match &left {
                        Type::Bool | Type::Char | Type::String | Type::Unit => true,
                        Type::Enum { variants, .. } => {
                            variants.iter().all(|(_, payload)| payload.is_none())
                        }
                        _ => false,
                    };
                let comparable = left.is_error()
                    || right.is_error()
                    || super::super::util::numeric_compatible(&left, &right)
                    || same_comparable_type;
                if !comparable {
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
