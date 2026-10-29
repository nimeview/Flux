//! Expression type inference.

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, HirMatchArm, SymbolId};
use flux_syntax::UnaryOp;

use crate::types::{resolve_type_name, Type};

use super::super::state::TypeCheckContext;
use super::super::util::{
    lookup_name, numeric_type, trait_dispatch_tag, types_compatible, unwrap_value_type,
};
use super::builtins::infer_builtin_call;
use super::ops::infer_binary;

fn bind_match_arm_locals(
    ctx: &TypeCheckContext,
    scrut_ty: &Type,
    arm: &HirMatchArm,
    scope: &mut HashMap<SymbolId, Type>,
    _source: SourceId,
    _diagnostics: &mut DiagnosticBag,
) {
    if arm.bindings.is_empty() {
        return;
    }
    let inner = unwrap_value_type(scrut_ty);
    match inner {
        Type::Option(payload) if arm.variant == "Some" && arm.bindings.len() == 1 => {
            scope.insert(arm.bindings[0], *payload);
        }
        Type::Result { ok, .. } if arm.variant == "Ok" && arm.bindings.len() == 1 => {
            scope.insert(arm.bindings[0], *ok);
        }
        Type::Result { err, .. } if arm.variant == "Err" && arm.bindings.len() == 1 => {
            scope.insert(arm.bindings[0], *err);
        }
        Type::Enum { name, .. } => {
            if let Some(payload_ty) = ctx.enum_payload(&name, &arm.variant).cloned() {
                if arm.binding_fields.is_empty() {
                    if let Some(&sym) = arm.bindings.first() {
                        scope.insert(sym, payload_ty);
                    }
                } else if let Type::Struct { fields, .. } = &payload_ty {
                    for (sym, field) in arm.bindings.iter().zip(arm.binding_fields.iter()) {
                        if let Some((_, ft)) = fields.iter().find(|(n, _)| n == field) {
                            scope.insert(*sym, ft.clone());
                        }
                    }
                }
            }
        }
        _ if arm.variant.is_empty() && arm.bindings.len() == 1 => {
            let inner = unwrap_value_type(scrut_ty);
            scope.insert(arm.bindings[0], inner.clone());
        }
        _ => {}
    }
}

