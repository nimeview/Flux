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
use super::util::{types_compatible, unwrap_value_type};

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
            source_id: source,
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
        check_derived_expr(&binding.value, source, names, diagnostics);
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
                source_id: source,
            });
        }
    }
}

/// Derived bindings must be pure expressions — no control-flow constructs.
fn check_derived_expr(
    expr: &HirExpr,
    source: SourceId,
    names: &HashMap<SymbolId, String>,
    diagnostics: &mut DiagnosticBag,
) {
    match expr {
        HirExpr::Snap { span, .. } => {
            diagnostics.push(FluxError::InvalidControlFlowInDerived {
                construct: "snap()".into(),
                span: span_to_miette(*span),
                source_id: source,
            });
        }
        HirExpr::Call { callee, args, span } => {
            if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                if let Some(name) = names.get(symbol) {
                    if flux_std::SIDE_EFFECT_BUILTINS.contains(&name.as_str()) {
                        diagnostics.push(FluxError::SideEffectInDerived {
                            name: name.clone(),
                            span: span_to_miette(*span),
                            source_id: source,
                        });
                    }
                }
            }
            for arg in args {
                check_derived_expr(arg, source, names, diagnostics);
            }
            check_derived_expr(callee, source, names, diagnostics);
        }
        HirExpr::Unary { expr, .. } => check_derived_expr(expr, source, names, diagnostics),
        HirExpr::Binary { left, right, .. } => {
            check_derived_expr(left, source, names, diagnostics);
            check_derived_expr(right, source, names, diagnostics);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            check_derived_expr(cond, source, names, diagnostics);
            check_derived_expr(then_expr, source, names, diagnostics);
            check_derived_expr(else_expr, source, names, diagnostics);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                check_derived_expr(elem, source, names, diagnostics);
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
                check_derived_expr(e, source, names, diagnostics);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            check_derived_expr(scrutinee, source, names, diagnostics);
            for arm in arms {
                check_derived_expr(&arm.expr, source, names, diagnostics);
            }
        }
        HirExpr::Field { base, .. } => check_derived_expr(base, source, names, diagnostics),
        HirExpr::Cast { expr, .. } => check_derived_expr(expr, source, names, diagnostics),
        HirExpr::Closure { .. } => {}
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
    let _ = infer_expr(ctx, &stmt.cond, scope, names, source, diagnostics);
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
                    .and_then(|t| resolve_hir_type(ctx, t, source, diagnostics))
                    .unwrap_or_else(|| {
                        // Use scope (includes params + prior locals) not just module-level types.
                        infer_expr(ctx, &s.value, scope, names, source, diagnostics)
                            .unwrap_or(Type::Error)
                    });
            scope.insert(s.name, ty);
            if !s.mutable {
                immutable_locals.insert(s.name);
            }
            let _ = infer_expr(ctx, &s.value, scope, names, source, diagnostics);
        }
        HirStmt::Assign(HirAssign { name, value, span }) => {
            if let Some(local_name) = names.get(name) {
                if ctx.const_bindings.contains(local_name) {
                    diagnostics.push(FluxError::AssignToImmutableLocal {
                        name: local_name.clone(),
                        span: span_to_miette(*span),
                        source_id: source,
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
                    source_id: source,
                });
            } else if let Some(ty) = module.get(name).or_else(|| scope.get(name)) {
                if matches!(ty, Type::Read(_)) {
                    diagnostics.push(FluxError::AssignToDerived {
                        name: format!("binding #{name:?}"),
                        span: span_to_miette(*span),
                        source_id: source,
                    });
                }
            }
            let _ = infer_expr(ctx, value, scope, names, source, diagnostics);
        }
        HirStmt::Emit(HirEmit { event, args, span }) => match module.get(event) {
            Some(Type::Event(payload)) => {
                if args.len() > 1 {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "single payload argument".into(),
                        found: format!("{} arguments", args.len()),
                        span: span_to_miette(*span),
                        source_id: source,
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
                            source_id: source,
                        });
                    }
                } else if !matches!(**payload, Type::Unit) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: payload.display(),
                        found: "Unit".into(),
                        span: span_to_miette(*span),
                        source_id: source,
                    });
                }
            }
            Some(_) | None => {
                diagnostics.push(FluxError::UnknownEvent {
                    name: format!("event #{event:?}"),
                    span: span_to_miette(*span),
                    source_id: source,
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
                        source_id: source,
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
