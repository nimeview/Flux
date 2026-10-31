//! Expression type inference.

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, HirMatchArm, HirStmt, SymbolId};
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

/// Types a block arm's statements in order, extending `scope` with its lets so
/// the arm's final expression can use them.
fn infer_arm_body(
    ctx: &TypeCheckContext,
    arm: &HirMatchArm,
    scope: &mut HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    for stmt in &arm.body.stmts {
        match stmt {
            HirStmt::Let(l) => {
                let value = infer_expr(ctx, &l.value, scope, names, source, diagnostics)
                    .unwrap_or(Type::Error);
                let ty = match &l.ty {
                    Some(ann) => {
                        let declared =
                            super::super::resolve::resolve_hir_type(ctx, ann, diagnostics)
                                .unwrap_or(Type::Error);
                        check_assignable(&declared, &value, l.value.span(), diagnostics);
                        declared
                    }
                    None => value,
                };
                scope.insert(l.name, ty);
            }
            HirStmt::Assign(a) => {
                let value = infer_expr(ctx, &a.value, scope, names, source, diagnostics)
                    .unwrap_or(Type::Error);
                if let Some(target) = scope.get(&a.name).cloned() {
                    check_assignable(&target, &value, a.value.span(), diagnostics);
                }
            }
            HirStmt::Expr(e) => {
                let _ = infer_expr(ctx, e, scope, names, source, diagnostics);
            }
            _ => {}
        }
    }
}

