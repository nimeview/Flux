//! Statement- and binding-level checking (control flow, mutability, purity).

use super::state::TypeCheckContext;

use std::collections::{HashMap, HashSet};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{
    BindingKind, HirAssign, HirBinding, HirBlock, HirElse, HirEmit, HirExpr, HirFor, HirIf,
    HirStmt, SymbolId,
};

use crate::types::Type;

use super::infer::infer_expr;

use super::resolve::resolve_hir_type;
use super::util::{default_unknown_variants, mentions_empty, types_compatible, unwrap_value_type};

fn require_bool(
    ctx: &TypeCheckContext,
    expr: &HirExpr,
    scope: &mut HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    let Some(ty) = infer_expr(ctx, expr, scope, names, source, diagnostics) else {
        return;
    };
    let ty = unwrap_value_type(&ty);
    if !ty.is_error() && ty != Type::Bool {
        diagnostics.push(FluxError::TypeMismatch {
            expected: "Bool".into(),
            found: ty.display(),
            span: span_to_miette(expr.span()),
            source_id: expr.span().source,
        });
    }
}

pub(super) fn check_binding(
    ctx: &TypeCheckContext,
    binding: &HirBinding,
    types: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    if matches!(binding.kind, BindingKind::Derived) {
        check_derived_expr(ctx, &binding.value, names, diagnostics);
    }

    let expected_inner = types
        .get(&binding.name)
        .and_then(|t| t.unwrap_reactive().cloned())
        .unwrap_or(Type::Error);

    let found = infer_expr(ctx, &binding.value, types, names, source, diagnostics);
    if let (exp, Some(got)) = (expected_inner, found) {
        if !exp.is_error() && !got.is_error() && !types_compatible(&exp, &got) {
            diagnostics.push(FluxError::TypeMismatch {
                expected: exp.display(),
                found: got.display(),
                span: span_to_miette(binding.span),
                source_id: binding.span.source,
            });
        }
    }
}

