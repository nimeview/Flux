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
pub use exprs::split_numeric_suffix;
pub mod limits;
mod stmts;
mod types;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId, Span};
use pest::error::InputLocation;
use pest::Parser;
use pest_derive::Parser as PestDeriveParser;

use crate::ast::*;

use decls::{
    parse_const_binding, parse_derived_binding, parse_enum, parse_event, parse_extern, parse_fn,
    parse_impl, parse_import, parse_let_binding, parse_live, parse_rfn, parse_struct, parse_trait,
    parse_type_alias,
};
use stmts::{parse_if_stmt, parse_on, parse_watch};

#[derive(PestDeriveParser)]
#[grammar = "flux.pest"]
pub struct FluxParser;

/// Native stack for the parser and the compiler passes after it: they recurse
/// over the syntax tree, which `parser::limits` bounds, so this is a fixed,
/// thread-independent budget (allocated on demand, not up front).
pub const COMPILER_STACK: usize = 64 << 20;

pub fn parse_program(
    source_id: SourceId,
    input: &str,
    diagnostics: &mut DiagnosticBag,
) -> Option<Program> {
    if let Some((span, message)) = limits::lexical_nesting(source_id, input) {
        diagnostics.push(FluxError::Parse {
            message,
            span: span_to_miette(span),
            source_id,
        });
        return None;
    }
    stacker::grow(COMPILER_STACK, || {
        let program = parse_program_unbounded(source_id, input, diagnostics)?;
        if let Some(span) = limits::tree_depth_exceeds(&program) {
            diagnostics.push(FluxError::Parse {
                message: format!(
                    "expression nested more than {} levels deep (for example, a very long \
                     chain of operators); split it with `let` bindings",
                    limits::MAX_TREE_DEPTH
                ),
                span: span_to_miette(span),
                source_id,
            });
            // Dropping the tree recurses too: do it on this stack.
            drop(program);
            return None;
        }
        Some(program)
    })
}

/// Whether `before` (the text up to a parse error) ends inside the braces of
/// a `trait`, whose body holds only signatures.
fn in_trait_body(before: &str) -> bool {
    let Some(start) = before
        .rmatch_indices("trait ")
        .map(|(i, _)| i)
        .find(|&i| i == 0 || !before.as_bytes()[i - 1].is_ascii_alphanumeric())
    else {
        return false;
    };
    let body = &before[start..];
    body.matches('{').count() == 1 && !body.contains('}')
}

