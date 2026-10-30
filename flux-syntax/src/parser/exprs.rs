//! Expression parsing: precedence chains, unary/postfix, literals, closures,
//! match expressions and match patterns.

use flux_diagnostics::SourceId;

use crate::ast::*;

use super::decls::parse_closure_param;
use super::{merge_span, parse_ident, span_of, Rule};

pub(super) fn parse_expr(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    match pair.as_rule() {
        Rule::expr => parse_expr(source_id, pair.into_inner().next().expect("expr inner")),
        Rule::match_expr => {
            let span = span_of(source_id, &pair);
            let mut inner = pair.into_inner();
            let scrutinee = Box::new(parse_expr(
                source_id,
                inner.next().expect("match scrutinee"),
            ));
            let arms = inner
                .map(|arm| {
                    let asp = span_of(source_id, &arm);
                    let mut ai = arm.into_inner();
                    let pattern_pair = ai.next().expect("arm pattern");
                    let pattern = parse_match_pattern(source_id, pattern_pair);
                    let mut guard = None;
                    let mut arm_expr = None;
                    for child in ai {
                        match child.as_rule() {
                            Rule::guard => {
                                guard = Some(parse_expr(
                                    source_id,
                                    child.into_inner().next().expect("guard expr"),
                                ));
                            }
                            Rule::expr => arm_expr = Some(parse_expr(source_id, child)),
                            _ => {}
                        }
                    }
                    MatchArm {
                        pattern,
                        guard,
                        expr: arm_expr.expect("arm expr"),
                        span: asp,
                    }
                })
                .collect();
            Expr::Match {
                scrutinee,
                arms,
                span,
            }
        }
        Rule::if_expr => parse_if_expr(source_id, pair),
        Rule::or_expr => parse_chained_binary(source_id, pair, "||", BinaryOp::Or),
        Rule::and_expr => parse_chained_binary(source_id, pair, "&&", BinaryOp::And),
        Rule::eq_expr => parse_chained_eq(source_id, pair),
        Rule::rel_expr => parse_chained_rel(source_id, pair),
        Rule::add_expr => parse_chained_add(source_id, pair),
        Rule::mul_expr => parse_chained_mul(source_id, pair),
        Rule::cast_expr => parse_cast_expr(source_id, pair),
        Rule::unary_expr => parse_unary_expr(source_id, pair),
        Rule::postfix_expr => parse_postfix_expr(source_id, pair),
        Rule::primary_expr => parse_expr(source_id, pair.into_inner().next().expect("primary")),
        Rule::closure_expr => {
            let span = span_of(source_id, &pair);
            let mut inner = pair.into_inner();
            let params = inner
                .find(|p| p.as_rule() == Rule::closure_params)
                .map(|p| {
                    p.into_inner()
                        .map(|param| parse_closure_param(source_id, param))
                        .collect()
                })
                .unwrap_or_default();
            let body = Box::new(parse_expr(
                source_id,
                inner
                    .find(|p| p.as_rule() == Rule::expr)
                    .expect("closure body"),
            ));
            Expr::Closure { params, body, span }
        }
        Rule::array_lit => {
            let span = span_of(source_id, &pair);
            let elems = pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::expr_list)
                .map(|p| parse_expr_list(source_id, p))
                .unwrap_or_default();
            Expr::Array { elems, span }
        }
        Rule::struct_lit => {
            let span = span_of(source_id, &pair);
            let mut inner = pair.into_inner();
            let name = parse_ident(source_id, inner.next().expect("struct name"));
            let fields = inner
                .map(|f| {
                    let fspan = span_of(source_id, &f);
                    let mut fi = f.into_inner();
                    let fname = parse_ident(source_id, fi.next().expect("field name"));
                    let value = parse_expr(source_id, fi.next().expect("field value"));
                    FieldInit {
                        name: fname,
                        value,
                        span: fspan,
                    }
                })
                .collect();
            Expr::StructLit { name, fields, span }
        }
        Rule::float_lit => {
            let span = span_of(source_id, &pair);
            let text = pair.as_str();
            let (digits, suffix) = split_numeric_suffix(text);
            let value = Expr::Float {
                value: digits.parse().expect("float literal"),
                span,
            };
            suffix
                .and_then(suffix_type)
                .map(|ty| Expr::Cast {
                    expr: Box::new(value.clone()),
                    ty: ty.into(),
                    span,
                })
                .unwrap_or(value)
        }
        Rule::int_lit => {
            let span = span_of(source_id, &pair);
            let text = pair.as_str();
            let (digits, suffix) = split_numeric_suffix(text);
            let value = Expr::Int {
                value: digits
                    .parse()
                    .expect("integer literal within supported range"),
                span,
            };
            suffix
                .and_then(suffix_type)
                .map(|ty| Expr::Cast {
                    expr: Box::new(value.clone()),
                    ty: ty.into(),
                    span,
                })
                .unwrap_or(value)
        }
        Rule::string_lit => parse_string_lit(source_id, pair),
        Rule::bool_lit => {
            let span = span_of(source_id, &pair);
            Expr::Bool {
                value: pair.as_str() == "true",
                span,
            }
        }
        Rule::ident => {
            let span = span_of(source_id, &pair);
            Expr::Ident {
                name: pair.as_str().to_string(),
                span,
            }
        }
        other => panic!("unexpected expr rule: {other:?}"),
    }
}

