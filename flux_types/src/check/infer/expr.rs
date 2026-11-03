//! Expression type inference.

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, HirMatchArm, HirStmt, SymbolId};
use flux_syntax::UnaryOp;

use crate::types::{resolve_type_name, Type};

use super::super::resolve::resolve_hir_type;
use super::super::state::{Scope, TypeCheckContext};
use super::super::util::{
    lookup_name, numeric_type, trait_dispatch_tag, types_compatible, unwrap_value_type,
};
use super::builtins::infer_builtin_call;
use super::infer_block_return;
use super::ops::infer_binary;

fn bind_match_arm_locals(
    ctx: &TypeCheckContext,
    scrut_ty: &Type,
    arm: &HirMatchArm,
    scope: &mut Scope<'_>,
    _source: SourceId,
    _diagnostics: &mut DiagnosticBag,
) {
    if arm.bindings.is_empty() {
        return;
    }
    for symbol in &arm.bindings {
        scope.insert(*symbol, Type::Error);
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
    scope: &mut Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    for stmt in &arm.body.stmts {
        match stmt {
            HirStmt::Let(l) => {
                let annotation =
                    l.ty.as_ref()
                        .and_then(|t| resolve_hir_type(ctx, t, diagnostics));
                let value = infer_expr_expected(
                    ctx,
                    &l.value,
                    annotation.as_ref(),
                    scope,
                    names,
                    source,
                    diagnostics,
                )
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
            // A module binding gets the full check below: it may be read-only.
            HirStmt::Assign(a) if !scope.module().contains_key(&a.name) => {
                let value = infer_expr(ctx, &a.value, scope, names, source, diagnostics)
                    .unwrap_or(Type::Error);
                if let Some(target) = scope.get(&a.name).cloned() {
                    check_assignable(&target, &value, a.value.span(), diagnostics);
                }
            }
            HirStmt::Expr(e) => {
                let _ = infer_expr(ctx, e, scope, names, source, diagnostics);
            }
            // Control flow (allowed only inside functions).
            other => {
                let module = scope.module();
                super::super::stmt::check_stmt(
                    ctx,
                    other,
                    scope,
                    module,
                    names,
                    false,
                    source,
                    diagnostics,
                    &mut std::collections::HashSet::new(),
                );
            }
        }
    }
}

/// The payload type of `variant` in `ty` (an enum, `Option`, or `Result`).
fn variant_payload_type(ctx: &TypeCheckContext, ty: &Type, variant: &str) -> Option<Type> {
    match (ctx.expand(unwrap_value_type(ty)), variant) {
        (Type::Option(inner), "Some") => Some(*inner),
        (Type::Result { ok, .. }, "Ok") => Some(*ok),
        (Type::Result { err, .. }, "Err") => Some(*err),
        (Type::Enum { variants, .. }, _) => variants
            .into_iter()
            .find(|(name, _)| name == variant)
            .and_then(|(_, payload)| payload),
        _ => None,
    }
}

/// Whether the pattern shapes together match every value of `ty`.
fn shapes_cover(ctx: &TypeCheckContext, shapes: &[&flux_hir::PatShape], ty: &Type) -> bool {
    use flux_hir::PatShape;
    if shapes.iter().any(|s| matches!(s, PatShape::Any)) {
        return true;
    }
    let variants: Vec<String> = match ctx.expand(unwrap_value_type(ty)) {
        Type::Option(_) => vec!["Some".into(), "None".into()],
        Type::Result { .. } => vec!["Ok".into(), "Err".into()],
        Type::Enum { variants, .. } => variants.into_iter().map(|(name, _)| name).collect(),
        _ => return false,
    };
    variants.iter().all(|variant| {
        let payloads: Vec<&Option<Box<PatShape>>> = shapes
            .iter()
            .filter_map(|s| match s {
                PatShape::Variant { name, payload } if name == variant => Some(payload),
                _ => None,
            })
            .collect();
        if payloads.is_empty() {
            return false;
        }
        if payloads
            .iter()
            .any(|p| p.as_deref().is_none_or(|p| *p == PatShape::Any))
        {
            return true;
        }
        let inner: Vec<&PatShape> = payloads.iter().filter_map(|p| p.as_deref()).collect();
        variant_payload_type(ctx, ty, variant)
            .is_some_and(|payload| shapes_cover(ctx, &inner, &payload))
    })
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
    types: &Scope<'_>,
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
                name: lookup_name(names, *symbol)
                    .cloned()
                    .unwrap_or_else(|| format!("symbol #{}", symbol.0)),
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
                    ctx.method_calls
                        .borrow_mut()
                        .insert((ctx.current_function.get(), *span), fn_id);
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
                            let got = infer_expr_expected(
                                ctx,
                                a,
                                Some(p),
                                types,
                                names,
                                source,
                                diagnostics,
                            )
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
                // Not a method of any impl, and not a struct field to call.
                let is_field = matches!(
                    ctx.expand(unwrap_value_type(&base_ty)),
                    Type::Struct { ref fields, .. } if fields.iter().any(|(n, _)| n == field)
                );
                if !is_field && !base_ty.is_error() {
                    diagnostics.push(FluxError::UnknownMethod {
                        method: field.clone(),
                        ty: type_tag,
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    return Some(Type::Error);
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
            match unwrap_value_type(&callee_ty) {
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
                        let got =
                            infer_expr_expected(ctx, a, Some(p), types, names, source, diagnostics)
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
                found if found.is_error() => Some(Type::Error),
                _ => {
                    diagnostics.push(FluxError::NotCallable {
                        callee: match callee.as_ref() {
                            HirExpr::Field { field, .. } => field.clone(),
                            HirExpr::Ident { symbol, .. } => lookup_name(names, *symbol)
                                .cloned()
                                .unwrap_or_else(|| "this value".to_string()),
                            _ => "this value".to_string(),
                        },
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    Some(Type::Error)
                }
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
            if tv.is_error() || ev.is_error() {
                return Some(Type::Error);
            }
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
            let scrut_value_ty = ctx.expand(unwrap_value_type(&scrut_ty));
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
            // Payload shapes of nested-pattern arms, per outer variant.
            let mut nested_arms: HashMap<String, Vec<flux_hir::PatShape>> = HashMap::new();
            let mut has_catch_all = false;
            let mut bools_covered = [false; 2];
            for arm in arms {
                let mut arm_scope = types.clone();
                bind_match_arm_locals(ctx, &scrut_ty, arm, &mut arm_scope, source, diagnostics);
                // An arm for an unknown variant binds nothing; its error is
                // reported once below, not again at every use of the binding.
                for binding in &arm.bindings {
                    if arm_scope.get(binding).is_none() {
                        arm_scope.insert(*binding, Type::Error);
                    }
                }
                if let Some(literal) = &arm.literal {
                    // A literal arm covers nothing by itself, except that
                    // `true` and `false` together cover `Bool`.
                    if arm.guard.is_none() {
                        if let HirExpr::Bool { value, .. } = literal {
                            bools_covered[usize::from(*value)] = true;
                        }
                    }
                    let lit_ty = infer_expr(ctx, literal, &arm_scope, names, source, diagnostics)
                        .unwrap_or(Type::Error);
                    let (want, got) = (
                        unwrap_value_type(&scrut_value_ty),
                        unwrap_value_type(&lit_ty),
                    );
                    if !want.is_error() && !got.is_error() && !types_compatible(&want, &got) {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: want.display(),
                            found: got.display(),
                            span: span_to_miette(literal.span()),
                            source_id: literal.span().source,
                        });
                    }
                } else if arm.variant.is_empty() {
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
                    } else if let Some(shape) = &arm.nested {
                        nested_arms
                            .entry(arm.variant.clone())
                            .or_default()
                            .push(shape.clone());
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
                // An arm that leaves (`return`/`break`/`continue`) has no value.
                if arm.diverges {
                    continue;
                }
                // A block arm ending in a statement is `()`.
                if arm.unit {
                    arm_types.push(Type::Unit);
                    continue;
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
            // A variant is also covered when its nested-pattern arms together
            // cover its payload (`Some(A)`, `Some(B)` for `Option[AB]`).
            for (variant, shapes) in &nested_arms {
                let payload = variant_payload_type(ctx, &scrut_value_ty, variant);
                let refs: Vec<&flux_hir::PatShape> = shapes.iter().collect();
                if payload.is_some_and(|ty| shapes_cover(ctx, &refs, &ty)) {
                    covered_variants.insert(variant.clone());
                }
            }
            if bools_covered == [true, true] {
                has_catch_all = true;
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
                // Every arm leaves: the match produces no value.
                return Some(if arms.is_empty() {
                    Type::Error
                } else {
                    Type::Unit
                });
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
            let inner = ctx.expand(unwrap_value_type(&base_ty));
            if inner.is_error() {
                return Some(Type::Error);
            }
            if let Type::Struct { ref fields, .. } = inner {
                if let Some((_, ft)) = fields.iter().find(|(n, _)| n == field) {
                    return Some(ft.clone());
                }
            }
            let defaulted_param = match base.as_ref() {
                HirExpr::Ident { symbol, .. }
                    if ctx.defaulted_closure_params.borrow().contains(symbol) =>
                {
                    names.get(symbol)
                }
                _ => None,
            };
            let error = if let Some(name) = defaulted_param {
                FluxError::TypeMismatchWithHelp {
                    expected: format!("field `{field}`"),
                    found: inner.display(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                    help: format!(
                        "closure parameter `{name}` defaulted to `Int` because it had no type \
                         annotation or call-site hint; add an explicit annotation, \
                         e.g. `|{name}: YourType| ...`"
                    ),
                }
            } else {
                FluxError::TypeMismatch {
                    expected: format!("field `{field}`"),
                    found: inner.display(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                }
            };
            diagnostics.push(error);
            Some(Type::Error)
        }
        HirExpr::Closure { .. } => {
            infer_closure(ctx, expr, None, types, names, source, diagnostics)
        }
        HirExpr::Index { base, index, span } => {
            let base_ty = infer_expr(ctx, base, types, names, source, diagnostics)?;
            let index_ty = infer_expr(ctx, index, types, names, source, diagnostics)?;
            let index_ty = unwrap_value_type(&index_ty);
            // `m[k]`: the key must have the map's key type.
            if let Type::Map { key, value } = unwrap_value_type(&base_ty) {
                if !index_ty.is_error() && !key.is_error() && !types_compatible(&key, &index_ty) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: key.display(),
                        found: index_ty.display(),
                        span: span_to_miette(index.span()),
                        source_id: index.span().source,
                    });
                }
                return Some(*value);
            }
            if !index_ty.is_error() && !index_ty.is_integer() {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "an integer index".into(),
                    found: index_ty.display(),
                    span: span_to_miette(index.span()),
                    source_id: index.span().source,
                });
            }
            match unwrap_value_type(&base_ty) {
                Type::Array(inner) => Some(*inner),
                Type::Error => Some(Type::Error),
                other => {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "an array or map to index".into(),
                        found: other.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    Some(Type::Error)
                }
            }
        }
        HirExpr::ToText { expr, span } => {
            let inner = infer_expr(ctx, expr, types, names, source, diagnostics)?;
            let inner = unwrap_value_type(&inner);
            // Shown as `to_string` shows it; a function has no text form.
            let printable = !matches!(inner, Type::Func { .. });
            if !printable {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "a value with a text form inside `{…}` (not a function)".into(),
                    found: inner.display(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
            }
            Some(Type::String)
        }
        HirExpr::Try { expr, span } => {
            let inner = infer_expr(ctx, expr, types, names, source, diagnostics)?;
            let inner = unwrap_value_type(&inner);
            if inner.is_error() {
                return Some(Type::Error);
            }
            let fail = |message: String, diagnostics: &mut DiagnosticBag| {
                diagnostics.push(FluxError::InvalidTry {
                    message,
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                Some(Type::Error)
            };
            let Some(ret) = ctx.fn_return.borrow().clone() else {
                return fail(
                    "`?` is only allowed in a function with a declared `Result[…]` or \
                     `Option[…]` return type"
                        .into(),
                    diagnostics,
                );
            };
            // The error type must match exactly: `?` performs no conversion.
            let same = |a: &Type, b: &Type| types_compatible(a, b) && types_compatible(b, a);
            match (unwrap_value_type(&ret), inner) {
                (Type::Result { err: want, .. }, Type::Result { ok, err }) => {
                    if want.is_error() || err.is_error() || same(&want, &err) {
                        Some(*ok)
                    } else {
                        fail(
                            format!(
                                "`?` on a `Result` with error type `{}` in a function returning \
                                 error type `{}`; the error types must match",
                                err.display(),
                                want.display()
                            ),
                            diagnostics,
                        )
                    }
                }
                (Type::Option(_), Type::Option(inner)) => Some(*inner),
                (ret @ (Type::Result { .. } | Type::Option(_)), inner) => fail(
                    format!(
                        "`?` on `{}` in a function returning `{}`; use `?` on {}",
                        inner.display(),
                        ret.display(),
                        if matches!(ret, Type::Option(_)) {
                            "an `Option`"
                        } else {
                            "a `Result`"
                        }
                    ),
                    diagnostics,
                ),
                (ret, _) => fail(
                    format!(
                        "`?` needs the function to return `Result[…]` or `Option[…]`, \
                         but it returns `{}`",
                        ret.display()
                    ),
                    diagnostics,
                ),
            }
        }
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

/// Expected function parameters constrain only unannotated closure parameters.
pub(crate) fn infer_expr_expected(
    ctx: &TypeCheckContext,
    expr: &HirExpr,
    expected: Option<&Type>,
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    if matches!(expr, HirExpr::Closure { .. }) {
        let params = expected.and_then(|t| match unwrap_value_type(t) {
            Type::Func { params, .. } => Some(params),
            _ => None,
        });
        infer_closure(
            ctx,
            expr,
            params.as_deref(),
            types,
            names,
            source,
            diagnostics,
        )
    } else {
        infer_expr(ctx, expr, types, names, source, diagnostics)
    }
}

pub(super) fn infer_closure(
    ctx: &TypeCheckContext,
    expr: &HirExpr,
    expected: Option<&[Type]>,
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<Type> {
    let HirExpr::Closure {
        fn_symbol,
        captures,
        ..
    } = expr
    else {
        return infer_expr(ctx, expr, types, names, source, diagnostics);
    };
    let closure = ctx.capturing_closure(*fn_symbol)?;
    // A closure sees module bindings and its recorded captures, not every
    // preceding local in the creator. Copying that growing scope is quadratic.
    let mut scope = Scope::new(types.module());
    for (inner, outer) in closure.captures.iter().zip(captures) {
        if let Some(ty) = types.get(outer) {
            scope.insert(*inner, ty.clone());
        }
    }
    let mut params = Vec::new();
    for (i, p) in closure.params.iter().enumerate() {
        let contextual = expected.and_then(|ps| ps.get(i)).filter(|t| {
            usable_context(t)
                || (!ctx.generic_parameters.borrow().is_empty() && !matches!(t, Type::Empty))
        });
        let ty = if p.unannotated {
            let previous = ctx.closure_params.borrow().get(&p.name).cloned();
            let ty = contextual
                .cloned()
                .or_else(|| previous.clone())
                .unwrap_or_else(|| {
                    resolve_hir_type(ctx, &p.ty, diagnostics).unwrap_or(Type::Int64)
                });
            if contextual.is_some() {
                ctx.defaulted_closure_params.borrow_mut().remove(&p.name);
            } else if previous.is_none() && p.defaulted_to_int {
                ctx.defaulted_closure_params.borrow_mut().insert(p.name);
            }
            ctx.closure_params.borrow_mut().insert(p.name, ty.clone());
            ty
        } else {
            resolve_hir_type(ctx, &p.ty, diagnostics).unwrap_or(Type::Error)
        };
        scope.insert(p.name, ty.clone());
        params.push(ty);
    }
    let outer_return = ctx.fn_return.replace(None);
    let outer_function = ctx.current_function.replace(Some(*fn_symbol));
    // Lowering gives a closure exactly one expression statement. Inferring
    // that expression checks it (including match-arm statements) and returns
    // its value type; a preceding check_block walk would double nested work.
    debug_assert!(matches!(closure.body.stmts.as_slice(), [HirStmt::Expr(_)]));
    #[cfg(test)]
    {
        let count = ctx.closure_body_checks.get() + 1;
        ctx.closure_body_checks.set(count);
        if let Some(limit) = ctx.closure_check_limit.get() {
            assert!(
                count <= limit,
                "closure body check budget exceeded: {count} > {limit}"
            );
        }
    }
    let ret = infer_block_return(ctx, &closure.body, &scope, names, source, diagnostics)
        .unwrap_or(Type::Unit);
    *ctx.fn_return.borrow_mut() = outer_return;
    ctx.current_function.set(outer_function);
    let ty = Type::Func {
        params,
        ret: Box::new(ret),
    };
    ctx.closure_types
        .borrow_mut()
        .insert(*fn_symbol, ty.clone());
    Some(ty)
}

pub(crate) fn usable_context(ty: &Type) -> bool {
    match ty {
        Type::Error | Type::Empty | Type::Var(_) => false,
        Type::Array(t) | Type::Option(t) | Type::Cell(t) | Type::Read(t) | Type::Event(t) => {
            usable_context(t)
        }
        Type::Map { key, value } => usable_context(key) && usable_context(value),
        Type::Result { ok, err } => usable_context(ok) && usable_context(err),
        Type::Func { params, ret } => params.iter().all(usable_context) && usable_context(ret),
        _ => true,
    }
}
