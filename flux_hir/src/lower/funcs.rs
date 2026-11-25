//! Lowering of functions (`fn` / `rfn`) and reactive effects (`watch` / `on`).

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, FluxWarning, Span};
use flux_syntax::{Block, FnDecl, FnKind, OnStmt, Param, RFnDecl, TypeName, WatchStmt};

use crate::context::ScopeKind;
use crate::nodes::*;
use crate::symbol::{SymbolKind, SymbolTable, Writer, WriterKind};

use super::exprs::{lower_param_type, resolve_name};
use super::stmts::lower_block;

pub(super) fn lower_fn(
    decl: &FnDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reactive: bool,
) -> Option<HirFn> {
    let kind = match decl.kind {
        FnKind::Plain => WriterKind::Plain,
        FnKind::Track => WriterKind::Track,
        FnKind::Act => WriterKind::Act,
    };
    if let Some(p) = decl
        .params
        .first()
        .filter(|p| super::decls::takes_self(decl) && p.name.name == "self")
    {
        diagnostics.push(FluxError::Parse {
            message: "`self` is only for methods inside an `impl`".to_string(),
            span: span_to_miette(p.span),
            source_id: p.span.source,
        });
        return None;
    }
    let writer = Writer::new(kind, &decl.name.name, Some(decl.keyword));
    lower_fn_as(decl, symbols, diagnostics, reactive, writer)
}

/// Lowers `decl`, reporting its writes to sources as `writer`'s.
pub(super) fn lower_fn_as(
    decl: &FnDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reactive: bool,
    writer: Writer,
) -> Option<HirFn> {
    symbols.writers.push(writer);
    let function = lower_fn_inner(
        &decl.name.name,
        &decl.type_params,
        &decl.params,
        decl.ret.as_ref(),
        &decl.body,
        decl.span,
        symbols,
        diagnostics,
        reactive || decl.kind == FnKind::Track,
        decl.kind == FnKind::Act,
    );
    symbols.writers.pop();
    let mut function = function?;
    function.act = decl.kind == FnKind::Act;
    Some(function)
}

pub(super) fn lower_rfn(
    decl: &RFnDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirFn> {
    diagnostics.push_warning(FluxWarning::Deprecated {
        message: "`rfn` is now written `track fn`".to_string(),
        replacement: "track fn".to_string(),
        span: span_to_miette(decl.keyword),
        source_id: decl.keyword.source,
    });
    symbols
        .writers
        .push(Writer::new(WriterKind::Track, &decl.name.name, None));
    let function = lower_fn_inner(
        &decl.name.name,
        &decl.type_params,
        &decl.params,
        decl.ret.as_ref(),
        &decl.body,
        decl.span,
        symbols,
        diagnostics,
        true,
        false,
    );
    symbols.writers.pop();
    function
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
    act: bool,
) -> Option<HirFn> {
    let sym_id = symbols.lookup(name)?.id;
    symbols.push(ScopeKind::Function { reactive });

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
                unannotated: false,
                defaulted_to_int: false,
                span: p.span,
            })
        })
        .collect();

    // An `act fn` reads current values like a handler, without tracking them.
    let scope = if reactive || act {
        ScopeKind::Tracking
    } else {
        ScopeKind::Function { reactive: false }
    };

    let hir_body = lower_block(body, symbols, scope, diagnostics);
    // Close the function's scope even when its body failed, or its
    // parameters would hide names in the functions after it.
    symbols.pop();
    let hir_body = hir_body?;

    Some(HirFn {
        is_closure: false,
        name: sym_id,
        type_params: type_params.to_vec(),
        params: hir_params,
        captures: Vec::new(),
        ret: ret.map(|t| lower_param_type(t, type_params)),
        body: hir_body,
        reactive,
        act,
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
    symbols.push(ScopeKind::Tracking);
    let prev = stmt
        .prev
        .as_ref()
        .and_then(|p| symbols.declare(&p.name, SymbolKind::Param, p.span).ok());
    symbols
        .writers
        .push(Writer::new(WriterKind::Watch, "watch", None));
    let body = lower_block(&stmt.body, symbols, ScopeKind::Tracking, diagnostics);
    symbols.writers.pop();
    symbols.pop();
    let body = body?;
    Some(HirWatch {
        target,
        prev,
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
    let mut names: Vec<&flux_syntax::Ident> = stmt.params.iter().collect();
    if let Some((name, span)) = &stmt.arrow {
        if names.is_empty() {
            diagnostics.push_warning(FluxWarning::Deprecated {
                message: format!(
                    "`on {0} {{ {1} -> }}` is now written `on {0}({1}) {{ }}`",
                    stmt.event.name, name.name
                ),
                replacement: format!("({}) {{", name.name),
                span: span_to_miette(*span),
                source_id: span.source,
            });
        } else {
            diagnostics.push(FluxError::Parse {
                message: "name the event's values once: `on E(x, y) { }`".to_string(),
                span: span_to_miette(name.span),
                source_id: name.span.source,
            });
        }
        names.push(name);
    }
    symbols.push(ScopeKind::Tracking);
    let mut params = Vec::with_capacity(names.len());
    for p in names {
        match symbols.declare(&p.name, SymbolKind::Param, p.span) {
            Ok(id) => params.push(id),
            Err(prev) => diagnostics.push(FluxError::DuplicateDefinition {
                name: p.name.clone(),
                prev_span: span_to_miette(prev.span),
                span: span_to_miette(p.span),
                source_id: p.span.source,
            }),
        }
    }
    symbols
        .writers
        .push(Writer::new(WriterKind::Handler, &stmt.event.name, None));
    let body = lower_block(&stmt.body, symbols, ScopeKind::Tracking, diagnostics);
    symbols.writers.pop();
    symbols.pop();
    Some(HirOn {
        event,
        params,
        body: body?,
        span: stmt.span,
    })
}
