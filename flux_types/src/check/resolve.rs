//! Lowering of HIR type annotations into checker [`Type`] values.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_hir::{HirEnumPayload, HirType};

use crate::types::{resolve_type_name, Type};

use super::state::TypeCheckContext;

pub(super) fn resolve_hir_enum_payload(
    ctx: &TypeCheckContext,
    payload: &HirEnumPayload,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    match payload {
        HirEnumPayload::Tuple(ty) => resolve_hir_type(ctx, ty, diagnostics),
        HirEnumPayload::Struct(fields) => Some(Type::Struct {
            name: String::new(),
            args: vec![],
            fields: fields
                .iter()
                .filter_map(|(n, t)| {
                    resolve_hir_type(ctx, t, diagnostics).map(|ty| (n.clone(), ty))
                })
                .collect(),
        }),
    }
}

pub(super) fn resolve_hir_type(
    ctx: &TypeCheckContext,
    ty: &HirType,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    match ty {
        HirType::Named { name, span } => {
            if ctx.generic_parameters.borrow().contains(name) {
                return Some(Type::Error);
            }
            if let Some(ty) = ctx.user_type(name) {
                let params = crate::generic::template_params(ty);
                if !params.is_empty() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!(
                            "{}[{}]",
                            flux_hir::display_path(name),
                            params.join(", ")
                        ),
                        found: format!("{} without type arguments", flux_hir::display_path(name)),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    return Some(Type::Error);
                }
                return Some(ty.clone());
            }
            if let Some(alias) = ctx.type_alias(name) {
                return resolve_hir_type(ctx, alias, diagnostics);
            }
            match resolve_type_name(name, *span) {
                Ok(t) => Some(t),
                Err((name, span)) => {
                    let help = unknown_type_help(ctx, &name);
                    diagnostics.push(FluxError::UnknownType {
                        name,
                        span: span_to_miette(span),
                        source_id: span.source,
                        help,
                    });
                    Some(Type::Error)
                }
            }
        }
        HirType::Array { inner, .. } => Some(Type::Array(Box::new(resolve_hir_type(
            ctx,
            inner,
            diagnostics,
        )?))),
        HirType::Option { inner, .. } => Some(Type::Option(Box::new(resolve_hir_type(
            ctx,
            inner,
            diagnostics,
        )?))),
        HirType::Result { ok, err, .. } => Some(Type::Result {
            ok: Box::new(resolve_hir_type(ctx, ok, diagnostics)?),
            err: Box::new(resolve_hir_type(ctx, err, diagnostics)?),
        }),
        HirType::Function { params, ret, .. } => Some(Type::Func {
            params: params
                .iter()
                .map(|p| resolve_hir_type(ctx, p, diagnostics))
                .collect::<Option<Vec<_>>>()?,
            ret: Box::new(match ret {
                Some(ret) => resolve_hir_type(ctx, ret, diagnostics)?,
                None => Type::Unit,
            }),
        }),
        HirType::Generic { name, args, span } => {
            if name == "Option" && args.len() == 1 {
                Some(Type::Option(Box::new(resolve_hir_type(
                    ctx,
                    &args[0],
                    diagnostics,
                )?)))
            } else if name == "Result" && args.len() == 2 {
                Some(Type::Result {
                    ok: Box::new(resolve_hir_type(ctx, &args[0], diagnostics)?),
                    err: Box::new(resolve_hir_type(ctx, &args[1], diagnostics)?),
                })
            } else if name == flux_hir::TUPLE {
                Some(Type::Tuple(
                    args.iter()
                        .map(|a| resolve_hir_type(ctx, a, diagnostics))
                        .collect::<Option<Vec<_>>>()?,
                ))
            } else if name == "Map" && args.len() == 2 {
                let key = resolve_hir_type(ctx, &args[0], diagnostics)?;
                if !is_map_key(&key) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "a map key type (an integer type, String, Bool, or Char)".into(),
                        found: key.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
                Some(Type::Map {
                    key: Box::new(key),
                    value: Box::new(resolve_hir_type(ctx, &args[1], diagnostics)?),
                })
            } else if let Some(template) = ctx.user_type(name) {
                let params = crate::generic::template_params(template);
                if params.len() != args.len() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: if params.is_empty() {
                            format!(
                                "{}, which takes no type arguments",
                                flux_hir::display_path(name)
                            )
                        } else {
                            format!("{}[{}]", flux_hir::display_path(name), params.join(", "))
                        },
                        found: format!(
                            "{} with {} type argument{}",
                            flux_hir::display_path(name),
                            args.len(),
                            if args.len() == 1 { "" } else { "s" }
                        ),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    return Some(Type::Error);
                }
                let args = args
                    .iter()
                    .map(|a| resolve_hir_type(ctx, a, diagnostics))
                    .collect::<Option<Vec<_>>>()?;
                Some(crate::generic::instantiate(template, &args))
            } else {
                match resolve_type_name(name, *span) {
                    Ok(t) => Some(t),
                    Err((name, span)) => {
                        let help = unknown_type_help(ctx, &name);
                        diagnostics.push(FluxError::UnknownType {
                            name,
                            span: span_to_miette(span),
                            source_id: span.source,
                            help,
                        });
                        Some(Type::Error)
                    }
                }
            }
        }
        HirType::TypeParam { name, .. } => Some(if ctx.generic_parameters.borrow().is_empty() {
            Type::Var(name.clone())
        } else {
            Type::Error
        }),
    }
}