pub(crate) fn infer_expr(
    ctx: &TypeCheckContext,
    expr: &HirExpr,
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    match expr {
        HirExpr::Int { .. } => Some(Type::Int64),
        HirExpr::Float { .. } => Some(Type::Float64),
        HirExpr::Bool { .. } => Some(Type::Bool),
        HirExpr::String { .. } => Some(Type::String),
        HirExpr::Ident { symbol, span } => types.get(symbol).cloned().or_else(|| {
            diagnostics.push(FluxError::UndefinedName {
                name: format!("symbol #{symbol:?}"),
                span: span_to_miette(*span),
                source_id: source,
            });
            Some(Type::Error)
        }),
        HirExpr::Snap { expr, .. } => infer_expr(ctx, expr, types, names, source, diagnostics)
            .and_then(|t| t.unwrap_reactive().cloned()),
        HirExpr::Call { callee, args, span } => {
            if let HirExpr::Field { base, field, .. } = callee.as_ref() {
                let base_ty = infer_expr(ctx, base, types, names, source, diagnostics)?;
                let type_tag = trait_dispatch_tag(&base_ty);
                let fn_id = ctx.trait_impl(&type_tag, field);
                if let Some(fn_id) = fn_id {
                    if let Some(Type::Func { params, ret }) = types.get(&fn_id) {
                        let mut call_args = vec![base.as_ref().clone()];
                        call_args.extend(args.iter().cloned());
                        if params.len() != call_args.len() {
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: format!("{} arguments", params.len()),
                                found: format!("{} arguments", call_args.len()),
                                span: span_to_miette(*span),
                                source_id: source,
                            });
                        }
                        for (p, a) in params.iter().zip(call_args.iter()) {
                            let got = infer_expr(ctx, a, types, names, source, diagnostics)
                                .unwrap_or(Type::Error);
                            if !p.is_error() && !got.is_error() && !types_compatible(p, &got) {
                                diagnostics.push(FluxError::TypeMismatch {
                                    expected: p.display(),
                                    found: got.display(),
                                    span: span_to_miette(a.span()),
                                    source_id: source,
                                });
                            }
                        }
                        return Some(*ret.clone());
                    }
                }
            }
            if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                if let Some(name) = lookup_name(names, *symbol) {
                    if flux_std::is_builtin_fn(name) {
                        return infer_builtin_call(
                            ctx,
                            name,
                            args,
                            types,
                            names,
                            source,
                            diagnostics,
                            *span,
                        );
                    }
                }
            }
            let callee_ty = infer_expr(ctx, callee, types, names, source, diagnostics)?;
            match callee_ty {
                Type::Func { params, ret } => {
                    if params.len() != args.len() {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: format!("{} arguments", params.len()),
                            found: format!("{} arguments", args.len()),
                            span: span_to_miette(*span),
                            source_id: source,
                        });
                    }
                    for (p, a) in params.iter().zip(args.iter()) {
                        let got = infer_expr(ctx, a, types, names, source, diagnostics)
                            .unwrap_or(Type::Error);
                        if !p.is_error() && !got.is_error() && !types_compatible(p, &got) {
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: p.display(),
                                found: got.display(),
                                span: span_to_miette(a.span()),
                                source_id: source,
                            });
                        }
                    }
                    Some(*ret)
                }
                _ => Some(Type::Error),
            }
        }
        HirExpr::Unary { op, expr, .. } => {
            let inner = infer_expr(ctx, expr, types, names, source, diagnostics)?;
            match op {
                UnaryOp::Neg => numeric_type(&inner),
                UnaryOp::Not => Some(Type::Bool),
            }
        }
        HirExpr::Binary {
            op,
            left,
            right,
            span,
        } => {
            let l = infer_expr(ctx, left, types, names, source, diagnostics)?;
            let r = infer_expr(ctx, right, types, names, source, diagnostics)?;
            infer_binary(*op, &l, &r, *span, source, diagnostics)
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            let _ = infer_expr(ctx, cond, types, names, source, diagnostics);
            let t = infer_expr(ctx, then_expr, types, names, source, diagnostics)?;
            let e = infer_expr(ctx, else_expr, types, names, source, diagnostics)?;
            if types_compatible(&t, &e) {
                Some(t)
            } else {
                Some(e)
            }
        }
        HirExpr::Array { elems, .. } => {
            if elems.is_empty() {
                return Some(Type::Array(Box::new(Type::Unit)));
            }
            let first = infer_expr(ctx, &elems[0], types, names, source, diagnostics)?;
            let elem = unwrap_value_type(&first);
            for item in &elems[1..] {
                let got = infer_expr(ctx, item, types, names, source, diagnostics)?;
                let got = unwrap_value_type(&got);
                if !types_compatible(&elem, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: elem.display(),
                        found: got.display(),
                        span: span_to_miette(item.span()),
                        source_id: source,
                    });
                }
            }
            Some(Type::Array(Box::new(elem)))
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            let scrut_ty = infer_expr(ctx, scrutinee, types, names, source, diagnostics)?;
            let mut arm_types = Vec::new();
            for arm in arms {
                let mut arm_scope = types.clone();
                bind_match_arm_locals(ctx, &scrut_ty, arm, &mut arm_scope, source, diagnostics);
                if let Some(guard) = &arm.guard {
                    let guard_ty = infer_expr(ctx, guard, &arm_scope, names, source, diagnostics)
                        .unwrap_or(Type::Error);
                    if !guard_ty.is_error() && guard_ty != Type::Bool {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: "Bool".into(),
                            found: guard_ty.display(),
                            span: span_to_miette(guard.span()),
                            source_id: source,
                        });
                    }
                }
                arm_types.push(infer_expr(
                    ctx,
                    &arm.expr,
                    &arm_scope,
                    names,
                    source,
                    diagnostics,
                )?);
            }
            arm_types.into_iter().next()
        }
        HirExpr::EnumVariant { enum_name, .. } => ctx.user_type(enum_name).cloned().or_else(|| {
            Some(Type::Enum {
                name: enum_name.clone(),
                variants: vec![],
            })
        }),
        HirExpr::StructLit { name, fields, .. } => {
            for (_, expr) in fields {
                let _ = infer_expr(ctx, expr, types, names, source, diagnostics)?;
            }
            ctx.user_type(name).cloned().or_else(|| {
                Some(Type::Struct {
                    name: name.clone(),
                    fields: vec![],
                })
            })
        }
        HirExpr::Field { base, field, span } => {
            let base_ty = infer_expr(ctx, base, types, names, source, diagnostics)?;
            let inner = unwrap_value_type(&base_ty);
            if let Type::Struct { ref fields, .. } = inner {
                if let Some((_, ft)) = fields.iter().find(|(n, _)| n == field) {
                    return Some(ft.clone());
                }
            }
            diagnostics.push(FluxError::TypeMismatch {
                expected: format!("field `{field}`"),
                found: inner.display(),
                span: span_to_miette(*span),
                source_id: source,
            });
            Some(Type::Error)
        }
        HirExpr::Closure { fn_symbol, .. } => types.get(fn_symbol).cloned(),
        HirExpr::Cast { expr, ty, span } => {
            let _ = infer_expr(ctx, expr, types, names, source, diagnostics)?;
            resolve_type_name(ty, *span)
                .map_err(|(name, s)| {
                    diagnostics.push(FluxError::UnknownType {
                        name,
                        span: span_to_miette(s),
                        source_id: source,
                    });
                })
                .ok()
        }
    }
}