fn parse_match_pattern(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> MatchPattern {
    let pair = match pair.as_rule() {
        Rule::match_pattern => pair.into_inner().next().expect("match pattern inner"),
        _ => pair,
    };
    let span = span_of(source_id, &pair);
    match pair.as_rule() {
        Rule::simple_pat => {
            let ident = parse_ident(source_id, pair.into_inner().next().expect("simple pat"));
            MatchPattern::Simple(ident)
        }
        Rule::tuple_pat => {
            let mut inner = pair.into_inner();
            let variant = parse_ident(source_id, inner.next().expect("tuple variant"));
            let bindings = inner.map(|p| parse_ident(source_id, p)).collect();
            MatchPattern::Tuple {
                variant,
                bindings,
                span,
            }
        }
        Rule::struct_pat => {
            let mut inner = pair.into_inner();
            let variant = parse_ident(source_id, inner.next().expect("struct variant"));
            let fields = inner
                .next()
                .map(|list| match list.as_rule() {
                    Rule::ident_list => list
                        .into_inner()
                        .map(|p| parse_ident(source_id, p))
                        .collect(),
                    Rule::ident => vec![parse_ident(source_id, list)],
                    _ => list
                        .into_inner()
                        .filter(|p| p.as_rule() == Rule::ident)
                        .map(|p| parse_ident(source_id, p))
                        .collect(),
                })
                .unwrap_or_default();
            MatchPattern::Struct {
                variant,
                fields,
                span,
            }
        }
        _ => unreachable!("match pattern"),
    }
}

fn parse_chained_binary(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    _op_token: &str,
    op: BinaryOp,
) -> Expr {
    let mut inner = pair.into_inner();
    let first = inner.next().expect("binary lhs");
    let mut expr = parse_expr(source_id, first);
    for rhs in inner {
        let rhs = parse_expr(source_id, rhs);
        let span = merge_span(expr.span(), rhs.span());
        expr = Expr::Binary {
            op,
            left: Box::new(expr),
            right: Box::new(rhs),
            span,
        };
    }
    expr
}

fn parse_chained_eq(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let mut expr = parse_expr(source_id, inner.next().expect("eq lhs"));
    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "==" => BinaryOp::Eq,
            "!=" => BinaryOp::Ne,
            _ => continue,
        };
        let rhs = parse_expr(source_id, inner.next().expect("eq rhs"));
        let span = merge_span(expr.span(), rhs.span());
        expr = Expr::Binary {
            op,
            left: Box::new(expr),
            right: Box::new(rhs),
            span,
        };
    }
    expr
}

fn parse_chained_rel(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let mut expr = parse_expr(source_id, inner.next().expect("rel lhs"));
    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "<=" => BinaryOp::Le,
            ">=" => BinaryOp::Ge,
            "<" => BinaryOp::Lt,
            ">" => BinaryOp::Gt,
            _ => continue,
        };
        let rhs = parse_expr(source_id, inner.next().expect("rel rhs"));
        let span = merge_span(expr.span(), rhs.span());
        expr = Expr::Binary {
            op,
            left: Box::new(expr),
            right: Box::new(rhs),
            span,
        };
    }
    expr
}

fn parse_chained_add(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let mut expr = parse_expr(source_id, inner.next().expect("add lhs"));
    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            _ => continue,
        };
        let rhs = parse_expr(source_id, inner.next().expect("add rhs"));
        let span = merge_span(expr.span(), rhs.span());
        expr = Expr::Binary {
            op,
            left: Box::new(expr),
            right: Box::new(rhs),
            span,
        };
    }
    expr
}

pub(super) fn split_numeric_suffix(text: &str) -> (&str, Option<&str>) {
    const SUFFIXES: &[&str] = &[
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
    ];
    for sfx in SUFFIXES {
        if let Some(stem) = text.strip_suffix(sfx) {
            if !stem.is_empty() {
                return (stem, Some(*sfx));
            }
        }
    }
    (text, None)
}

fn suffix_type(suffix: &str) -> Option<&'static str> {
    Some(match suffix {
        "i8" => "Int8",
        "i16" => "Int16",
        "i32" => "Int32",
        "i64" => "Int64",
        "u8" => "UInt8",
        "u16" => "UInt16",
        "u32" => "UInt32",
        "u64" => "UInt64",
        "f32" => "Float32",
        "f64" => "Float64",
        _ => return None,
    })
}

