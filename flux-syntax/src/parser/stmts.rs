//! Statement, block, handler and `if` parsing.

use flux_diagnostics::{SourceId, Span};

use crate::ast::*;

use super::decls::{parse_let_pattern, take_mut};
use super::exprs::{parse_expr, parse_expr_list};
use super::types::parse_type_ann;
use super::{parse_ident, span_of, Rule};

pub(super) fn parse_watch(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> WatchStmt {
    let inner = pair.into_inner();
    let mut target = None;
    let mut body = None;
    for child in inner {
        match child.as_rule() {
            Rule::ident => target = Some(parse_ident(source_id, child)),
            Rule::block => body = Some(parse_block(source_id, child)),
            _ => {}
        }
    }
    WatchStmt {
        target,
        body: body.expect("watch body"),
        span,
    }
}

pub(super) fn parse_on(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> OnStmt {
    let mut inner = pair.into_inner();
    let event = parse_ident(source_id, inner.next().expect("on event"));
    let handler = inner.next().expect("on handler");
    let (param, body) = parse_handler(source_id, handler);
    OnStmt {
        event,
        param,
        body,
        span,
    }
}

fn parse_handler(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> (Option<Ident>, Block) {
    let inner = pair.into_inner().next().expect("handler inner");
    match inner.as_rule() {
        Rule::handler_body => {
            let mut parts = inner.into_inner();
            let first = parts.next().expect("handler body part");
            match first.as_rule() {
                Rule::handler_param => {
                    let param = parse_ident(source_id, first.into_inner().next().unwrap());
                    let stmts = parts.next().expect("handler stmts");
                    let stmts_span = span_of(source_id, &stmts);
                    (Some(param), stmts_to_block(source_id, stmts, stmts_span))
                }
                Rule::block_stmts => {
                    let block_span = span_of(source_id, &first);
                    (None, stmts_to_block(source_id, first, block_span))
                }
                _ => panic!("unexpected handler body"),
            }
        }
        _ => panic!("expected handler_body"),
    }
}

pub(super) fn parse_block(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Block {
    let span = span_of(source_id, &pair);
    let stmts_pair = pair.into_inner().next().expect("block stmts");
    stmts_to_block(source_id, stmts_pair, span)
}

fn stmts_to_block(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>, span: Span) -> Block {
    Block {
        stmts: pair
            .into_inner()
            .filter_map(|p| parse_stmt(source_id, p))
            .collect(),
        span,
    }
}

fn parse_stmt(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Option<Stmt> {
    let pair = match pair.as_rule() {
        Rule::stmt => pair.into_inner().next()?,
        _ => pair,
    };
    let span = span_of(source_id, &pair);
    match pair.as_rule() {
        Rule::let_stmt => {
            let mut inner = pair.into_inner().peekable();
            let mutable = take_mut(&mut inner);
            let pattern = parse_let_pattern(source_id, inner.next().expect("let pattern"));
            let mut ty = None;
            let mut value = None;
            for child in inner {
                match child.as_rule() {
                    Rule::type_ann => ty = Some(parse_type_ann(source_id, child)),
                    Rule::expr => value = Some(parse_expr(source_id, child)),
                    _ => {}
                }
            }
            Some(Stmt::Let(LetStmt {
                pub_: false,
                mutable,
                pattern,
                ty,
                value: value.expect("let value"),
                span,
            }))
        }
        Rule::assign_stmt => {
            let mut inner = pair.into_inner();
            let name = parse_ident(source_id, inner.next().expect("assign name"));
            let value = parse_expr(source_id, inner.next().expect("assign value"));
            Some(Stmt::Assign(AssignStmt { name, value, span }))
        }
        Rule::emit_stmt => {
            let mut inner = pair.into_inner();
            let event = parse_ident(source_id, inner.next().expect("emit event"));
            let args = inner
                .next()
                .map(|emit_args| {
                    emit_args
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::expr_list)
                        .map(|p| parse_expr_list(source_id, p))
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            Some(Stmt::Emit(EmitStmt { event, args, span }))
        }
        Rule::expr_stmt => {
            let expr = parse_expr(source_id, pair.into_inner().next().expect("expr"));
            Some(Stmt::Expr(expr))
        }
        Rule::if_stmt => Some(Stmt::If(parse_if_stmt(source_id, pair, span))),
        Rule::while_stmt => {
            let mut inner = pair.into_inner();
            let cond = parse_expr(source_id, inner.next().expect("while cond"));
            let body = parse_block(source_id, inner.next().expect("while body"));
            Some(Stmt::While(WhileStmt { cond, body, span }))
        }
        Rule::for_stmt => {
            let mut inner = pair.into_inner();
            let name = parse_ident(source_id, inner.next().expect("for name"));
            let iter = parse_expr(source_id, inner.next().expect("for iter"));
            let body = parse_block(source_id, inner.next().expect("for body"));
            Some(Stmt::For(ForStmt {
                name,
                iter,
                body,
                span,
            }))
        }
        Rule::watch_stmt => Some(Stmt::Watch(parse_watch(source_id, pair, span))),
        Rule::on_stmt => Some(Stmt::On(parse_on(source_id, pair, span))),
        Rule::return_stmt => {
            let value = pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::expr)
                .map(|p| parse_expr(source_id, p));
            Some(Stmt::Return(ReturnStmt { value, span }))
        }
        Rule::break_stmt => Some(Stmt::Break(span)),
        Rule::continue_stmt => Some(Stmt::Continue(span)),
        _ => None,
    }
}

pub(super) fn parse_if_stmt(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> IfStmt {
    let mut inner = pair.into_inner();
    let cond = parse_expr(source_id, inner.next().expect("if cond"));
    let then_branch = parse_block(source_id, inner.next().expect("if then"));
    let else_branch = inner.next().map(|p| {
        // `else_clause = { "else" ~ (if_stmt | block) }`: branch on its child.
        let p = if p.as_rule() == Rule::else_clause {
            p.into_inner().next().expect("else body")
        } else {
            p
        };
        let else_span = span_of(source_id, &p);
        match p.as_rule() {
            Rule::if_stmt => ElseBranch::If(Box::new(parse_if_stmt(source_id, p, else_span))),
            _ => ElseBranch::Block(parse_block(source_id, p)),
        }
    });
    IfStmt {
        cond,
        then_branch,
        else_branch,
        span,
    }
}