fn check_assignable(
    expected: &Type,
    found: &Type,
    span: flux_diagnostics::Span,
    diagnostics: &mut DiagnosticBag,
) {
    let (e, f) = (unwrap_value_type(expected), unwrap_value_type(found));
    if !e.is_error() && !f.is_error() && !types_compatible(&e, &f) {
        diagnostics.push(FluxError::TypeMismatch {
            expected: e.display(),
            found: f.display(),
            span: span_to_miette(span),
            source_id: span.source,
        });
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
                source_id: span.source,
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
                                source_id: span.source,
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
                                    source_id: a.span().source,
                                });
                            }
                        }
                        return Some(*ret.clone());
                    }
                }
            }
            if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                if let Some(name) =
                    lookup_name(names, *symbol).filter(|_| ctx.is_builtin_symbol(*symbol))
                {
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
                            source_id: span.source,
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
                                source_id: a.span().source,
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
                UnaryOp::Neg => {
                    let found = unwrap_value_type(&inner);
                    let unsigned = matches!(
                        found,
                        Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64
                    );
                    if unsigned {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: "signed numeric type or Float".into(),
                            found: found.display(),
                            span: span_to_miette(expr.span()),
                            source_id: expr.span().source,
                        });
                        Some(Type::Error)
                    } else {
                        numeric_type(&inner).or_else(|| {
                            if found.is_error() {
                                return Some(Type::Error);
                            }
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: "numeric type".into(),
                                found: found.display(),
                                span: span_to_miette(expr.span()),
                                source_id: expr.span().source,
                            });
                            Some(Type::Error)
                        })
                    }
                }
                UnaryOp::Not => {
                    let found = unwrap_value_type(&inner);
                    if found != Type::Bool && !found.is_error() {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: "Bool".into(),
                            found: found.display(),
                            span: span_to_miette(expr.span()),
                            source_id: expr.span().source,
                        });
                        Some(Type::Error)
                    } else {
                        Some(Type::Bool)
                    }
                }
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
            infer_binary(*op, &l, &r, *span, diagnostics)
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            span,
        } => {
            let cond_ty = infer_expr(ctx, cond, types, names, source, diagnostics)?;
            let cond_ty = unwrap_value_type(&cond_ty);
            if cond_ty != Type::Bool && !cond_ty.is_error() {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "Bool".into(),
                    found: cond_ty.display(),
                    span: span_to_miette(cond.span()),
                    source_id: cond.span().source,
                });
            }
            let t = infer_expr(ctx, then_expr, types, names, source, diagnostics)?;
            let e = infer_expr(ctx, else_expr, types, names, source, diagnostics)?;
            let tv = unwrap_value_type(&t);
            let ev = unwrap_value_type(&e);
            if super::super::util::numeric_compatible(&tv, &ev) {
                Some(super::super::util::widen_numeric(&tv, &ev))
            } else if types_compatible(&tv, &ev) {
                Some(super::super::util::refine_unknown(&tv, &ev))
            } else {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: tv.display(),
                    found: ev.display(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                Some(Type::Error)
            }
        }
        HirExpr::Array { elems, .. } => {
            if elems.is_empty() {
                return Some(Type::Array(Box::new(Type::Empty)));
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
                        source_id: item.span().source,
                    });
                }
            }
            Some(Type::Array(Box::new(elem)))
        }
        HirExpr::Match {
            scrutinee,
            arms,
            span,
        } => {
            let scrut_ty = infer_expr(ctx, scrutinee, types, names, source, diagnostics)?;
            let scrut_value_ty = unwrap_value_type(&scrut_ty);
            let variants: Option<Vec<String>> = match &scrut_value_ty {
                Type::Enum { variants, .. } => {
                    Some(variants.iter().map(|(name, _)| name.clone()).collect())
                }
                Type::Option(_) => Some(vec!["Some".into(), "None".into()]),
                Type::Result { .. } => Some(vec!["Ok".into(), "Err".into()]),
                _ => None,
            };
            let mut arm_types = Vec::new();
            let mut covered_variants = std::collections::HashSet::new();
            let mut has_catch_all = false;
            for arm in arms {
                let mut arm_scope = types.clone();
                bind_match_arm_locals(ctx, &scrut_ty, arm, &mut arm_scope, source, diagnostics);
                if arm.variant.is_empty() {
                    if arm.guard.is_none() {
                        has_catch_all = true;
                    }
                } else if let Some(variants) = &variants {
                    if !variants.iter().any(|variant| variant == &arm.variant) {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: format!("one of {}", variants.join(", ")),
                            found: arm.variant.clone(),
                            span: span_to_miette(*span),
                            source_id: span.source,
                        });
                    } else if arm.guard.is_none() {
                        covered_variants.insert(arm.variant.clone());
                    }
                } else {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "enum, Option, Result, or catch-all pattern".into(),
                        found: scrut_value_ty.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
                if let Some(guard) = &arm.guard {
                    let guard_ty = infer_expr(ctx, guard, &arm_scope, names, source, diagnostics)
                        .unwrap_or(Type::Error);
                    if !guard_ty.is_error() && guard_ty != Type::Bool {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: "Bool".into(),
                            found: guard_ty.display(),
                            span: span_to_miette(guard.span()),
                            source_id: guard.span().source,
                        });
                    }
                }
                infer_arm_body(ctx, arm, &mut arm_scope, names, source, diagnostics);
                arm_types.push(infer_expr(
                    ctx,
                    &arm.expr,
                    &arm_scope,
                    names,
                    source,
                    diagnostics,
                )?);
            }
            if !has_catch_all {
                if let Some(variants) = &variants {
                    let missing: Vec<_> = variants
                        .iter()
                        .filter(|variant| !covered_variants.contains(*variant))
                        .cloned()
                        .collect();
                    if !missing.is_empty() {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: format!("exhaustive match ({})", variants.join(", ")),
                            found: format!("missing {}", missing.join(", ")),
                            span: span_to_miette(*span),
                            source_id: span.source,
                        });
                    }
                } else {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "catch-all pattern".into(),
                        found: scrut_value_ty.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            let Some(first) = arm_types.first().cloned() else {
                return Some(Type::Error);
            };
            let mut unified = unwrap_value_type(&first);
            for arm_ty in arm_types.iter().skip(1) {
                let arm_ty = unwrap_value_type(arm_ty);
                if super::super::util::numeric_compatible(&unified, &arm_ty) {
                    unified = super::super::util::widen_numeric(&unified, &arm_ty);
                } else if !types_compatible(&unified, &arm_ty) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: unified.display(),
                        found: arm_ty.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    unified = Type::Error;
                } else {
                    unified = super::super::util::refine_unknown(&unified, &arm_ty);
                }
            }
            Some(unified)
        }
        HirExpr::EnumVariant {
            enum_name,
            variant,
            payload,
            span,
        } => {
            let Some(enum_ty @ Type::Enum { .. }) = ctx.user_type(enum_name).cloned() else {
                diagnostics.push(FluxError::UnknownType {
                    name: enum_name.clone(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                return Some(Type::Error);
            };
            let Type::Enum { variants, .. } = &enum_ty else {
                unreachable!()
            };
            match variants.iter().find(|(name, _)| name == variant) {
                Some((_, Some(expected))) => match payload {
                    Some(payload) => {
                        let got = infer_expr(ctx, payload, types, names, source, diagnostics)
                            .unwrap_or(Type::Error);
                        let got = unwrap_value_type(&got);
                        if !expected.is_error()
                            && !got.is_error()
                            && !types_compatible(expected, &got)
                        {
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: expected.display(),
                                found: got.display(),
                                span: span_to_miette(payload.span()),
                                source_id: payload.span().source,
                            });
                        }
                    }
                    None => diagnostics.push(FluxError::TypeMismatch {
                        expected: format!("payload for {variant}"),
                        found: "no payload".into(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    }),
                },
                Some((_, None)) if payload.is_some() => {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!("payload-free variant {variant}"),
                        found: "payload provided".into(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
                Some((_, None)) => {}
                None => diagnostics.push(FluxError::TypeMismatch {
                    expected: format!(
                        "one of {}",
                        variants
                            .iter()
                            .map(|(name, _)| name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    found: variant.clone(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                }),
            }
            Some(enum_ty)
        }
        HirExpr::StructLit { name, fields, span } => {
            let Some(struct_ty @ Type::Struct { .. }) = ctx.user_type(name).cloned() else {
                diagnostics.push(FluxError::UnknownType {
                    name: name.clone(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                return Some(Type::Error);
            };
            let Type::Struct {
                fields: declared_fields,
                ..
            } = &struct_ty
            else {
                unreachable!()
            };
            let mut seen = std::collections::HashSet::new();
            for (field_name, expr) in fields {
                let got =
                    infer_expr(ctx, expr, types, names, source, diagnostics).unwrap_or(Type::Error);
                let Some((_, expected)) = declared_fields
                    .iter()
                    .find(|(declared, _)| declared == field_name)
                else {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!(
                            "field from {}",
                            declared_fields
                                .iter()
                                .map(|(field, _)| field.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        found: field_name.clone(),
                        span: span_to_miette(expr.span()),
                        source_id: expr.span().source,
                    });
                    continue;
                };
                if !seen.insert(field_name.clone()) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "each struct field exactly once".into(),
                        found: format!("duplicate field `{field_name}`"),
                        span: span_to_miette(expr.span()),
                        source_id: expr.span().source,
                    });
                }
                let got = unwrap_value_type(&got);
                if !expected.is_error() && !got.is_error() && !types_compatible(expected, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected.display(),
                        found: got.display(),
                        span: span_to_miette(expr.span()),
                        source_id: expr.span().source,
                    });
                }
            }
            for (expected_name, _) in declared_fields {
                if !seen.contains(expected_name) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!("field `{expected_name}`"),
                        found: "missing field".into(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            Some(struct_ty)
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
                source_id: span.source,
            });
            Some(Type::Error)
        }
        HirExpr::Closure { fn_symbol, .. } => types.get(fn_symbol).cloned(),
        HirExpr::Cast { expr, ty, span } => {
            let source_ty = infer_expr(ctx, expr, types, names, source, diagnostics)?;
            let target_ty = resolve_type_name(ty, *span)
                .map_err(|(name, s)| {
                    diagnostics.push(FluxError::UnknownType {
                        name,
                        span: span_to_miette(s),
                        source_id: s.source,
                    });
                })
                .ok()?;
            let source_ty = unwrap_value_type(&source_ty);
            let allowed = source_ty.is_error()
                || source_ty == target_ty
                || (source_ty.is_integer() || source_ty.is_float())
                    && (target_ty.is_integer() || target_ty.is_float())
                || matches!(source_ty, Type::Bool | Type::Char)
                    && (target_ty.is_integer() || target_ty.is_float())
                || source_ty.is_integer() && matches!(target_ty, Type::Bool | Type::Char);
            if !allowed {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: format!("value convertible to {}", target_ty.display()),
                    found: source_ty.display(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                Some(Type::Error)
            } else {
                Some(target_ty)
            }
        }
    }
}
