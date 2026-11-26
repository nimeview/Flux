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
    if decl.params.iter().any(|p| p.mutable) {
        return lower_fn_as(
            &with_mut_params(decl),
            symbols,
            diagnostics,
            reactive,
            writer,
        );
    }
    lower_fn_as(decl, symbols, diagnostics, reactive, writer)
}

/// Records the `mut` parameters of the function declared as `name`.
pub(super) fn note_mut_params(symbols: &mut SymbolTable, name: &str, decl: &FnDecl) {
    let positions: Vec<usize> = decl
        .params
        .iter()
        .enumerate()
        .filter(|(_, p)| p.mutable)
        .map(|(i, _)| i)
        .collect();
    if !positions.is_empty() {
        symbols.mut_params.insert(
            name.to_string(),
            crate::symbol::MutParams {
                positions,
                has_result: decl.ret.is_some(),
            },
        );
    }
}

/// `decl` with its `mut` parameters given back: each is a mutable local that
/// starts as its argument, and the function's value is `(result, p1, …)`
/// when it declares a result, else `(p1, …)` or `p1` alone. Call sites
/// store the parts back (`stmts::mut_call_stmts`).
fn with_mut_params(decl: &FnDecl) -> FnDecl {
    use flux_syntax::{Expr, Ident, LetPattern, LetStmt, Stmt, TypeName};
    let mut decl = decl.clone();
    let span = decl.name.span;
    let muts: Vec<(String, TypeName)> = decl
        .params
        .iter()
        .filter(|p| p.mutable)
        .map(|p| (p.name.name.clone(), p.ty.clone()))
        .collect();
    let has_result = decl.ret.is_some();
    let mut lets = Vec::new();
    for p in decl.params.iter_mut().filter(|p| p.mutable) {
        let hidden = format!("\0{}", p.name.name);
        lets.push(Stmt::Let(LetStmt {
            pub_: false,
            mutable: true,
            pattern: LetPattern::Ident(Ident {
                name: p.name.name.clone(),
                span: p.span,
            }),
            ty: None,
            value: Expr::Ident {
                name: hidden.clone(),
                span: p.span,
            },
            span: p.span,
        }));
        p.name.name = hidden;
        p.mutable = false;
    }
    let values = |span: flux_diagnostics::Span| -> Vec<Expr> {
        muts.iter()
            .map(|(name, _)| Expr::Ident {
                name: name.clone(),
                span,
            })
            .collect()
    };
    // The function's value: the result (if any) and then each parameter.
    let pack = |result: Option<Expr>, span: flux_diagnostics::Span| -> Expr {
        let mut elems: Vec<Expr> = result.into_iter().collect();
        elems.extend(values(span));
        if elems.len() == 1 {
            elems.pop().expect("one value")
        } else {
            Expr::Tuple { elems, span }
        }
    };
    let mut types: Vec<TypeName> = decl.ret.iter().cloned().collect();
    types.extend(muts.iter().map(|(_, ty)| ty.clone()));
    decl.ret = Some(if types.len() == 1 {
        types.pop().expect("one type")
    } else {
        TypeName::Tuple { elems: types, span }
    });
    let mut body = lets;
    body.extend(std::mem::take(&mut decl.body.stmts));
    pack_returns(&mut body, &pack, has_result);
    if !matches!(body.last(), Some(Stmt::Return(_))) {
        if has_result {
            pack_tail(&mut body, &pack);
        } else {
            body.push(Stmt::Expr(pack(None, span)));
        }
    }
    decl.body.stmts = body;
    decl
}

/// Every `return` in `stmts` (not in closures) returns the packed value.
fn pack_returns(
    stmts: &mut [flux_syntax::Stmt],
    pack: &dyn Fn(Option<flux_syntax::Expr>, flux_diagnostics::Span) -> flux_syntax::Expr,
    has_result: bool,
) {
    use flux_syntax::{ElseBranch, Expr, IfStmt, Stmt};
    fn in_if(
        stmt: &mut IfStmt,
        pack: &dyn Fn(Option<Expr>, flux_diagnostics::Span) -> Expr,
        has_result: bool,
    ) {
        pack_returns(&mut stmt.then_branch.stmts, pack, has_result);
        match &mut stmt.else_branch {
            Some(ElseBranch::Block(block)) => pack_returns(&mut block.stmts, pack, has_result),
            Some(ElseBranch::If(next)) => in_if(next, pack, has_result),
            None => {}
        }
    }
    fn in_expr(
        expr: &mut Expr,
        pack: &dyn Fn(Option<Expr>, flux_diagnostics::Span) -> Expr,
        has_result: bool,
    ) {
        if let Expr::Match { arms, .. } = expr {
            for arm in arms {
                pack_returns(&mut arm.stmts, pack, has_result);
            }
        }
    }
    for stmt in stmts {
        match stmt {
            Stmt::Return(ret) => {
                let value = ret.value.take();
                ret.value = Some(pack(if has_result { value } else { None }, ret.span));
            }
            Stmt::If(branch) => in_if(branch, pack, has_result),
            Stmt::While(w) => pack_returns(&mut w.body.stmts, pack, has_result),
            Stmt::For(f) => pack_returns(&mut f.body.stmts, pack, has_result),
            Stmt::Expr(e) => in_expr(e, pack, has_result),
            Stmt::Let(l) => in_expr(&mut l.value, pack, has_result),
            _ => {}
        }
    }
}

/// The function's last value packed with the parameters: a trailing
/// expression, or each branch's of a trailing `if … else`.
fn pack_tail(
    stmts: &mut [flux_syntax::Stmt],
    pack: &dyn Fn(Option<flux_syntax::Expr>, flux_diagnostics::Span) -> flux_syntax::Expr,
) {
    use flux_syntax::{ElseBranch, Stmt};
    match stmts.last_mut() {
        Some(Stmt::Expr(e)) => {
            let span = e.span();
            let value = std::mem::replace(e, flux_syntax::Expr::Bool { value: false, span });
            *e = pack(Some(value), span);
        }
        Some(Stmt::If(branch)) if branch.else_branch.is_some() => {
            fn in_if(
                stmt: &mut flux_syntax::IfStmt,
                pack: &dyn Fn(
                    Option<flux_syntax::Expr>,
                    flux_diagnostics::Span,
                ) -> flux_syntax::Expr,
            ) {
                pack_tail(&mut stmt.then_branch.stmts, pack);
                match &mut stmt.else_branch {
                    Some(ElseBranch::Block(block)) => pack_tail(&mut block.stmts, pack),
                    Some(ElseBranch::If(next)) => in_if(next, pack),
                    None => {}
                }
            }
            in_if(branch, pack);
        }
        _ => {}
    }
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
    if !decl.type_bounds.is_empty() {
        let bounds = decl
            .type_bounds
            .iter()
            .map(|b| crate::HirBound {
                param: b.param.clone(),
                traits: b.traits.iter().map(|t| (t.name.clone(), t.span)).collect(),
            })
            .collect();
        symbols.type_bounds.insert(function.name, bounds);
    }
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
    let event =
        super::exprs::resolve_event(&stmt.event.name, stmt.event.span, symbols, diagnostics)?;
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