fn parse_cast_expr(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let mut expr = parse_expr(source_id, inner.next().expect("cast base"));
    for ty_pair in inner {
        if ty_pair.as_rule() == Rule::type_name {
            let ty = ty_pair.as_str().to_string();
            let span = merge_span(expr.span(), span_of(source_id, &ty_pair));
            expr = Expr::Cast {
                expr: Box::new(expr),
                ty,
                span,
            };
        }
    }
    expr
}

fn parse_chained_mul(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let mut expr = parse_expr(source_id, inner.next().expect("mul lhs"));
    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "%" => BinaryOp::Rem,
            _ => continue,
        };
        let rhs = parse_expr(source_id, inner.next().expect("mul rhs"));
        let span = merge_span(expr.span(), rhs.span());
        expr = Expr::Binary {
            op,
            left: Box::new(expr),
            right: Box::new(rhs),
            span,
        };
    }
    expr
}

fn parse_unary_expr(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let unary_span = span_of(source_id, &pair);
    let mut inner = pair.into_inner();
    let first = inner.next().expect("unary");
    match first.as_rule() {
        Rule::unary_op => {
            let op = match first.as_str() {
                "-" => UnaryOp::Neg,
                "!" => UnaryOp::Not,
                other => panic!("unknown unary op: {other}"),
            };
            let expr_pair = inner.next().expect("unary expr");
            if op == UnaryOp::Neg {
                let (digits, suffix) = split_numeric_suffix(expr_pair.as_str().trim());
                if digits == "9223372036854775808" && suffix.is_none_or(|suffix| suffix == "i64") {
                    let span = unary_span;
                    let value = Expr::Int {
                        value: i64::MIN as i128,
                        span,
                    };
                    return suffix
                        .and_then(suffix_type)
                        .map(|ty| Expr::Cast {
                            expr: Box::new(value.clone()),
                            ty: ty.into(),
                            span,
                        })
                        .unwrap_or(value);
                }
            }
            let expr = parse_expr(source_id, expr_pair);
            let span = merge_span(span_of(source_id, &first), expr.span());
            Expr::Unary {
                op,
                expr: Box::new(expr),
                span,
            }
        }
        _ => parse_expr(source_id, first),
    }
}

fn parse_postfix_expr(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let mut inner = pair.into_inner();
    let primary = parse_expr(source_id, inner.next().expect("postfix primary"));
    let mut expr = primary;
    for suffix in inner {
        if suffix.as_rule() == Rule::call_suffix {
            let call_span = span_of(source_id, &suffix);
            let args = suffix
                .into_inner()
                .next()
                .map(|call_args| {
                    call_args
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::expr_list)
                        .map(|p| parse_expr_list(source_id, p))
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let span = merge_span(expr.span(), call_span);
            expr = Expr::Call {
                callee: Box::new(expr),
                args,
                span,
            };
        } else if suffix.as_rule() == Rule::field_suffix {
            let field_span = span_of(source_id, &suffix);
            let field = suffix
                .into_inner()
                .find(|p| p.as_rule() == Rule::ident)
                .map(|p| parse_ident(source_id, p).name)
                .unwrap_or_default();
            let span = merge_span(expr.span(), field_span);
            expr = Expr::Field {
                base: Box::new(expr),
                field,
                span,
            };
        }
    }
    expr
}

fn parse_if_expr(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let span = span_of(source_id, &pair);
    let mut inner = pair.into_inner();
    let cond = parse_expr(source_id, inner.next().expect("if cond"));
    let then_expr = parse_expr(source_id, inner.next().expect("if then"));
    let else_expr = parse_expr(source_id, inner.next().expect("if else"));
    Expr::If {
        cond: Box::new(cond),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
        span,
    }
}

pub(super) fn parse_expr_list(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> Vec<Expr> {
    pair.into_inner()
        .map(|p| parse_expr(source_id, p))
        .collect()
}

fn parse_string_lit(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let span = span_of(source_id, &pair);
    let mut parts = Vec::new();
    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::string_content => {
                for piece in child.into_inner() {
                    match piece.as_rule() {
                        Rule::string_char => {
                            parts.push(StringPart::Text(piece.as_str().to_string()));
                        }
                        Rule::interp_expr => {
                            let ident = piece.into_inner().next().expect("interp ident");
                            parts.push(StringPart::Ident(ident.as_str().to_string()));
                        }
                        _ => {}
                    }
                }
            }
            Rule::string_char => {
                parts.push(StringPart::Text(child.as_str().to_string()));
            }
            Rule::interp_expr => {
                let ident = child.into_inner().next().expect("interp ident");
                parts.push(StringPart::Ident(ident.as_str().to_string()));
            }
            _ => {}
        }
    }
    let mut merged = Vec::new();
    for part in parts {
        match (&mut merged.last_mut(), part) {
            (Some(StringPart::Text(acc)), StringPart::Text(text)) => acc.push_str(&text),
            (_, part) => merged.push(part),
        }
    }
    if merged.len() == 1 {
        if let StringPart::Text(text) = &merged[0] {
            return Expr::String {
                value: text.clone(),
                span,
            };
        }
    }
    Expr::InterpolatedString {
        parts: merged,
        span,
    }
}
