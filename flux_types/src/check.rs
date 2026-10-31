//! Type checking and inference driver.
//!
//! The checker is split into focused submodules:
//! - [`state`]  — per-run [`TypeCheckContext`] (user types, enum payloads, trait impls)
//! - [`resolve`] — HIR type annotation → checker [`Type`] lowering
//! - [`infer`]  — expression / block inference (`expr`, `builtins`, `ops`, `util`)
//! - [`stmt`]   — statement, binding and control-flow checking
//! - [`util`]   — small type helpers (numeric widening, compatibility, ...)
//!
//! [`typecheck_program`] orchestrates the passes: register types, seed module
//! bindings and function signatures, infer binding types, then check bodies.

#[cfg(test)]
mod tests;

mod cycles;
mod effects;
mod infer;
mod resolve;
pub(crate) mod state;
mod stmt;
mod util;

use std::collections::{HashMap, HashSet};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{BindingKind, HirItem, HirProgram, SymbolId};

use crate::builtins::builtin_fn_type;
use crate::types::Type;

use infer::{infer_block_return, infer_expr};
use resolve::{resolve_hir_enum_payload, resolve_hir_type};
use state::TypeCheckContext;
use stmt::{check_binding, check_block, check_if, check_return_types};
use util::types_compatible;

#[derive(Debug, Clone)]
pub struct TypedProgram {
    pub types: HashMap<SymbolId, Type>,
}