/// Derived bindings must be pure expressions — no control-flow constructs.
fn check_derived_expr(
    ctx: &TypeCheckContext,
    expr: &HirExpr,
    names: &HashMap<SymbolId, String>,
    diagnostics: &mut DiagnosticBag,
) {
    match expr {
        HirExpr::Snap { span, .. } => {
            diagnostics.push(FluxError::InvalidControlFlowInDerived {
                construct: "snap()".into(),
                span: span_to_miette(*span),
                source_id: span.source,
            });
        }
        HirExpr::Call { callee, args, span } => {
            if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                let direct_builtin = ctx.is_builtin_symbol(*symbol)
                    && names.get(symbol).is_some_and(|name| {
                        flux_std::SIDE_EFFECT_BUILTINS.contains(&name.as_str())
                    });
                if direct_builtin || ctx.is_effectful_function(*symbol) {
                    diagnostics.push(FluxError::SideEffectInDerived {
                        name: names.get(symbol).cloned().unwrap_or_default(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            for arg in args {
                check_derived_expr(ctx, arg, names, diagnostics);
            }
            check_derived_expr(ctx, callee, names, diagnostics);
        }
        HirExpr::Unary { expr, .. } => check_derived_expr(ctx, expr, names, diagnostics),
        HirExpr::Binary { left, right, .. } => {
            check_derived_expr(ctx, left, names, diagnostics);
            check_derived_expr(ctx, right, names, diagnostics);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            check_derived_expr(ctx, cond, names, diagnostics);
            check_derived_expr(ctx, then_expr, names, diagnostics);
            check_derived_expr(ctx, else_expr, names, diagnostics);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                check_derived_expr(ctx, elem, names, diagnostics);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::Ident { .. }
        | HirExpr::EnumVariant { .. } => {}
        HirExpr::StructLit { fields, .. } => {
            for (_, e) in fields {
                check_derived_expr(ctx, e, names, diagnostics);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            check_derived_expr(ctx, scrutinee, names, diagnostics);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    check_derived_expr(ctx, guard, names, diagnostics);
                }
                for e in arm.body_exprs() {
                    check_derived_expr(ctx, e, names, diagnostics);
                }
                check_derived_expr(ctx, &arm.expr, names, diagnostics);
            }
        }
        HirExpr::Field { base, .. } => check_derived_expr(ctx, base, names, diagnostics),
        HirExpr::Cast { expr, .. } => check_derived_expr(ctx, expr, names, diagnostics),
        HirExpr::Closure { fn_symbol, span } => {
            if ctx.is_effectful_function(*fn_symbol) {
                diagnostics.push(FluxError::SideEffectInDerived {
                    name: names.get(fn_symbol).cloned().unwrap_or_default(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_block(
    ctx: &TypeCheckContext,
    block: &HirBlock,
    locals: &HashMap<SymbolId, Type>,
    module: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    reactive: bool,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    immutable_locals: &mut HashSet<SymbolId>,
) {
    let mut scope = locals.clone();
    for stmt in &block.stmts {
        check_stmt(
            ctx,
            stmt,
            &mut scope,
            module,
            names,
            reactive,
            source,
            diagnostics,
            immutable_locals,
        );
    }
}

/// Check every explicit return against a function's declared result type.
/// `infer_block_return` is used to infer a function's implicit tail value; it
/// cannot validate returns nested inside control-flow branches.
pub(super) fn check_return_types(
    ctx: &TypeCheckContext,
    block: &HirBlock,
    locals: &HashMap<SymbolId, Type>,
    expected: &Type,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    let mut scope = locals.clone();
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let(binding) => {
                let ty = binding
                    .ty
                    .as_ref()
                    .and_then(|ty| resolve_hir_type(ctx, ty, diagnostics))
                    .or_else(|| infer_expr(ctx, &binding.value, &scope, names, source, diagnostics))
                    .unwrap_or(Type::Error);
                scope.insert(binding.name, ty);
            }
            HirStmt::Return(ret) => {
                let found = ret
                    .value
                    .as_ref()
                    .and_then(|value| infer_expr(ctx, value, &scope, names, source, diagnostics))
                    .unwrap_or(Type::Unit);
                let expected_value = unwrap_value_type(expected);
                let found_value = unwrap_value_type(&found);
                if !expected_value.is_error()
                    && !found_value.is_error()
                    && !types_compatible(&expected_value, &found_value)
                {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected_value.display(),
                        found: found_value.display(),
                        span: span_to_miette(ret.span),
                        source_id: ret.span.source,
                    });
                }
            }
            HirStmt::If(if_stmt) => {
                check_return_types(
                    ctx,
                    &if_stmt.then_branch,
                    &scope,
                    expected,
                    names,
                    source,
                    diagnostics,
                );
                if let Some(else_branch) = &if_stmt.else_branch {
                    match else_branch {
                        HirElse::If(nested) => {
                            let nested_block = HirBlock {
                                stmts: vec![HirStmt::If((**nested).clone())],
                                span: nested.span,
                            };
                            check_return_types(
                                ctx,
                                &nested_block,
                                &scope,
                                expected,
                                names,
                                source,
                                diagnostics,
                            );
                        }
                        HirElse::Block(block) => check_return_types(
                            ctx,
                            block,
                            &scope,
                            expected,
                            names,
                            source,
                            diagnostics,
                        ),
                    }
                }
            }
            HirStmt::While(loop_stmt) => check_return_types(
                ctx,
                &loop_stmt.body,
                &scope,
                expected,
                names,
                source,
                diagnostics,
            ),
            HirStmt::For(loop_stmt) => {
                let mut loop_scope = scope.clone();
                let iter_ty = infer_expr(ctx, &loop_stmt.iter, &scope, names, source, diagnostics)
                    .unwrap_or(Type::Error);
                let elem_ty = match unwrap_value_type(&iter_ty) {
                    Type::Array(inner) => *inner,
                    _ => Type::Error,
                };
                loop_scope.insert(loop_stmt.name, elem_ty);
                check_return_types(
                    ctx,
                    &loop_stmt.body,
                    &loop_scope,
                    expected,
                    names,
                    source,
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_if(
    ctx: &TypeCheckContext,
    stmt: &HirIf,
    scope: &mut HashMap<SymbolId, Type>,
    module: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    reactive: bool,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    immutable_locals: &mut HashSet<SymbolId>,
) {
    require_bool(ctx, &stmt.cond, scope, names, source, diagnostics);
    check_block(
        ctx,
        &stmt.then_branch,
        scope,
        module,
        names,
        reactive,
        source,
        diagnostics,
        immutable_locals,
    );
    if let Some(el) = &stmt.else_branch {
        match el {
            HirElse::If(i) => check_if(
                ctx,
                i,
                scope,
                module,
                names,
                reactive,
                source,
                diagnostics,
                immutable_locals,
            ),
            HirElse::Block(b) => check_block(
                ctx,
                b,
                scope,
                module,
                names,
                reactive,
                source,
                diagnostics,
                immutable_locals,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_stmt(
    ctx: &TypeCheckContext,
    stmt: &HirStmt,
    scope: &mut HashMap<SymbolId, Type>,
    module: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    reactive: bool,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    immutable_locals: &mut HashSet<SymbolId>,
) {
    match stmt {
        HirStmt::Let(s) => {
            let ty =
                s.ty.as_ref()
                    .and_then(|t| resolve_hir_type(ctx, t, diagnostics))
                    .unwrap_or_else(|| {
                        // Use scope (includes params + prior locals) not just module-level types.
                        let inferred = infer_expr(ctx, &s.value, scope, names, source, diagnostics)
                            .unwrap_or(Type::Error);
                        reject_empty_array_binding(inferred, s.span, diagnostics)
                    });
            scope.insert(s.name, ty);
            if !s.mutable {
                immutable_locals.insert(s.name);
            }
            if let Some(got) = infer_expr(ctx, &s.value, scope, names, source, diagnostics) {
                if let Some(expected) = scope.get(&s.name) {
                    let expected = unwrap_value_type(expected);
                    let got = unwrap_value_type(&got);
                    if !expected.is_error() && !got.is_error() && !types_compatible(&expected, &got)
                    {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: expected.display(),
                            found: got.display(),
                            span: span_to_miette(s.span),
                            source_id: s.span.source,
                        });
                    }
                }
            }
        }
        HirStmt::Assign(HirAssign { name, value, span }) => {
            if let Some(local_name) = names.get(name) {
                if ctx.const_bindings.contains(local_name) {
                    diagnostics.push(FluxError::AssignToImmutableLocal {
                        name: local_name.clone(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            if immutable_locals.contains(name) {
                let local_name = names
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| format!("binding #{name:?}"));
                diagnostics.push(FluxError::AssignToImmutableLocal {
                    name: local_name,
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
            } else if let Some(ty) = module.get(name).or_else(|| scope.get(name)) {
                if matches!(ty, Type::Read(_)) {
                    diagnostics.push(FluxError::AssignToDerived {
                        name: format!("binding #{name:?}"),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            let expected = module
                .get(name)
                .or_else(|| scope.get(name))
                .map(unwrap_value_type);
            let got = infer_expr(ctx, value, scope, names, source, diagnostics)
                .map(|ty| unwrap_value_type(&ty));
            if let (Some(expected), Some(got)) = (expected, got) {
                if !expected.is_error() && !got.is_error() && !types_compatible(&expected, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected.display(),
                        found: got.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
        }
        HirStmt::Emit(HirEmit { event, args, span }) => match module.get(event) {
            Some(Type::Event(payload)) => {
                if args.len() > 1 {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "single payload argument".into(),
                        found: format!("{} arguments", args.len()),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
                if let Some(arg) = args.first() {
                    let got = infer_expr(ctx, arg, scope, names, source, diagnostics)
                        .unwrap_or(Type::Error);
                    if !payload.is_error() && !got.is_error() && !types_compatible(payload, &got) {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: payload.display(),
                            found: got.display(),
                            span: span_to_miette(arg.span()),
                            source_id: arg.span().source,
                        });
                    }
                } else if !matches!(**payload, Type::Unit) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: payload.display(),
                        found: "Unit".into(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            Some(_) | None => {
                diagnostics.push(FluxError::UnknownEvent {
                    name: format!("event #{event:?}"),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
            }
        },
        HirStmt::Expr(expr) => {
            let _ = infer_expr(ctx, expr, scope, names, source, diagnostics);
        }
        HirStmt::Return(r) => {
            if let Some(v) = &r.value {
                let _ = infer_expr(ctx, v, scope, names, source, diagnostics);
            }
        }
        HirStmt::Break(_) | HirStmt::Continue(_) => {}
        HirStmt::If(s) => check_if(
            ctx,
            s,
            scope,
            module,
            names,
            reactive,
            source,
            diagnostics,
            immutable_locals,
        ),
        HirStmt::While(h) => {
            require_bool(ctx, &h.cond, scope, names, source, diagnostics);
            check_block(
                ctx,
                &h.body,
                scope,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
        HirStmt::For(HirFor {
            name, iter, body, ..
        }) => {
            let iter_ty =
                infer_expr(ctx, iter, scope, names, source, diagnostics).unwrap_or(Type::Error);
            let iter_ty = unwrap_value_type(&iter_ty);
            let elem = match iter_ty {
                Type::Array(inner) => *inner,
                other if !other.is_error() => {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "Array (use range(n) for numeric sequences)".into(),
                        found: other.display(),
                        span: span_to_miette(iter.span()),
                        source_id: iter.span().source,
                    });
                    Type::Error
                }
                _ => Type::Error,
            };
            scope.insert(*name, elem);
            check_block(
                ctx,
                body,
                scope,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
        HirStmt::Watch(w) => {
            check_block(
                ctx,
                &w.body,
                scope,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
        HirStmt::On(o) => {
            let mut local = module.clone();
            if let (Some(param), Some(Type::Event(payload))) = (o.param, module.get(&o.event)) {
                local.insert(param, *payload.clone());
            }
            check_block(
                ctx,
                &o.body,
                &local,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
    }
}

/// A binding cannot take its type from a bare `[]`: there is no element type
/// to infer. Reports that and returns the type to use instead (elements `Error`).
pub(super) fn reject_empty_array_binding(
    ty: Type,
    span: flux_diagnostics::Span,
    diagnostics: &mut DiagnosticBag,
) -> Type {
    let ty = default_unknown_variants(&ty);
    if !mentions_empty(&ty) {
        return ty;
    }
    diagnostics.push(FluxError::TypeMismatch {
        expected: "an element type annotation, e.g. `let xs: [String] = []`".into(),
        found: "empty array `[]` with no element type".into(),
        span: span_to_miette(span),
        source_id: span.source,
    });
    Type::Array(Box::new(Type::Error))
}
