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
            if let Some(ty) = ctx.user_type(name) {
                return Some(ty.clone());
            }
            if let Some(alias) = ctx.type_alias(name) {
                return resolve_hir_type(ctx, alias, diagnostics);
            }
            match resolve_type_name(name, *span) {
                Ok(t) => Some(t),
                Err((name, span)) => {
                    diagnostics.push(FluxError::UnknownType {
                        name,
                        span: span_to_miette(span),
                        source_id: span.source,
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
            } else {
                resolve_type_name(name, *span).ok()
            }
        }
        HirType::TypeParam { name, .. } => Some(Type::Var(name.clone())),
    }
}

pub(super) fn enum_payload_to_type(payload: &HirEnumPayload) -> Type {
    match payload {
        HirEnumPayload::Tuple(ty) => resolve_hir_type_simple(ty),
        HirEnumPayload::Struct(fields) => Type::Struct {
            name: String::new(),
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
                variants: vec![],
            },
        },
        HirType::Array { inner, .. } => Type::Array(Box::new(resolve_hir_type_simple(inner))),
        HirType::Option { inner, .. } => Type::Option(Box::new(resolve_hir_type_simple(inner))),
        HirType::Result { ok, err, .. } => Type::Result {
            ok: Box::new(resolve_hir_type_simple(ok)),
            err: Box::new(resolve_hir_type_simple(err)),
        },
        HirType::Generic { name, args, .. } => {
            if name == "Option" && args.len() == 1 {
                Type::Option(Box::new(resolve_hir_type_simple(&args[0])))
            } else if name == "Result" && args.len() == 2 {
                Type::Result {
                    ok: Box::new(resolve_hir_type_simple(&args[0])),
                    err: Box::new(resolve_hir_type_simple(&args[1])),
                }
            } else {
                Type::Error
            }
        }
        _ => Type::Error,
    }
}
