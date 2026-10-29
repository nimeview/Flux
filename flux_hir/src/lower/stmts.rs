//! Block, statement, match-arm and `if` lowering.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, Span};
use flux_syntax::{
    AssignStmt, Block, ElseBranch, EmitStmt, Expr, ForStmt, IfStmt, LetPattern, LetStmt,
    MatchPattern, ReturnStmt, Stmt, TypeName, WhileStmt,
};

use crate::context::ScopeKind;
use crate::nodes::*;
use crate::symbol::{SymbolId, SymbolKind, SymbolTable};

use super::exprs::{lower_expr, resolve_name};

pub(super) fn lower_block(
    block: &Block,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirBlock> {
    symbols.push(scope);
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        stmts.extend(lower_stmt(stmt, symbols, diagnostics)?);
    }
    symbols.pop();
    Some(HirBlock {
        stmts,
        span: block.span,
    })
}

fn lower_local_decl(
    pattern: &LetPattern,
    ty: &Option<TypeName>,
    value: &Expr,
    span: Span,
    mutable: bool,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<Vec<HirStmt>> {
    match pattern {
        LetPattern::Ident(name) => {
            let ctx = symbols.current_context();
            let id = match symbols.declare(&name.name, SymbolKind::Local { mutable }, span) {
                Ok(id) => id,
                Err(prev) => {
                    diagnostics.push(FluxError::DuplicateDefinition {
                        name: name.name.clone(),
                        prev_span: span_to_miette(prev.span),
                        span: span_to_miette(span),
                        source_id: span.source,
                    });
                    return None;
                }
            };
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let value = lower_expr(value, symbols, scope, diagnostics, &mut reads_reactive)?;
            Some(vec![HirStmt::Let(HirLet {
                name: id,
                ty: ty.as_ref().map(lower_type),
                value,
                mutable,
                span,
            })])
        }
        LetPattern::Struct {
            fields,
            span: pat_span,
            ..
        } => {
            let ctx = symbols.current_context();
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let base = lower_expr(value, symbols, scope, diagnostics, &mut reads_reactive)?;
            let mut stmts = Vec::new();
            for field in fields {
                let id =
                    match symbols.declare(&field.name, SymbolKind::Local { mutable }, *pat_span) {
                        Ok(id) => id,
                        Err(prev) => {
                            diagnostics.push(FluxError::DuplicateDefinition {
                                name: field.name.clone(),
                                prev_span: span_to_miette(prev.span),
                                span: span_to_miette(*pat_span),
                                source_id: pat_span.source,
                            });
                            return None;
                        }
                    };
                let field_expr = HirExpr::Field {
                    base: Box::new(base.clone()),
                    field: field.name.clone(),
                    span: *pat_span,
                };
                stmts.push(HirStmt::Let(HirLet {
                    name: id,
                    ty: None,
                    value: field_expr,
                    mutable,
                    span: *pat_span,
                }));
            }
            let _ = ty;
            Some(stmts)
        }
    }
}

pub(super) fn lower_match_arm(
    arm: &flux_syntax::MatchArm,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirMatchArm> {
    symbols.push(ScopeKind::Block);
    let (variant, bindings, binding_fields) = match &arm.pattern {
        MatchPattern::Simple(v) => {
            if v.name == "_" {
                (String::new(), vec![], vec![])
            } else if symbols.enum_variants.contains_key(&v.name) {
                (v.name.clone(), vec![], vec![])
            } else {
                let id = symbols
                    .declare(&v.name, SymbolKind::Local { mutable: false }, v.span)
                    .ok()?;
                (String::new(), vec![id], vec![])
            }
        }
        MatchPattern::Tuple {
            variant, bindings, ..
        } => {
            let ids: Vec<SymbolId> = bindings
                .iter()
                .filter_map(|b| {
                    symbols
                        .declare(&b.name, SymbolKind::Local { mutable: false }, b.span)
                        .ok()
                })
                .collect();
            (variant.name.clone(), ids, vec![])
        }
        MatchPattern::Struct {
            variant, fields, ..
        } => {
            let field_names: Vec<String> = fields.iter().map(|f| f.name.clone()).collect();
            let ids: Vec<SymbolId> = fields
                .iter()
                .filter_map(|f| {
                    symbols
                        .declare(&f.name, SymbolKind::Local { mutable: false }, f.span)
                        .ok()
                })
                .collect();
            (variant.name.clone(), ids, field_names)
        }
    };
    let guard = arm
        .guard
        .as_ref()
        .and_then(|g| lower_expr(g, symbols, scope, diagnostics, reads_reactive));
    let expr = lower_expr(&arm.expr, symbols, scope, diagnostics, reads_reactive)?;
    symbols.pop();
    Some(HirMatchArm {
        variant,
        bindings,
        binding_fields,
        guard,
        expr,
    })
}

fn lower_stmt(
    stmt: &Stmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<Vec<HirStmt>> {
    let ctx = symbols.current_context();
    match stmt {
        Stmt::Let(LetStmt {
            mutable,
            pattern,
            ty,
            value,
            span,
            ..
        }) => lower_local_decl(pattern, ty, value, *span, *mutable, symbols, diagnostics),
        Stmt::Assign(AssignStmt { name, value, span }) => {
            let sym = resolve_name(&name.name, *span, symbols, diagnostics)?;
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let value = lower_expr(value, symbols, scope, diagnostics, &mut reads_reactive)?;
            let _ = reads_reactive;
            Some(vec![HirStmt::Assign(HirAssign {
                name: sym,
                value,
                span: *span,
            })])
        }
        Stmt::Emit(EmitStmt { event, args, span }) => {
            let event_id = resolve_name(&event.name, event.span, symbols, diagnostics)?;
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let args = args
                .iter()
                .filter_map(|a| lower_expr(a, symbols, scope, diagnostics, &mut reads_reactive))
                .collect();
            Some(vec![HirStmt::Emit(HirEmit {
                event: event_id,
                args,
                span: *span,
            })])
        }
        Stmt::Return(ReturnStmt { value, span }) => {
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let value = match value {
                Some(v) => Some(lower_expr(
                    v,
                    symbols,
                    scope,
                    diagnostics,
                    &mut reads_reactive,
                )?),
                None => None,
            };
            Some(vec![HirStmt::Return(HirReturn { value, span: *span })])
        }
        Stmt::Break(span) => Some(vec![HirStmt::Break(*span)]),
        Stmt::Continue(span) => Some(vec![HirStmt::Continue(*span)]),
        Stmt::Expr(expr) => {
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            lower_expr(expr, symbols, scope, diagnostics, &mut reads_reactive)
                .map(|e| vec![HirStmt::Expr(e)])
        }
        Stmt::If(s) => lower_if(s, symbols, diagnostics).map(|i| vec![HirStmt::If(i)]),
        Stmt::While(WhileStmt { cond, body, span }) => {
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let cond = lower_expr(cond, symbols, scope, diagnostics, &mut reads_reactive)?;
            let body = lower_block(body, symbols, ScopeKind::Tracking, diagnostics)?;
            Some(vec![HirStmt::While(HirWhile {
                cond,
                body,
                span: *span,
            })])
        }
        Stmt::For(ForStmt {
            name,
            iter,
            body,
            span,
        }) => {
            symbols.push(ScopeKind::Block);
            let id =
                match symbols.declare(&name.name, SymbolKind::Local { mutable: true }, name.span) {
                    Ok(id) => id,
                    Err(prev) => {
                        diagnostics.push(FluxError::DuplicateDefinition {
                            name: name.name.clone(),
                            prev_span: span_to_miette(prev.span),
                            span: span_to_miette(name.span),
                            source_id: name.span.source,
                        });
                        symbols.pop();
                        return None;
                    }
                };
            let _ = id;
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let iter = lower_expr(iter, symbols, scope, diagnostics, &mut reads_reactive)?;
            let body = lower_block(body, symbols, ScopeKind::Tracking, diagnostics)?;
            symbols.pop();
            Some(vec![HirStmt::For(HirFor {
                name: id,
                iter,
                body,
                span: *span,
            })])
        }
        Stmt::Watch(w) => {
            diagnostics.push(FluxError::ReactiveEffectInFunction {
                construct: "watch".into(),
                span: span_to_miette(w.span),
                source_id: w.span.source,
            });
            None
        }
        Stmt::On(o) => {
            diagnostics.push(FluxError::ReactiveEffectInFunction {
                construct: "on".into(),
                span: span_to_miette(o.span),
                source_id: o.span.source,
            });
            None
        }
    }
}

pub(super) fn lower_if(
    stmt: &IfStmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirIf> {
    let ctx = symbols.current_context();
    let mut reads_reactive = false;
    let scope = tracking_scope(ctx);
    let cond = lower_expr(&stmt.cond, symbols, scope, diagnostics, &mut reads_reactive)?;
    let then_branch = lower_block(&stmt.then_branch, symbols, ScopeKind::Tracking, diagnostics)?;
    let else_branch = stmt.else_branch.as_ref().and_then(|e| match e {
        ElseBranch::If(s) => lower_if(s, symbols, diagnostics).map(|i| HirElse::If(Box::new(i))),
        ElseBranch::Block(b) => {
            lower_block(b, symbols, ScopeKind::Tracking, diagnostics).map(HirElse::Block)
        }
    });
    Some(HirIf {
        cond,
        then_branch,
        else_branch,
        span: stmt.span,
    })
}

fn tracking_scope(ctx: crate::CompileContext) -> ScopeKind {
    match ctx {
        crate::CompileContext::Module => ScopeKind::Module,
        crate::CompileContext::PureFn => ScopeKind::Function { reactive: false },
        crate::CompileContext::Tracking => ScopeKind::Tracking,
    }
}