pub fn typecheck_program(
    program: &HirProgram,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<TypedProgram> {
    let mut types = HashMap::new();

    let ctx = TypeCheckContext::from_program(program);

    if let Some(cycle) = cycles::find_derived_cycle(program) {
        let first = cycle[0];
        let span = program
            .items
            .iter()
            .find_map(|item| match item {
                HirItem::Binding(binding) if binding.name == first => Some(binding.span),
                _ => None,
            })
            .unwrap_or_else(|| program.symbols[&first].span);
        diagnostics.push(FluxError::CyclicDependency {
            cycle: cycle
                .iter()
                .map(|id| program.names.get(id).map(String::as_str).unwrap_or("?"))
                .collect::<Vec<_>>()
                .join(" -> "),
            span: span_to_miette(span),
            source_id: span.source,
        });
        return None;
    }

    // Builtin function types.
    for (id, name) in &program.names {
        if !ctx.is_builtin_symbol(*id) {
            continue;
        }
        if let Some(ty) = builtin_fn_type(name) {
            types.insert(*id, ty);
        } else if flux_std::is_builtin_fn(name) {
            types.insert(
                *id,
                Type::Func {
                    params: vec![],
                    ret: Box::new(Type::Unit),
                },
            );
        }
    }

    // Pre-register module bindings so forward references resolve during inference.
    for item in &program.items {
        let HirItem::Binding(b) = item else { continue };
        let inner =
            b.ty.as_ref()
                .and_then(|t| resolve_hir_type(&ctx, t, diagnostics))
                .unwrap_or(Type::Error);
        types.insert(
            b.name,
            match b.kind {
                BindingKind::Source => Type::Cell(Box::new(inner)),
                BindingKind::Derived => Type::Read(Box::new(inner)),
            },
        );
    }

    // Register Event/Fn/Struct/Enum types first, in a dedicated pass, so that bindings
    // referencing them (including trait impl methods and desugared closures, which are
    // appended at the end of `program.items` regardless of their source position) always
    // see a fully-registered function type during inference below.
    for item in &program.items {
        match item {
            HirItem::Event(ev) => {
                let payload = ev
                    .payload
                    .as_ref()
                    .and_then(|f| resolve_hir_type(&ctx, &f.ty, diagnostics))
                    .unwrap_or(Type::Unit);
                types.insert(ev.name, Type::Event(Box::new(payload)));
            }
            HirItem::Fn(f) if f.type_params.is_empty() => {
                let mut fn_scope = types.clone();
                for p in &f.params {
                    fn_scope.insert(
                        p.name,
                        resolve_hir_type(&ctx, &p.ty, diagnostics).unwrap_or(Type::Error),
                    );
                }
                let inferred = if f.ret.is_none() {
                    let mut speculative_diagnostics = DiagnosticBag::new();
                    infer_block_return(
                        &ctx,
                        &f.body,
                        &fn_scope,
                        &program.names,
                        source,
                        &mut speculative_diagnostics,
                    )
                } else {
                    None
                };
                let ret = match &f.ret {
                    Some(t) => resolve_hir_type(&ctx, t, diagnostics)?,
                    None => inferred.unwrap_or(Type::Unit),
                };
                let params = f
                    .params
                    .iter()
                    .map(|p| resolve_hir_type(&ctx, &p.ty, diagnostics))
                    .collect::<Option<Vec<_>>>()?;
                types.insert(
                    f.name,
                    Type::Func {
                        params,
                        ret: Box::new(ret),
                    },
                );
            }
            HirItem::Fn(_) => {}
            HirItem::Struct(s) => {
                let fields = s
                    .fields
                    .iter()
                    .map(|(n, t)| {
                        (
                            n.clone(),
                            resolve_hir_type(&ctx, t, diagnostics).unwrap_or(Type::Error),
                        )
                    })
                    .collect();
                let name = program.names.get(&s.name).cloned().unwrap_or_default();
                types.insert(s.name, Type::Struct { name, fields });
            }
            HirItem::Enum(e) => {
                let variants = e
                    .variants
                    .iter()
                    .map(|(n, t)| {
                        (
                            n.clone(),
                            t.as_ref()
                                .and_then(|p| resolve_hir_enum_payload(&ctx, p, diagnostics)),
                        )
                    })
                    .collect();
                let name = program.names.get(&e.name).cloned().unwrap_or_default();
                types.insert(e.name, Type::Enum { name, variants });
            }
            HirItem::Binding(_)
            | HirItem::Watch(_)
            | HirItem::On(_)
            | HirItem::SourceAssign(_)
            | HirItem::InitIf(_)
            | HirItem::Trait(_)
            | HirItem::Impl(_) => {}
        }
    }

    // Resolve inferred module types to a fixed point. An `Error` placeholder
    // carries no false assumption about a forward reference's eventual type.
    // Function return inference participates because bindings may call functions
    // that themselves read later bindings.
    for _ in 0..=program.items.len() {
        let mut changed = false;
        let mut speculative_diagnostics = DiagnosticBag::new();
        for item in &program.items {
            match item {
                HirItem::Binding(b) if b.ty.is_none() => {
                    let Some(inner) = infer_expr(
                        &ctx,
                        &b.value,
                        &types,
                        &program.names,
                        source,
                        &mut speculative_diagnostics,
                    ) else {
                        continue;
                    };
                    if inner.is_error() {
                        continue;
                    }
                    let binding_ty = match b.kind {
                        BindingKind::Source => Type::Cell(Box::new(inner)),
                        BindingKind::Derived => Type::Read(Box::new(inner)),
                    };
                    if types.get(&b.name) != Some(&binding_ty) {
                        types.insert(b.name, binding_ty);
                        changed = true;
                    }
                }
                HirItem::Fn(f) if f.type_params.is_empty() && f.ret.is_none() => {
                    let mut fn_scope = types.clone();
                    for p in &f.params {
                        fn_scope.insert(
                            p.name,
                            resolve_hir_type(&ctx, &p.ty, &mut speculative_diagnostics)
                                .unwrap_or(Type::Error),
                        );
                    }
                    let Some(ret) = infer_block_return(
                        &ctx,
                        &f.body,
                        &fn_scope,
                        &program.names,
                        source,
                        &mut speculative_diagnostics,
                    ) else {
                        continue;
                    };
                    if ret.is_error() {
                        continue;
                    }
                    if let Some(Type::Func { ret: current, .. }) = types.get_mut(&f.name) {
                        if **current != ret {
                            **current = ret;
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }

    for item in &program.items {
        let HirItem::Binding(b) = item else { continue };
        let inner = if let Some(ty) = &b.ty {
            resolve_hir_type(&ctx, ty, diagnostics)?
        } else {
            let inferred = infer_expr(&ctx, &b.value, &types, &program.names, source, diagnostics)?;
            stmt::reject_empty_array_binding(inferred, b.span, diagnostics)
        };
        if inner.is_error() && !diagnostics.has_errors() {
            diagnostics.push(FluxError::TypeMismatch {
                expected: "inferable binding type".into(),
                found: "unresolved type".into(),
                span: span_to_miette(b.span),
                source_id: b.span.source,
            });
        }
        let binding_ty = match b.kind {
            BindingKind::Source => Type::Cell(Box::new(inner)),
            BindingKind::Derived => Type::Read(Box::new(inner)),
        };
        types.insert(b.name, binding_ty);
    }

    if diagnostics.has_errors() {
        return None;
    }

    for item in &program.items {
        match item {
            HirItem::Binding(b) => {
                check_binding(&ctx, b, &types, &program.names, source, diagnostics);
            }
            HirItem::Fn(f) if f.type_params.is_empty() => {
                let mut local = types.clone();
                let mut immutable_locals = HashSet::new();
                for p in &f.params {
                    local.insert(p.name, resolve_hir_type(&ctx, &p.ty, diagnostics).unwrap());
                    immutable_locals.insert(p.name);
                }
                check_block(
                    &ctx,
                    &f.body,
                    &local,
                    &types,
                    &program.names,
                    f.reactive,
                    source,
                    diagnostics,
                    &mut immutable_locals,
                );
                if let Some(explicit) = &f.ret {
                    let expected =
                        resolve_hir_type(&ctx, explicit, diagnostics).unwrap_or(Type::Error);
                    check_return_types(
                        &ctx,
                        &f.body,
                        &local,
                        &expected,
                        &program.names,
                        source,
                        diagnostics,
                    );
                    let got = infer_block_return(
                        &ctx,
                        &f.body,
                        &local,
                        &program.names,
                        source,
                        diagnostics,
                    )
                    .unwrap_or(Type::Unit);
                    if !expected.is_error() && !got.is_error() && !types_compatible(&expected, &got)
                    {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: expected.display(),
                            found: got.display(),
                            span: span_to_miette(f.span),
                            source_id: f.span.source,
                        });
                    }
                }
            }
            HirItem::Watch(w) => {
                if let Some(target) = w.target {
                    if let Some(ty) = types.get(&target) {
                        if !ty.is_reactive() {
                            diagnostics.push(FluxError::TypeMismatch {
                                expected: "reactive binding".into(),
                                found: ty.display(),
                                span: span_to_miette(w.span),
                                source_id: w.span.source,
                            });
                        }
                    }
                }
                let mut immutable_locals = HashSet::new();
                check_block(
                    &ctx,
                    &w.body,
                    &types,
                    &types,
                    &program.names,
                    true,
                    source,
                    diagnostics,
                    &mut immutable_locals,
                );
            }
            HirItem::On(o) => {
                match types.get(&o.event) {
                    Some(Type::Event(_)) => {}
                    Some(sym_ty) => {
                        let name = program
                            .names
                            .get(&o.event)
                            .cloned()
                            .unwrap_or_else(|| format!("#{}", o.event.0));
                        diagnostics.push(FluxError::UnknownEvent {
                            name,
                            span: span_to_miette(o.span),
                            source_id: o.span.source,
                        });
                        let _ = sym_ty;
                    }
                    None => {
                        diagnostics.push(FluxError::UnknownEvent {
                            name: format!("#{}", o.event.0),
                            span: span_to_miette(o.span),
                            source_id: o.span.source,
                        });
                    }
                }
                let mut local = types.clone();
                if let (Some(param), Some(Type::Event(payload))) = (o.param, types.get(&o.event)) {
                    local.insert(param, *payload.clone());
                }
                let mut immutable_locals = HashSet::new();
                check_block(
                    &ctx,
                    &o.body,
                    &local,
                    &types,
                    &program.names,
                    true,
                    source,
                    diagnostics,
                    &mut immutable_locals,
                );
            }
            HirItem::Event(_) => {}
            HirItem::SourceAssign(a) => {
                if let Some(Type::Cell(inner)) = types.get(&a.name) {
                    let got =
                        infer_expr(&ctx, &a.value, &types, &program.names, source, diagnostics)
                            .unwrap_or(Type::Error);
                    if !inner.is_error() && !got.is_error() && !types_compatible(inner, &got) {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: inner.display(),
                            found: got.display(),
                            span: span_to_miette(a.span),
                            source_id: a.span.source,
                        });
                    }
                }
            }
            HirItem::InitIf(i) => {
                let mut scope = types.clone();
                let mut immutable_locals = HashSet::new();
                check_if(
                    &ctx,
                    i,
                    &mut scope,
                    &types,
                    &program.names,
                    false,
                    source,
                    diagnostics,
                    &mut immutable_locals,
                );
            }
            HirItem::Struct(_) | HirItem::Enum(_) | HirItem::Trait(_) | HirItem::Impl(_) => {}
            HirItem::Fn(_) => {}
        }
    }

    check_main_signature(program, diagnostics);

    if diagnostics.has_errors() {
        None
    } else {
        Some(TypedProgram { types })
    }
}

/// `fn main` is the program entry point that `flux run` calls after module
/// initialization, so it takes no parameters and returns `Int` (the exit
/// status) or nothing. Imported modules' `main` functions are mangled and
/// never match.
fn check_main_signature(program: &HirProgram, diagnostics: &mut DiagnosticBag) {
    for item in &program.items {
        let HirItem::Fn(f) = item else { continue };
        if program.names.get(&f.name).map(String::as_str) != Some("main") {
            continue;
        }
        let ret_ok = match &f.ret {
            None => true,
            Some(flux_hir::HirType::Named { name, .. }) => {
                matches!(name.as_str(), "Int" | "Int64" | "Unit")
            }
            Some(_) => false,
        };
        if !f.params.is_empty() || !f.type_params.is_empty() || !ret_ok {
            diagnostics.push(FluxError::TypeMismatch {
                expected: "`fn main()` or `fn main() -> Int` (no parameters)".into(),
                found: format!(
                    "`main` with {} parameter(s){}",
                    f.params.len(),
                    if ret_ok {
                        ""
                    } else {
                        " and a non-Int return type"
                    }
                ),
                span: span_to_miette(f.span),
                source_id: f.span.source,
            });
        }
    }
}