fn parse_program_unbounded(
    source_id: SourceId,
    input: &str,
    diagnostics: &mut DiagnosticBag,
) -> Option<Program> {
    let pairs = match FluxParser::parse(Rule::program, input) {
        Ok(mut pairs) => pairs.next()?.into_inner(),
        Err(err) => {
            let span = span_from_pest_error(source_id, &err);
            let message = match input.get(span.start..).and_then(|rest| rest.chars().next()) {
                Some(c) if !c.is_ascii() => format!(
                    "unexpected `{c}`: outside strings and comments, Flux code is ASCII only"
                ),
                _ if in_trait_body(&input[..span.start]) => "a trait method is only a \
                    signature and ends with `;`, as in `fn area(self) -> Float;`"
                    .into(),
                _ => match parse_hint(&input[..span.start], &input[span.start..]) {
                    Some(hint) => hint,
                    // The span already carries the location; pest's `Display`
                    // would render a second snippet inside the diagnostic.
                    None => match &err.variant {
                        pest::error::ErrorVariant::ParsingError { positives, .. }
                            if !positives.is_empty() =>
                        {
                            expected_in_words(positives, &input[..span.start], &input[span.start..])
                        }
                        _ => err.variant.message().into_owned(),
                    },
                },
            };
            diagnostics.push(FluxError::Parse {
                message,
                span: span_to_miette(span),
                source_id,
            });
            return None;
        }
    };

    for pair in pairs.clone().flatten() {
        if pair.as_rule() == Rule::tuple_payload && pair.clone().into_inner().count() > 1 {
            diagnostics.push(FluxError::Parse {
                message: "a tuple variant holds one type; use a struct for several, as in \
                          `Pair(Point)` or `Pair { x: Int, y: Int }`"
                    .into(),
                span: span_to_miette(span_of(source_id, &pair)),
                source_id,
            });
            return None;
        }
    }

    let mut items = Vec::new();
    for pair in pairs {
        if let Some((span, literal)) = invalid_integer_literal(source_id, pair.clone(), false) {
            diagnostics.push(FluxError::Parse {
                message: format!(
                    "numeric literal `{literal}` is outside the supported range for its type"
                ),
                span: span_to_miette(span),
                source_id,
            });
            return None;
        }
        if let Some((span, escape)) = invalid_escape(source_id, pair.clone()) {
            diagnostics.push(FluxError::Parse {
                message: format!(
                    "unknown escape sequence `{escape}` in string literal \
                     (supported: \\n \\t \\r \\0 \\\\ \\\" \\{{ \\}})"
                ),
                span: span_to_miette(span),
                source_id,
            });
            return None;
        }
        if let Some(span) = indexed_emit(source_id, input, pair.clone()) {
            diagnostics.push(FluxError::Parse {
                message: "`emit` names an instance directly: bind it first, as in \
                          `let item = items[i]` then `emit item.Ping()`"
                    .into(),
                span: span_to_miette(span),
                source_id,
            });
            return None;
        }
        if let Some(span) = leading_minus(source_id, input, pair.clone()) {
            diagnostics.push(FluxError::Parse {
                message: "this line starts with `-`, which would continue the expression on \
                          the line above as a subtraction; to subtract, end the previous line \
                          with `-`; for a new negative value, write `0 - …` or bind it with `let`"
                    .into(),
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

/// A parse error for the usual slips, in words: what is wrong and how to
/// write it. `before` is the text up to the error, `rest` the text from it.
fn parse_hint(before: &str, rest: &str) -> Option<String> {
    let line_rest = rest.lines().next().unwrap_or("").trim();
    let last_line = before.rsplit('\n').next().unwrap_or("").trim();
    // `A => x` then a new line `B => y` with no comma between.
    if line_rest.contains("=>")
        && !line_rest.starts_with("=>")
        && before.trim_end().ends_with(|c: char| c != ',' && c != '{')
        && inside_match(before)
    {
        return Some(
            "expected `,` before this match arm: arms are separated by commas \
             (a block arm `=> { … }` needs none after it)"
                .into(),
        );
    }
    // `let m = {}`: braces are a block, not a value.
    if rest.starts_with('{')
        && (last_line.ends_with('=') || last_line.ends_with('('))
        && rest[1..].trim_start().starts_with('}')
    {
        return Some(
            "`{}` is not a value: an empty map is `Map::new()` (with its type on the \
             binding, as in `let m: Map[String, Int] = Map::new()`)"
                .into(),
        );
    }
    // `fn f(a: Int b: Int)`, `struct P { x: Int\n y: Int }`: the error lands
    // on the type, and what follows it is the next name.
    if last_line.ends_with(':') || last_line.ends_with(": mut") {
        // The type (`Int`, `[Int]`, `Map[String, Int]` without spaces),
        // then what follows it: `b: …` means a missing comma.
        let type_end = rest
            .find(|c: char| !(c.is_alphanumeric() || "_[]".contains(c)))
            .unwrap_or(rest.len());
        let after_type = rest[type_end..].trim_start();
        let name_end = after_type
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(after_type.len());
        let next_is_name = name_end > 0 && after_type[name_end..].trim_start().starts_with(':');
        if next_is_name {
            return Some(
                "expected `,` after this type: parameters and fields are separated by commas"
                    .into(),
            );
        }
    }
    // `emit items[i].Ping()`: an instance's event goes through a name.
    if last_line.starts_with("emit ") && rest.starts_with('[') {
        return Some(
            "`emit` names an instance directly: bind it first, as in \
             `let item = items[i]` then `emit item.Ping()`"
                .into(),
        );
    }
    None
}

/// Whether `before` ends inside the braces of a `match`.
fn inside_match(before: &str) -> bool {
    let mut depth = 0i32;
    for (i, c) in before.char_indices().rev() {
        match c {
            '}' => depth += 1,
            '{' if depth > 0 => depth -= 1,
            '{' => {
                let head = before[..i].rsplit('\n').next().unwrap_or("");
                return head.trim_start().starts_with("match ")
                    || head.contains(" match ")
                    || head.contains("= match ")
                    || head.contains("=> match ");
            }
            _ => {}
        }
    }
    false
}

/// pest's "expected eq_op, rel_op, …" in words, with what was found.
fn expected_in_words(positives: &[Rule], before: &str, rest: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for rule in positives {
        let word = match rule {
            Rule::eq_op
            | Rule::rel_op
            | Rule::add_op
            | Rule::bits_op
            | Rule::mul_op
            | Rule::assign_op
            | Rule::kw_as
            | Rule::kw_with => "an operator",
            Rule::index_suffix
            | Rule::try_suffix
            | Rule::field_suffix
            | Rule::call_args
            | Rule::call_suffix
            | Rule::with_suffix => "`.`, `(`, `[`, or `?`",
            Rule::stmt | Rule::block_stmts => "a statement",
            Rule::expr
            | Rule::or_expr
            | Rule::and_expr
            | Rule::eq_expr
            | Rule::rel_expr
            | Rule::add_expr
            | Rule::mul_expr
            | Rule::cast_expr
            | Rule::unary_expr
            | Rule::postfix_expr
            | Rule::primary_expr
            | Rule::expr_list
            | Rule::arg_list
            | Rule::call_arg => "an expression",
            Rule::ident | Rule::path_ident | Rule::ident_list => "a name",
            Rule::type_name
            | Rule::simple_type
            | Rule::generic_type
            | Rule::array_type
            | Rule::tuple_type
            | Rule::function_type
            | Rule::option_type
            | Rule::result_type
            | Rule::type_ann => "a type",
            Rule::match_arm | Rule::match_pattern => "a match arm",
            Rule::block | Rule::if_branch | Rule::arm_block => "`{`",
            Rule::item => "a declaration (`fn`, `let`, `struct`, `event`, …)",
            Rule::EOI => "the end of the file",
            Rule::param | Rule::param_list => "a parameter",
            Rule::kw_else | Rule::else_clause => "`else`",
            Rule::emit_args => "`(`",
            Rule::string_lit | Rule::raw_string_lit => "a string",
            Rule::int_lit | Rule::float_lit | Rule::lit_pat | Rule::neg_int_lit => "a number",
            _ => "",
        };
        if !word.is_empty() && !words.contains(&word) {
            words.push(word);
        }
    }
    let expected = match words.as_slice() {
        [] => "something else".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
    };
    let found: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace())
        .take(20)
        .collect();
    let message = if found.is_empty() {
        format!("expected {expected}, found the end of the line")
    } else {
        format!("expected {expected}, found `{found}`")
    };
    match unclosed(before) {
        Some((open, line)) => format!("{message}; the `{open}` on line {line} is not closed"),
        None => message,
    }
}

/// The innermost `(` or `[` still open at the end of `before`, with its
/// line, skipping strings and comments.
fn unclosed(before: &str) -> Option<(char, usize)> {
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut line = 1;
    let mut chars = before.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        match c {
            '\n' => {
                line += 1;
                in_string = false;
            }
            '\\' if in_string => {
                chars.next();
            }
            '"' => in_string = !in_string,
            '/' if !in_string && chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        line += 1;
                        break;
                    }
                }
            }
            '(' | '[' | '{' if !in_string => stack.push((c, line)),
            ')' | ']' | '}' if !in_string => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.pop().filter(|(c, _)| *c != '{')
}

/// `emit items[i].Ping()`: the grammar reads `emit items` and then an array
/// `[i]`, so it is caught here, by the `[` right after the emitted name.
fn indexed_emit(
    source_id: SourceId,
    input: &str,
    pair: pest::iterators::Pair<'_, Rule>,
) -> Option<Span> {
    if pair.as_rule() == Rule::emit_stmt {
        let target = pair.clone().into_inner().find(|p| {
            matches!(
                p.as_rule(),
                Rule::emit_member | Rule::path_ident | Rule::ident
            )
        })?;
        let end = target.as_span().end();
        if input[end..].starts_with('[') {
            return Some(span_of(source_id, &pair));
        }
        return None;
    }
    pair.into_inner()
        .find_map(|child| indexed_emit(source_id, input, child))
}

/// A binary `-` that begins a line: newlines do not end expressions, so
/// `f()` followed by a line `-1` would silently mean `f() - 1`.
fn leading_minus(
    source_id: SourceId,
    input: &str,
    pair: pest::iterators::Pair<'_, Rule>,
) -> Option<Span> {
    if pair.as_rule() == Rule::add_expr {
        let children: Vec<_> = pair.clone().into_inner().collect();
        for window in children.windows(2) {
            let (lhs, op) = (&window[0], &window[1]);
            if op.as_rule() != Rule::add_op || op.as_str() != "-" {
                continue;
            }
            let lhs_end = lhs.as_span().start() + lhs.as_str().trim_end().len();
            if input[lhs_end..op.as_span().start()].contains('\n') {
                return Some(span_of(source_id, op));
            }
        }
    }
    pair.into_inner()
        .find_map(|child| leading_minus(source_id, input, child))
}

fn invalid_escape(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> Option<(Span, String)> {
    if pair.as_rule() == Rule::string_escape {
        if exprs::unescape(pair.as_str()).is_none() {
            return Some((span_of(source_id, &pair), pair.as_str().to_string()));
        }
        return None;
    }
    pair.into_inner()
        .find_map(|child| invalid_escape(source_id, child))
}

fn invalid_integer_literal(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    allow_i64_min_magnitude: bool,
) -> Option<(Span, String)> {
    if pair.as_rule() == Rule::float_lit {
        let (digits, _) = exprs::split_numeric_suffix(pair.as_str());
        if digits
            .replace('_', "")
            .parse::<f64>()
            .is_ok_and(f64::is_infinite)
        {
            return Some((span_of(source_id, &pair), pair.as_str().to_string()));
        }
    }
    if pair.as_rule() == Rule::int_lit {
        let (digits, suffix) = exprs::split_numeric_suffix(pair.as_str());
        let digits = exprs::decimal_digits(digits);
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
                exprs::decimal_digits(digits) == "9223372036854775808"
                    && suffix.is_none_or(|suffix| suffix == "i64")
            })
    } else {
        false
    };
    let allow_child_i64_min = allow_i64_min_magnitude || direct_i64_min_operand;
    pair.into_inner()
        .find_map(|child| invalid_integer_literal(source_id, child, allow_child_i64_min))
}

pub(super) fn parse_item(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> Option<Item> {
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
        Rule::live_decl => Some(Item::Live(parse_live(source_id, inner, span))),
        Rule::extern_decl => Some(Item::Extern(parse_extern(source_id, inner, span))),
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