pub(super) fn enum_payload_to_type(payload: &HirEnumPayload) -> Type {
    match payload {
        HirEnumPayload::Tuple(ty) => resolve_hir_type_simple(ty),
        HirEnumPayload::Struct(fields) => Type::Struct {
            name: String::new(),
            args: vec![],
            fields: fields
                .iter()
                .map(|(n, t)| (n.clone(), resolve_hir_type_simple(t)))
                .collect(),
        },
    }
}

pub(crate) fn resolve_hir_type_simple(ty: &HirType) -> Type {
    match ty {
        HirType::Named { name, .. } => match name.as_str() {
            "Int" => Type::Int64,
            "Float" => Type::Float64,
            "Int8" => Type::Int8,
            "Int16" => Type::Int16,
            "Int32" => Type::Int32,
            "Int64" => Type::Int64,
            "UInt8" => Type::UInt8,
            "UInt16" => Type::UInt16,
            "UInt32" => Type::UInt32,
            "UInt64" => Type::UInt64,
            "Float32" => Type::Float32,
            "Float64" => Type::Float64,
            "Char" => Type::Char,
            "Bool" => Type::Bool,
            "String" => Type::String,
            "Unit" => Type::Unit,
            other => Type::Enum {
                name: other.to_string(),
                args: vec![],
                variants: vec![],
            },
        },
        HirType::Array { inner, .. } => Type::Array(Box::new(resolve_hir_type_simple(inner))),
        HirType::Option { inner, .. } => Type::Option(Box::new(resolve_hir_type_simple(inner))),
        HirType::Result { ok, err, .. } => Type::Result {
            ok: Box::new(resolve_hir_type_simple(ok)),
            err: Box::new(resolve_hir_type_simple(err)),
        },
        HirType::Function { params, ret, .. } => Type::Func {
            params: params.iter().map(resolve_hir_type_simple).collect(),
            ret: Box::new(
                ret.as_ref()
                    .map_or(Type::Unit, |r| resolve_hir_type_simple(r)),
            ),
        },
        HirType::Generic { name, args, .. } => {
            if name == "Option" && args.len() == 1 {
                Type::Option(Box::new(resolve_hir_type_simple(&args[0])))
            } else if name == "Result" && args.len() == 2 {
                Type::Result {
                    ok: Box::new(resolve_hir_type_simple(&args[0])),
                    err: Box::new(resolve_hir_type_simple(&args[1])),
                }
            } else if name == flux_hir::TUPLE {
                Type::Tuple(args.iter().map(resolve_hir_type_simple).collect())
            } else if name == "Map" && args.len() == 2 {
                Type::Map {
                    key: Box::new(resolve_hir_type_simple(&args[0])),
                    value: Box::new(resolve_hir_type_simple(&args[1])),
                }
            } else {
                // A user type: a placeholder that `fill_user_types` and
                // `expand` replace with its instance.
                Type::Enum {
                    name: name.clone(),
                    args: args.iter().map(resolve_hir_type_simple).collect(),
                    variants: vec![],
                }
            }
        }
        HirType::TypeParam { name, .. } => Type::Var(name.clone()),
    }
}

/// Whether values of `ty` can be map keys.
pub(crate) fn is_map_key(ty: &Type) -> bool {
    ty.is_error() || ty.is_integer() || matches!(ty, Type::String | Type::Bool | Type::Char)
}

/// E007's help: a type with a close name, as the program would write it.
fn unknown_type_help(ctx: &TypeCheckContext, name: &str) -> Option<String> {
    const LIBRARY: &[&str] = &[
        "Int", "Int8", "Int16", "Int32", "Int64", "UInt8", "UInt16", "UInt32", "UInt64", "Float",
        "Float32", "Float64", "Bool", "Char", "String", "Bytes", "Unit", "Option", "Result", "Map",
    ];
    let user: Vec<String> = ctx
        .type_names()
        .map(|n| flux_value::type_display_name(n).to_string())
        .collect();
    let candidates = LIBRARY
        .iter()
        .copied()
        .chain(user.iter().map(String::as_str));
    flux_diagnostics::did_you_mean(name, candidates)
        .map(|close| format!("a type with a similar name exists: `{close}`"))
}
