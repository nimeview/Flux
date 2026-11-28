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
            let mut inner = pair.into_inner().filter(|p| p.as_rule() != Rule::kw_match);
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
                    let mut stmts = Vec::new();
                    for child in ai {
                        match child.as_rule() {
                            Rule::guard => {
                                guard = Some(parse_expr(
                                    source_id,
                                    child
                                        .into_inner()
                                        .find(|p| p.as_rule() != Rule::kw_if)
                                        .expect("guard expr"),
                                ));
                            }
                            Rule::expr => arm_expr = Some(parse_expr(source_id, child)),
                            Rule::arm_block => {
                                // A final expression is the arm's value;
                                // otherwise the arm leaves or is `()`. (A
                                // final `if … else` stays a statement: in a
                                // function, lowering takes its value with
                                // no numeric widening.)
                                let mut body = super::stmts::parse_block(source_id, child).stmts;
                                if let Some(Stmt::Expr(_)) = body.last() {
                                    if let Some(Stmt::Expr(tail)) = body.pop() {
                                        arm_expr = Some(tail);
                                    }
                                }
                                stmts = body;
                            }
                            Rule::arm_jump => {
                                let jump = child.into_inner().next().expect("arm jump");
                                stmts = super::stmts::parse_stmt(source_id, jump)
                                    .into_iter()
                                    .collect();
                            }
                            _ => {}
                        }
                    }
                    MatchArm {
                        pattern,
                        guard,
                        stmts,
                        expr: arm_expr,
                        span: asp,
                    }
                })
                .collect();
            Expr::Match {
                scrutinee,
                arms,
                if_form: false,
                span,
            }
        }
        Rule::if_expr => parse_if_expr(source_id, pair),
        Rule::or_expr => parse_chained_binary(source_id, pair, "||", BinaryOp::Or),
        Rule::and_expr => parse_chained_binary(source_id, pair, "&&", BinaryOp::And),
        Rule::eq_expr => parse_chained_eq(source_id, pair),
        Rule::rel_expr => parse_chained_rel(source_id, pair),
        Rule::add_expr => parse_chained_bits(source_id, pair),
        Rule::mul_expr => parse_chained_mul(source_id, pair),
        Rule::cast_expr => parse_cast_expr(source_id, pair),
        Rule::unary_expr => parse_unary_expr(source_id, pair),
        Rule::postfix_expr => parse_postfix_expr(source_id, pair),
        Rule::primary_expr => parse_expr(source_id, pair.into_inner().next().expect("primary")),
        Rule::closure_expr => {
            let span = span_of(source_id, &pair);
            let mut params = Vec::new();
            let mut body = None;
            for part in pair.into_inner() {
                match part.as_rule() {
                    Rule::closure_params => {
                        params = part
                            .into_inner()
                            .map(|param| parse_closure_param(source_id, param))
                            .collect();
                    }
                    _ => body = Some(part),
                }
            }
            let body = Box::new(parse_expr(source_id, body.expect("closure body")));
            Expr::Closure { params, body, span }
        }
        Rule::paren_expr => {
            let span = span_of(source_id, &pair);
            let mut inner = pair.into_inner();
            let first = parse_expr(source_id, inner.next().expect("parenthesized expr"));
            match inner.next() {
                None => first,
                Some(tail) => {
                    let mut elems = vec![first];
                    elems.extend(tail.into_inner().map(|p| parse_expr(source_id, p)));
                    Expr::Tuple { elems, span }
                }
            }
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
                value: digits.replace('_', "").parse().expect("float literal"),
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
                value: decimal_digits(digits)
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
        Rule::raw_string_lit => Expr::String {
            value: pair
                .clone()
                .into_inner()
                .find(|p| p.as_rule() == Rule::raw_string_text)
                .map(|p| p.as_str().to_string())
                .unwrap_or_default(),
            span: span_of(source_id, &pair),
        },
        Rule::string_lit => parse_string_lit(source_id, pair),
        Rule::bool_lit => {
            let span = span_of(source_id, &pair);
            Expr::Bool {
                value: pair.as_str() == "true",
                span,
            }
        }
        Rule::ident | Rule::path_ident => {
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
        Rule::lit_pat => {
            let lit = pair.into_inner().next().expect("literal pattern");
            let value = if lit.as_rule() == Rule::neg_int_lit {
                let inner = parse_expr(source_id, lit.into_inner().next().expect("int"));
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(inner),
                    span,
                }
            } else {
                parse_expr(source_id, lit)
            };
            MatchPattern::Literal(value)
        }
        Rule::simple_pat => {
            let ident = parse_ident(source_id, pair.into_inner().next().expect("simple pat"));
            MatchPattern::Simple(ident)
        }
        Rule::tuple_value_pat => MatchPattern::TupleValue {
            elems: pair
                .into_inner()
                .map(|p| parse_match_pattern(source_id, p))
                .collect(),
            span,
        },
        Rule::tuple_pat => {
            let mut inner = pair.into_inner();
            let variant = parse_ident(source_id, inner.next().expect("tuple variant"));
            let (bindings, nested) = match inner.next() {
                None => (vec![], None),
                Some(p) if p.as_rule() == Rule::ident_list => (
                    p.into_inner().map(|i| parse_ident(source_id, i)).collect(),
                    None,
                ),
                Some(p) if p.as_rule() == Rule::ident => (vec![parse_ident(source_id, p)], None),
                // `Some(Shape::Dot)`: a payload-free variant.
                Some(p) if p.as_rule() == Rule::path_ident => (
                    vec![],
                    Some(Box::new(MatchPattern::Simple(parse_ident(source_id, p)))),
                ),
                Some(p) => (vec![], Some(Box::new(parse_match_pattern(source_id, p)))),
            };
            MatchPattern::Tuple {
                variant,
                bindings,
                nested,
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

pub fn split_numeric_suffix(text: &str) -> (&str, Option<&str>) {
    const SUFFIXES: &[&str] = &[
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
    ];
    // In `0xf32`, the `f` is a digit.
    let is_hex = text.starts_with("0x");
    for sfx in SUFFIXES {
        if is_hex && sfx.starts_with('f') {
            continue;
        }
        if let Some(stem) = text.strip_suffix(sfx) {
            if !stem.is_empty() {
                return (stem, Some(*sfx));
            }
        }
    }
    (text, None)
}

/// An integer literal's digits in decimal, without `_` or a `0x` prefix. A value
/// too large for `u128` comes back as a number no integer type can hold.
pub(super) fn decimal_digits(digits: &str) -> String {
    let plain = digits.replace('_', "");
    match plain.strip_prefix("0x") {
        Some(hex) => u128::from_str_radix(hex, 16)
            .map(|value| value.to_string())
            .unwrap_or_else(|_| format!("{}0", u128::MAX)),
        None => plain,
    }
}

/// The character a `\x` escape in a string literal stands for, if `x` is one
/// of `n t r 0 \\ " { }`.
pub(super) fn unescape(escape: &str) -> Option<char> {
    Some(match escape.strip_prefix('\\')? {
        "n" => '\n',
        "t" => '\t',
        "r" => '\r',
        "0" => '\0',
        "\\" => '\\',
        "\"" => '"',
        "{" => '{',
        "}" => '}',
        _ => return None,
    })
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

/// `a | b`, `a ^ b`, `a & b`, `a << n`, `a >> n`: left to right in each level.
/// `a | b ^ c & d << e + f`: the operands and operators in order, combined
/// by precedence (`+` and `-`, then shifts, `&`, `^`, `|`), each left to
/// right.
fn parse_chained_bits(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    fn precedence(op: BinaryOp) -> u8 {
        match op {
            BinaryOp::Add | BinaryOp::Sub => 5,
            BinaryOp::Shl | BinaryOp::Shr => 4,
            BinaryOp::BitAnd => 3,
            BinaryOp::BitXor => 2,
            _ => 1,
        }
    }
    fn combine(operands: &mut Vec<Expr>, op: BinaryOp) {
        let right = operands.pop().expect("bit rhs");
        let left = operands.pop().expect("bit lhs");
        let span = merge_span(left.span(), right.span());
        operands.push(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
            span,
        });
    }
    let mut inner = pair.into_inner();
    let mut operands = vec![parse_expr(source_id, inner.next().expect("bit lhs"))];
    let mut ops: Vec<BinaryOp> = Vec::new();
    while let Some(op_pair) = inner.next() {
        let op = match op_pair.as_str() {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "|" => BinaryOp::BitOr,
            "^" => BinaryOp::BitXor,
            "&" => BinaryOp::BitAnd,
            "<<" => BinaryOp::Shl,
            ">>" => BinaryOp::Shr,
            _ => continue,
        };
        while ops
            .last()
            .is_some_and(|top| precedence(*top) >= precedence(op))
        {
            let top = ops.pop().expect("operator");
            combine(&mut operands, top);
        }
        ops.push(op);
        operands.push(parse_expr(source_id, inner.next().expect("bit rhs")));
    }
    while let Some(op) = ops.pop() {
        combine(&mut operands, op);
    }
    operands.pop().expect("bit expression")
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
                if decimal_digits(digits) == "9223372036854775808"
                    && suffix.is_none_or(|suffix| suffix == "i64")
                {
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
                // `-128i8` is the literal -128 of type Int8, not the negation
                // of 128i8 (which does not fit).
                let text = expr_pair.as_str().trim();
                let (digits, suffix) = split_numeric_suffix(text);
                let ty = suffix
                    .and_then(suffix_type)
                    .filter(|t| !t.starts_with("Float"));
                if let (Some(ty), true) = (
                    ty,
                    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '_'),
                ) {
                    if let Ok(n) = decimal_digits(digits).parse::<i128>() {
                        let span = unary_span;
                        return Expr::Cast {
                            expr: Box::new(Expr::Int { value: -n, span }),
                            ty: ty.into(),
                            span,
                        };
                    }
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
    let primary_pair = inner.next().expect("postfix primary");
    let primary_span = span_of(source_id, &primary_pair);
    // `(x)` with no tuple tail: parentheses that only group.
    let grouped = primary_pair
        .clone()
        .into_inner()
        .next()
        .is_some_and(|p| p.as_rule() == Rule::paren_expr && p.into_inner().count() == 1);
    let primary = parse_expr(source_id, primary_pair);
    let mut expr = primary;
    let mut first = true;
    for suffix in inner {
        // `(op.f)(3)` calls the function in field `f`; the parentheses are
        // kept so that it stays apart from the method call `op.f(3)`.
        let keep_parens = std::mem::take(&mut first)
            && grouped
            && suffix.as_rule() == Rule::call_suffix
            && matches!(expr, Expr::Field { .. });
        if keep_parens {
            expr = Expr::Paren {
                inner: Box::new(expr),
                span: primary_span,
            };
        }
        if suffix.as_rule() == Rule::call_suffix {
            let call_span = span_of(source_id, &suffix);
            let args = suffix
                .into_inner()
                .next()
                .map(|call_args| {
                    call_args
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::arg_list)
                        .map(|list| {
                            list.into_inner()
                                .map(|arg| match arg.as_rule() {
                                    Rule::mut_arg => {
                                        let span = span_of(source_id, &arg);
                                        let value = arg
                                            .into_inner()
                                            .find(|p| p.as_rule() != Rule::kw_mut)
                                            .expect("mut argument");
                                        Expr::MutArg {
                                            expr: Box::new(parse_expr(source_id, value)),
                                            span,
                                        }
                                    }
                                    _ => parse_expr(source_id, arg),
                                })
                                .collect()
                        })
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
                .find(|p| matches!(p.as_rule(), Rule::ident | Rule::tuple_index))
                .map(|p| p.as_str().to_string())
                .unwrap_or_default();
            let span = merge_span(expr.span(), field_span);
            expr = Expr::Field {
                base: Box::new(expr),
                field,
                span,
            };
        } else if suffix.as_rule() == Rule::index_suffix {
            let span = merge_span(expr.span(), span_of(source_id, &suffix));
            let index = parse_expr(source_id, suffix.into_inner().next().expect("index expr"));
            expr = Expr::Index {
                base: Box::new(expr),
                index: Box::new(index),
                span,
            };
        } else if suffix.as_rule() == Rule::with_suffix {
            let span = merge_span(expr.span(), span_of(source_id, &suffix));
            let fields = suffix
                .into_inner()
                .filter(|p| p.as_rule() == Rule::field_init)
                .map(|f| {
                    let fspan = span_of(source_id, &f);
                    let mut fi = f.into_inner();
                    let name = parse_ident(source_id, fi.next().expect("field name"));
                    let value = parse_expr(source_id, fi.next().expect("field value"));
                    FieldInit {
                        name,
                        value,
                        span: fspan,
                    }
                })
                .collect();
            expr = Expr::With {
                base: Box::new(expr),
                fields,
                span,
            };
        } else if suffix.as_rule() == Rule::try_suffix {
            let span = merge_span(expr.span(), span_of(source_id, &suffix));
            expr = Expr::Try {
                expr: Box::new(expr),
                span,
            };
        }
    }
    expr
}

fn parse_if_expr(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Expr {
    let span = span_of(source_id, &pair);
    let mut inner = pair
        .into_inner()
        .filter(|p| !matches!(p.as_rule(), Rule::kw_if | Rule::kw_else));
    let cond = parse_expr(source_id, inner.next().expect("if cond"));
    let then_branch = parse_if_branch(source_id, inner.next().expect("if then"));
    let else_pair = inner.next().expect("if else");
    let else_branch = match else_pair.as_rule() {
        Rule::if_expr => (Vec::new(), Some(parse_if_expr(source_id, else_pair))),
        _ => parse_if_branch(source_id, else_pair),
    };
    value_if(cond, then_branch, else_branch, span)
}

/// A value `if` from its condition and branches, each its statements and
/// final expression: a plain `Expr::If` when no branch has statements,
/// otherwise a match on the condition (see [`Expr::Match`]'s `if_form`).
fn value_if(
    cond: Expr,
    then_branch: (Vec<Stmt>, Option<Expr>),
    else_branch: (Vec<Stmt>, Option<Expr>),
    span: flux_diagnostics::Span,
) -> Expr {
    match (then_branch, else_branch) {
        // `if c { a } else { b }`, `… else if …`: a plain value `if`.
        ((then_stmts, Some(then_expr)), (else_stmts, Some(else_expr)))
            if then_stmts.is_empty() && else_stmts.is_empty() =>
        {
            Expr::If {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
                span,
            }
        }
        // A branch with statements, or one that leaves (`return`): the
        // arms `true` and `_` of a match on the condition, which already
        // run statements before their value.
        ((then_stmts, then_expr), (else_stmts, else_expr)) => {
            let cond_span = cond.span();
            Expr::Match {
                scrutinee: Box::new(cond),
                arms: vec![
                    MatchArm {
                        pattern: MatchPattern::Literal(Expr::Bool {
                            value: true,
                            span: cond_span,
                        }),
                        guard: None,
                        stmts: then_stmts,
                        expr: then_expr,
                        span,
                    },
                    MatchArm {
                        pattern: MatchPattern::Simple(Ident {
                            name: "_".into(),
                            span: cond_span,
                        }),
                        guard: None,
                        stmts: else_stmts,
                        expr: else_expr,
                        span,
                    },
                ],
                if_form: true,
                span,
            }
        }
    }
}

/// A branch `{ stmts… expr }` of a value `if`: its statements, and its final
/// expression (none when it leaves or ends in a statement).
fn parse_if_branch(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> (Vec<Stmt>, Option<Expr>) {
    split_tail(super::stmts::parse_block(source_id, pair).stmts)
}

/// A block's statements and its final expression. A last `if` with an
/// `else` at every level is the block's value too (`else { if a { 1 } else
/// { 2 } }`), as at the end of a function.
fn split_tail(mut stmts: Vec<Stmt>) -> (Vec<Stmt>, Option<Expr>) {
    let tail = match stmts.pop() {
        Some(Stmt::Expr(e)) => Some(e),
        Some(Stmt::If(i)) if has_else_everywhere(&i) => Some(if_stmt_value(i)),
        Some(other) => {
            stmts.push(other);
            None
        }
        None => None,
    };
    (stmts, tail)
}

fn has_else_everywhere(i: &IfStmt) -> bool {
    match &i.else_branch {
        Some(ElseBranch::Block(_)) => true,
        Some(ElseBranch::If(inner)) => has_else_everywhere(inner),
        None => false,
    }
}

/// `if c { … } else …` written as a statement, as a value.
fn if_stmt_value(i: IfStmt) -> Expr {
    let then_branch = split_tail(i.then_branch.stmts);
    let else_branch = match i.else_branch {
        Some(ElseBranch::If(inner)) => (Vec::new(), Some(if_stmt_value(*inner))),
        Some(ElseBranch::Block(block)) => split_tail(block.stmts),
        None => (Vec::new(), None),
    };
    value_if(i.cond, then_branch, else_branch, i.span)
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
                        Rule::string_escape => {
                            let c = unescape(piece.as_str()).unwrap_or('\u{FFFD}');
                            parts.push(StringPart::Text(c.to_string()));
                        }
                        Rule::interp_expr => {
                            let inner = piece.into_inner().next().expect("interp expr");
                            parts.push(StringPart::Expr(parse_expr(source_id, inner)));
                        }
                        _ => {}
                    }
                }
            }
            Rule::string_char => {
                parts.push(StringPart::Text(child.as_str().to_string()));
            }
            Rule::string_escape => {
                // Unknown escapes are rejected earlier (`parser::invalid_escape`).
                let c = unescape(child.as_str()).unwrap_or('\u{FFFD}');
                parts.push(StringPart::Text(c.to_string()));
            }
            Rule::interp_expr => {
                let inner = child.into_inner().next().expect("interp expr");
                parts.push(StringPart::Expr(parse_expr(source_id, inner)));
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
    // `""` has no parts; it is a plain (empty) literal, not an interpolation.
    match merged.as_slice() {
        [] => {
            return Expr::String {
                value: String::new(),
                span,
            }
        }
        [StringPart::Text(text)] => {
            return Expr::String {
                value: text.clone(),
                span,
            };
        }
        _ => {}
    }
    Expr::InterpolatedString {
        parts: merged,
        span,
    }
}
