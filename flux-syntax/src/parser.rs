//! Pest-based parser: `.fl` source → AST.
//!
//! The Pest grammar (`flux.pest`) and generated [`Rule`] enum live here, along
//! with the top-level program/item dispatch and shared span helpers. The actual
//! per-construct parsing is split into submodules (all reachable via `super`):
//! - [`decls`] — declarations (struct/enum/trait/impl/event/fn/binding + params)
//! - [`stmts`] — statements, blocks, handlers, `if`
//! - [`types`] — type annotations
//! - [`exprs`] — expressions, patterns, operator precedence chains

mod decls;
mod exprs;
mod stmts;
mod types;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId, Span};
use pest::error::InputLocation;
use pest::Parser;
use pest_derive::Parser as PestDeriveParser;

use crate::ast::*;

use decls::{
    parse_const_binding, parse_derived_binding, parse_enum, parse_event, parse_fn, parse_impl,
    parse_import, parse_let_binding, parse_rfn, parse_struct, parse_trait, parse_type_alias,
};
use stmts::{parse_if_stmt, parse_on, parse_watch};

#[derive(PestDeriveParser)]
#[grammar = "flux.pest"]
pub struct FluxParser;

pub fn parse_program(
    source_id: SourceId,
    input: &str,
    diagnostics: &mut DiagnosticBag,
) -> Option<Program> {
    let pairs = match FluxParser::parse(Rule::program, input) {
        Ok(mut pairs) => pairs.next()?.into_inner(),
        Err(err) => {
            diagnostics.push(FluxError::Parse {
                // The span already carries the location; pest's `Display` would
                // render a second snippet inside the diagnostic.
                message: err.variant.message().into_owned(),
                span: span_to_miette(span_from_pest_error(source_id, &err)),
                source_id,
            });
            return None;
        }
    };

    let mut items = Vec::new();
    for pair in pairs {
        if let Some((span, literal)) = invalid_integer_literal(source_id, pair.clone(), false) {
            diagnostics.push(FluxError::Parse {
                message: format!(
                    "integer literal `{literal}` is outside the supported range for its type"
                ),
                span: span_to_miette(span),
                source_id,
            });
            return None;
        }
        if let Some(item) = parse_item(source_id, pair) {
            items.push(item);
        }
    }

    Some(Program {
        source: source_id,
        items,
    })
}

fn invalid_integer_literal(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    allow_i64_min_magnitude: bool,
) -> Option<(Span, String)> {
    if pair.as_rule() == Rule::int_lit {
        let (digits, suffix) = exprs::split_numeric_suffix(pair.as_str());
        let magnitude = digits.parse::<u128>().ok();
        let is_i64_min_magnitude = allow_i64_min_magnitude
            && digits == "9223372036854775808"
            && suffix.is_none_or(|suffix| suffix == "i64");
        let fits = match suffix {
            Some("u64") => magnitude.is_some_and(|value| value <= u64::MAX as u128),
            _ => digits.parse::<i64>().is_ok() || is_i64_min_magnitude,
        };
        if !fits {
            return Some((span_of(source_id, &pair), pair.as_str().to_string()));
        }
    }
    let direct_i64_min_operand = if pair.as_rule() == Rule::unary_expr {
        pair.as_str()
            .trim()
            .strip_prefix('-')
            .map(str::trim)
            .is_some_and(|operand| {
                let (digits, suffix) = exprs::split_numeric_suffix(operand);
                digits == "9223372036854775808" && suffix.is_none_or(|suffix| suffix == "i64")
            })
    } else {
        false
    };
    let allow_child_i64_min = allow_i64_min_magnitude || direct_i64_min_operand;
    pair.into_inner()
        .find_map(|child| invalid_integer_literal(source_id, child, allow_child_i64_min))
}

fn parse_item(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Option<Item> {
    let span = span_of(source_id, &pair);
    let inner = match pair.as_rule() {
        Rule::item => pair.into_inner().next()?,
        _ => pair,
    };
    match inner.as_rule() {
        Rule::import_decl => Some(Item::Import(parse_import(source_id, inner, span))),
        Rule::type_alias_decl => Some(Item::TypeAlias(parse_type_alias(source_id, inner, span))),
        Rule::struct_decl => Some(Item::Struct(parse_struct(source_id, inner, span))),
        Rule::enum_decl => Some(Item::Enum(parse_enum(source_id, inner, span))),
        Rule::trait_decl => Some(Item::Trait(parse_trait(source_id, inner, span))),
        Rule::impl_decl => Some(Item::Impl(parse_impl(source_id, inner, span))),
        Rule::event_decl => Some(Item::Event(parse_event(source_id, inner, span))),
        Rule::fn_decl => Some(Item::Fn(parse_fn(source_id, inner, span))),
        Rule::rfn_decl => Some(Item::RFn(parse_rfn(source_id, inner, span))),
        Rule::watch_stmt => Some(Item::Watch(parse_watch(source_id, inner, span))),
        Rule::on_stmt => Some(Item::On(parse_on(source_id, inner, span))),
        Rule::let_binding => Some(Item::Let(parse_let_binding(source_id, inner, span))),
        Rule::derived_binding => Some(Item::Derived(parse_derived_binding(source_id, inner, span))),
        Rule::const_binding => Some(Item::Const(parse_const_binding(source_id, inner, span))),
        Rule::if_stmt => Some(Item::If(parse_if_stmt(source_id, inner, span))),
        _ => None,
    }
}

// ---- Shared helpers (used across the parser submodules) ----

fn parse_ident(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Ident {
    Ident {
        name: pair.as_str().to_string(),
        span: span_of(source_id, &pair),
    }
}

fn span_from_pest_error(source_id: SourceId, err: &pest::error::Error<Rule>) -> Span {
    let (start, end) = match err.location {
        InputLocation::Pos(p) => (p, p.saturating_add(1)),
        InputLocation::Span((s, e)) => (s, e),
    };
    Span {
        source: source_id,
        start,
        end,
    }
}

fn span_of(source_id: SourceId, pair: &pest::iterators::Pair<'_, Rule>) -> Span {
    let start = pair.as_span().start_pos().pos();
    let end = pair.as_span().end_pos().pos();
    Span {
        source: source_id,
        start,
        end,
    }
}

fn merge_span(a: Span, b: Span) -> Span {
    Span {
        source: a.source,
        start: a.start.min(b.start),
        end: a.end.max(b.end),
    }
}
