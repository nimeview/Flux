//! Lowering of functions (`fn` / `rfn`) and reactive effects (`watch` / `on`).

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, Span};
use flux_syntax::{Block, FnDecl, OnStmt, Param, RFnDecl, TypeName, WatchStmt};

use crate::context::ScopeKind;
use crate::nodes::*;
use crate::symbol::{SymbolKind, SymbolTable};

use super::exprs::{lower_param_type, record_array_hint, resolve_name};
use super::stmts::lower_block;

pub(super) fn lower_fn(
    decl: &FnDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reactive: bool,
) -> Option<HirFn> {
    lower_fn_inner(
        &decl.name.name,
        &decl.type_params,
        &decl.params,
        decl.ret.as_ref(),
        &decl.body,
        decl.span,
        symbols,
        diagnostics,
        reactive,
    )
}

pub(super) fn lower_rfn(
    decl: &RFnDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirFn> {
    lower_fn_inner(
        &decl.name.name,
        &decl.type_params,
        &decl.params,
        decl.ret.as_ref(),
        &decl.body,
        decl.span,
        symbols,
        diagnostics,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_fn_inner(
    name: &str,
    type_params: &[String],
    params: &[Param],
    ret: Option<&TypeName>,
    body: &Block,
    span: Span,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reactive: bool,
) -> Option<HirFn> {
    let sym_id = symbols.lookup(name)?.id;
    symbols.push(ScopeKind::Function { reactive });
    // Params and locals shadow outer element-type hints only inside this body.
    let outer_hints = symbols.array_elem_hints.clone();
    for p in params {
        record_array_hint(symbols, &p.name.name, Some(&p.ty), None);
    }

    for tp in type_params {
        let _ = symbols.declare(tp, SymbolKind::Builtin, span);
    }

    let hir_params = params
        .iter()
        .filter_map(|p| {
            let id = match symbols.declare(&p.name.name, SymbolKind::Param, p.span) {
                Ok(id) => id,
                Err(prev) => {
                    diagnostics.push(FluxError::DuplicateDefinition {
                        name: p.name.name.clone(),
                        prev_span: span_to_miette(prev.span),
                        span: span_to_miette(p.span),
                        source_id: p.span.source,
                    });
                    return None;
                }
            };
            Some(HirParam {
                name: id,
                ty: lower_param_type(&p.ty, type_params),
                span: p.span,
            })
        })
        .collect();

    let scope = if reactive {
        ScopeKind::Tracking
    } else {
        ScopeKind::Function { reactive: false }
    };

    let hir_body = lower_block(body, symbols, scope, diagnostics);
    symbols.array_elem_hints = outer_hints;
    let hir_body = hir_body?;

    symbols.pop();

    Some(HirFn {
        name: sym_id,
        type_params: type_params.to_vec(),
        params: hir_params,
        ret: ret.map(|t| lower_param_type(t, type_params)),
        body: hir_body,
        reactive,
        span,
    })
}

pub(super) fn lower_watch(
    stmt: &WatchStmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirWatch> {
    let target = stmt
        .target
        .as_ref()
        .and_then(|t| resolve_name(t.name.as_str(), t.span, symbols, diagnostics));
    let body = lower_block(&stmt.body, symbols, ScopeKind::Tracking, diagnostics)?;
    Some(HirWatch {
        target,
        body,
        span: stmt.span,
    })
}

pub(super) fn lower_on(
    stmt: &OnStmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirOn> {
    let event = resolve_name(&stmt.event.name, stmt.event.span, symbols, diagnostics)?;
    symbols.push(ScopeKind::Tracking);
    let param = if let Some(p) = &stmt.param {
        match symbols.declare(&p.name, SymbolKind::Param, p.span) {
            Ok(id) => Some(id),
            Err(prev) => {
                diagnostics.push(FluxError::DuplicateDefinition {
                    name: p.name.clone(),
                    prev_span: span_to_miette(prev.span),
                    span: span_to_miette(p.span),
                    source_id: p.span.source,
                });
                None
            }
        }
    } else {
        None
    };
    let body = lower_block(&stmt.body, symbols, ScopeKind::Tracking, diagnostics)?;
    symbols.pop();
    Some(HirOn {
        event,
        param,
        body,
        span: stmt.span,
    })
}
