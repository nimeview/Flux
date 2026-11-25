//! Block, statement, match-arm and `if` lowering.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, FluxWarning, Span};
use flux_syntax::{
    AssignStmt, Block, ElseBranch, EmitStmt, Expr, ForStmt, Ident, IfStmt, LetPattern, LetStmt,
    MatchArm, MatchPattern, PlaceStep, ReturnStmt, Stmt, TypeName, UnaryOp, WhileStmt,
};

use crate::context::ScopeKind;
use crate::nodes::*;
use crate::symbol::{SymbolId, SymbolKind, SymbolTable, WriterKind};

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
        match lower_stmt(stmt, symbols, diagnostics) {
            Some(lowered) => stmts.extend(lowered),
            // Close the scope even on failure, or its names would hide the
            // same names in the code after it.
            None => {
                symbols.pop();
                return None;
            }
        }
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
        LetPattern::Tuple { span: pat_span, .. } => {
            let ctx = symbols.current_context();
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let value = lower_expr(value, symbols, scope, diagnostics, &mut reads_reactive)?;
            // The value is evaluated once, into a hidden local; each name
            // then takes its part.
            let tmp = symbols
                .declare(
                    &format!("\0tup{}", pat_span.start),
                    SymbolKind::Local { mutable: false },
                    *pat_span,
                )
                .ok()?;
            let mut stmts = vec![HirStmt::Let(HirLet {
                name: tmp,
                ty: ty.as_ref().map(lower_type),
                value,
                mutable: false,
                span,
            })];
            let mut parts = Vec::new();
            let base = HirExpr::Ident {
                symbol: tmp,
                span: *pat_span,
            };
            tuple_parts(pattern, &base, symbols, &mut parts);
            for (name, part) in parts {
                if name.name == "_" {
                    stmts.push(HirStmt::Expr(part));
                    continue;
                }
                let id = match symbols.declare(&name.name, SymbolKind::Local { mutable }, name.span)
                {
                    Ok(id) => id,
                    Err(prev) => {
                        diagnostics.push(FluxError::DuplicateDefinition {
                            name: name.name.clone(),
                            prev_span: span_to_miette(prev.span),
                            span: span_to_miette(name.span),
                            source_id: name.span.source,
                        });
                        return None;
                    }
                };
                stmts.push(HirStmt::Let(HirLet {
                    name: id,
                    ty: None,
                    value: part,
                    mutable,
                    span: name.span,
                }));
            }
            Some(stmts)
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

/// For `place.m(…)` where `m` is the name of some `mut self` method and the
/// receiver is a place (a name, then fields and indices): the place's root,
/// its path, and `m`.
fn mut_call_place(callee: &Expr, symbols: &SymbolTable) -> Option<(Ident, Vec<PlaceStep>, String)> {
    let Expr::Field { base, field, .. } = callee else {
        return None;
    };
    if !symbols.mut_method_names.contains(field) {
        return None;
    }
    let mut steps = Vec::new();
    let mut at = base.as_ref();
    loop {
        match at {
            Expr::Ident { name, span } => {
                if name.contains("::") || symbols.module_aliases.contains_key(name) {
                    return None;
                }
                steps.reverse();
                let root = Ident {
                    name: name.clone(),
                    span: *span,
                };
                return Some((root, steps, field.clone()));
            }
            Expr::Field { base, field, span } => {
                steps.push(PlaceStep::Field(Ident {
                    name: field.clone(),
                    span: *span,
                }));
                at = base;
            }
            Expr::Index { base, index, .. } => {
                steps.push(PlaceStep::Index((**index).clone()));
                at = base;
            }
            _ => return None,
        }
    }
}

/// The names a tuple pattern binds, each with the expression for its part of
/// `base` (`base.0`, `base.1.0`, …); `_` binds nothing. Each level of the
/// pattern first checks that its value is a tuple of exactly that many
/// values (the hidden builtin [`TUPLE_CHECK`]); those checks come out under
/// the name `_`, so that a level of only `_` is still checked.
pub(super) fn tuple_parts(
    pattern: &LetPattern,
    base: &HirExpr,
    symbols: &mut SymbolTable,
    out: &mut Vec<(Ident, HirExpr)>,
) {
    let LetPattern::Tuple { elems, span } = pattern else {
        return;
    };
    let checked = HirExpr::Call {
        callee: Box::new(HirExpr::Ident {
            symbol: symbols.hidden_builtin(TUPLE_CHECK, *span),
            span: *span,
        }),
        args: vec![
            base.clone(),
            HirExpr::Int {
                value: elems.len() as i128,
                span: *span,
            },
        ],
        span: *span,
    };
    out.push((
        Ident {
            name: "_".into(),
            span: *span,
        },
        checked.clone(),
    ));
    for (i, elem) in elems.iter().enumerate() {
        let part = HirExpr::Field {
            base: Box::new(checked.clone()),
            field: i.to_string(),
            span: elem.span(),
        };
        match elem {
            LetPattern::Ident(name) if name.name == "_" => {}
            LetPattern::Ident(name) => out.push((name.clone(), part)),
            LetPattern::Tuple { .. } => tuple_parts(elem, &part, symbols, out),
            // The grammar allows only names and tuples inside a tuple.
            LetPattern::Struct { .. } => {}
        }
    }
}

/// The payload pattern of `Variant(…)` when it is more than a binding:
/// a nested pattern, or a lone name that is a payload-free variant.
fn nested_payload(pattern: &MatchPattern, symbols: &SymbolTable) -> Option<MatchPattern> {
    let MatchPattern::Tuple {
        bindings, nested, ..
    } = pattern
    else {
        return None;
    };
    if let Some(inner) = nested {
        return Some((**inner).clone());
    }
    match bindings.as_slice() {
        [name] if is_variant_name(&name.name, symbols) => Some(MatchPattern::Simple(name.clone())),
        _ => None,
    }
}

fn pattern_span(pattern: &MatchPattern) -> Option<Span> {
    match pattern {
        MatchPattern::Simple(v) => Some(v.span),
        MatchPattern::Literal(value) => Some(value.span()),
        MatchPattern::Tuple { span, .. }
        | MatchPattern::Struct { span, .. }
        | MatchPattern::TupleValue { span, .. } => Some(*span),
    }
}

fn is_variant_name(name: &str, symbols: &SymbolTable) -> bool {
    symbols.enum_variants.contains_key(name) || name == "None"
}

fn pattern_shape(pattern: &MatchPattern, symbols: &SymbolTable) -> PatShape {
    match pattern {
        MatchPattern::Literal(Expr::Bool { value, .. }) => PatShape::Bool(*value),
        MatchPattern::Literal(_) => PatShape::Literal,
        MatchPattern::TupleValue { elems, .. } => {
            PatShape::Tuple(elems.iter().map(|e| pattern_shape(e, symbols)).collect())
        }
        MatchPattern::Simple(v) if is_variant_name(&v.name, symbols) => PatShape::Variant {
            name: v.name.clone(),
            payload: None,
        },
        MatchPattern::Simple(_) => PatShape::Any,
        MatchPattern::Tuple { variant, .. } => PatShape::Variant {
            name: variant.name.clone(),
            payload: Some(Box::new(
                nested_payload(pattern, symbols)
                    .map_or(PatShape::Any, |inner| pattern_shape(&inner, symbols)),
            )),
        },
        MatchPattern::Struct { variant, .. } => PatShape::Variant {
            name: variant.name.clone(),
            payload: Some(Box::new(PatShape::Any)),
        },
    }
}

/// `Outer(inner) [if g] => body`, where `inner` is a pattern, lowers to
/// `Outer(tmp) if (match tmp { inner [if g] => true, _ => false })` with value
/// `match tmp { inner => body, _ => <never runs> }`: nested patterns reuse
/// ordinary matching (types, VM, JIT) one level at a time.
fn lower_nested_arm(
    arm: &MatchArm,
    variant: &Ident,
    inner: MatchPattern,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirMatchArm> {
    // The synthesized matches take the inner pattern's span: a match's
    // scrutinee temporary is named after its span start, so each nesting
    // level must differ (sharing the arm's span let an inner match overwrite
    // the outer one's temporary).
    let span = pattern_span(&inner).unwrap_or(arm.span);
    let shape = arm.guard.is_none().then(|| pattern_shape(&inner, symbols));
    symbols.push(ScopeKind::Block);
    // NUL cannot appear in source names; each level declares its own symbol.
    // Each level's name differs, so an inner level never hides an outer one.
    let tmp_name = format!("\0pat{}", span.start);
    let Ok(tmp) = symbols.declare(&tmp_name, SymbolKind::Local { mutable: false }, span) else {
        symbols.pop();
        return None;
    };
    let tmp_expr = Expr::Ident {
        name: tmp_name,
        span,
    };
    let wildcard = |value: bool| MatchArm {
        pattern: MatchPattern::Simple(Ident {
            name: "_".into(),
            span,
        }),
        guard: None,
        stmts: vec![],
        expr: Some(Expr::Bool { value, span }),
        span,
    };
    let test = Expr::Match {
        scrutinee: Box::new(tmp_expr.clone()),
        arms: vec![
            MatchArm {
                pattern: inner.clone(),
                guard: arm.guard.clone(),
                stmts: vec![],
                expr: Some(Expr::Bool { value: true, span }),
                span,
            },
            wildcard(false),
        ],
        span,
    };
    let value = Expr::Match {
        scrutinee: Box::new(tmp_expr),
        arms: vec![
            MatchArm {
                pattern: inner,
                guard: None,
                stmts: arm.stmts.clone(),
                expr: arm.expr.clone(),
                span,
            },
            wildcard(false),
        ],
        span,
    };
    let guard = lower_expr(&test, symbols, scope, diagnostics, reads_reactive);
    let mut expr = lower_expr(&value, symbols, scope, diagnostics, reads_reactive);
    symbols.pop();
    let (guard, expr) = (guard?, expr.as_mut()?);
    // The fallback never runs (the guard checked the pattern): it takes no part
    // in the type, and the arm leaves exactly when the real body does.
    let mut diverges = false;
    if let HirExpr::Match { arms, .. } = expr {
        if let Some(fallback) = arms.last_mut() {
            fallback.diverges = true;
        }
        diverges = arms.first().is_some_and(|a| a.diverges);
    }
    Some(HirMatchArm {
        variant: variant.name.clone(),
        bindings: vec![tmp],
        binding_fields: vec![],
        guard: Some(guard),
        literal: None,
        body: HirBlock {
            stmts: vec![],
            span,
        },
        expr: expr.clone(),
        diverges,
        unit: false,
        nested: shape,
    })
}

/// `(p0, …, pn) [if g] => body` lowers to `tmp if test => value`, where
/// `test` matches `tmp.0` against `p0`, then `tmp.1` against `p1`, …, and
/// is `true` when all match (and `g` holds), and `value` is the same chain
/// with `body` innermost. Each value is matched by an ordinary `match`, so
/// literals, variants, nested patterns, and bindings work as anywhere else.
#[allow(clippy::too_many_arguments)]
fn lower_tuple_arm(
    arm: &MatchArm,
    elems: &[MatchPattern],
    span: Span,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirMatchArm> {
    let shape = arm
        .guard
        .is_none()
        .then(|| pattern_shape(&arm.pattern, symbols));
    symbols.push(ScopeKind::Block);
    // Each level's name differs, so an inner level never hides an outer one.
    let tmp_name = format!("\0pat{}", span.start);
    let Ok(tmp) = symbols.declare(&tmp_name, SymbolKind::Local { mutable: false }, span) else {
        symbols.pop();
        return None;
    };
    // Each level's match takes its element's span: a match's scrutinee
    // temporary is named after its span start, so levels must differ.
    let level = |i: usize, inner: MatchArm| {
        let at = pattern_span(&elems[i]).unwrap_or(span);
        Expr::Match {
            scrutinee: Box::new(Expr::Field {
                base: Box::new(Expr::Ident {
                    name: tmp_name.clone(),
                    span: at,
                }),
                field: i.to_string(),
                span: at,
            }),
            arms: vec![
                inner,
                MatchArm {
                    pattern: MatchPattern::Simple(Ident {
                        name: "_".into(),
                        span: at,
                    }),
                    guard: None,
                    stmts: vec![],
                    expr: Some(Expr::Bool {
                        value: false,
                        span: at,
                    }),
                    span: at,
                },
            ],
            span: at,
        }
    };
    let chain = |leaf: MatchArm| {
        let last = elems.len() - 1;
        let mut expr = level(last, leaf);
        for i in (0..last).rev() {
            let at = pattern_span(&elems[i]).unwrap_or(span);
            expr = level(
                i,
                MatchArm {
                    pattern: elems[i].clone(),
                    guard: None,
                    stmts: vec![],
                    expr: Some(expr),
                    span: at,
                },
            );
        }
        expr
    };
    let last = elems.last()?.clone();
    let test = chain(MatchArm {
        pattern: last.clone(),
        guard: arm.guard.clone(),
        stmts: vec![],
        expr: Some(Expr::Bool { value: true, span }),
        span,
    });
    let value = chain(MatchArm {
        pattern: last,
        guard: None,
        stmts: arm.stmts.clone(),
        expr: arm.expr.clone(),
        span: arm.span,
    });
    let guard = lower_expr(&test, symbols, scope, diagnostics, reads_reactive);
    let mut expr = lower_expr(&value, symbols, scope, diagnostics, reads_reactive);
    symbols.pop();
    let (guard, expr) = (guard?, expr.as_mut()?);
    // The fallbacks never run (the guard checked every value): they take no
    // part in the type. Each level leaves exactly when the body does.
    // The body leaves when it ends in `return`/`break`/`continue`.
    let diverges = arm.expr.is_none()
        && matches!(
            arm.stmts.last(),
            Some(Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_))
        );
    let levels: Vec<Span> = elems
        .iter()
        .map(|e| pattern_span(e).unwrap_or(span))
        .collect();
    mark_levels(expr, &levels, diverges);
    Some(HirMatchArm {
        variant: String::new(),
        bindings: vec![tmp],
        binding_fields: vec![],
        guard: Some(guard),
        literal: None,
        body: HirBlock {
            stmts: vec![],
            span,
        },
        expr: expr.clone(),
        diverges,
        unit: false,
        nested: shape,
    })
}

/// Marks, in the matches of a tuple arm's value chain (found by their spans,
/// which are the element patterns'), the fallback as never running and the
/// arm that goes on as leaving exactly when the body does. The chain may pass
/// through other arms (an element that is itself a tuple or a nested
/// pattern), so every match inside is visited.
fn mark_levels(expr: &mut HirExpr, levels: &[Span], diverges: bool) {
    let HirExpr::Match { arms, span, .. } = expr else {
        return;
    };
    if levels.contains(span) {
        if let Some(fallback) = arms.last_mut() {
            fallback.diverges = true;
        }
        if let Some(first) = arms.first_mut() {
            first.diverges = diverges;
        }
    }
    for arm in arms.iter_mut() {
        mark_levels(&mut arm.expr, levels, diverges);
    }
}

pub(super) fn lower_match_arm(
    arm: &flux_syntax::MatchArm,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirMatchArm> {
    if let Some(arm) = tail_if_as_value(arm, symbols) {
        return lower_match_arm(&arm, symbols, scope, diagnostics, reads_reactive);
    }
    if let MatchPattern::TupleValue { elems, span } = &arm.pattern {
        return lower_tuple_arm(
            arm,
            elems,
            *span,
            symbols,
            scope,
            diagnostics,
            reads_reactive,
        );
    }
    if let MatchPattern::Tuple { variant, .. } = &arm.pattern {
        if let Some(inner) = nested_payload(&arm.pattern, symbols) {
            return lower_nested_arm(
                arm,
                variant,
                inner,
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            );
        }
    }
    symbols.push(ScopeKind::Block);
    let mut literal = None;
    let (variant, bindings, binding_fields) = match &arm.pattern {
        MatchPattern::Literal(value) => {
            let constant = matches!(
                value,
                Expr::Int { .. } | Expr::String { .. } | Expr::Bool { .. }
            ) || matches!(value, Expr::Unary { op: UnaryOp::Neg, expr, .. } if matches!(**expr, Expr::Int { .. }));
            if !constant {
                diagnostics.push(FluxError::Parse {
                    message: "a literal pattern must be an Int, a String without `{…}` \
                              interpolation, or a Bool"
                        .into(),
                    span: span_to_miette(value.span()),
                    source_id: value.span().source,
                });
            }
            literal = lower_expr(value, symbols, scope, diagnostics, reads_reactive);
            (String::new(), vec![], vec![])
        }
        MatchPattern::Simple(v) => {
            if v.name == "_" {
                (String::new(), vec![], vec![])
            } else if symbols.enum_variants.contains_key(&v.name) || v.name == "None" {
                // `None` is `Option`'s variant; as a binding it caught every
                // value (`None => 0` before `Some(v) => v` matched `Some(4)`).
                (v.name.clone(), vec![], vec![])
            } else {
                let Ok(id) = symbols.declare(&v.name, SymbolKind::Local { mutable: false }, v.span)
                else {
                    symbols.pop();
                    return None;
                };
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
        // Lowered by `lower_tuple_arm` above.
        MatchPattern::TupleValue { .. } => (String::new(), vec![], vec![]),
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
    let mut body = Vec::new();
    let in_function = symbols.in_function();
    for stmt in &arm.stmts {
        if let Some(problem) = unsupported_in_arm_block(stmt, symbols, in_function) {
            diagnostics.push(FluxError::Parse {
                message: format!("{problem} is not supported inside a match arm block"),
                span: span_to_miette(arm.span),
                source_id: arm.span.source,
            });
            continue;
        }
        body.extend(lower_stmt(stmt, symbols, diagnostics).unwrap_or_default());
    }
    let leaves = matches!(
        arm.stmts.last(),
        Some(Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_))
    );
    let diverges = arm.expr.is_none() && leaves;
    let unit = arm.expr.is_none() && !leaves;
    let expr = match &arm.expr {
        Some(value) => lower_expr(value, symbols, scope, diagnostics, reads_reactive),
        None => Some(HirExpr::Bool {
            value: false,
            span: arm.span,
        }),
    };
    symbols.pop();
    let expr = expr?;
    Some(HirMatchArm {
        variant,
        bindings,
        binding_fields,
        guard,
        literal,
        body: HirBlock {
            stmts: body,
            span: arm.span,
        },
        expr,
        diverges,
        unit,
        nested: None,
    })
}

/// A block arm that ends in `if … else` takes the value of the branch that
/// runs, as a function body does. Inside a function (the only place an arm
/// may hold an `if`), the chain becomes the arm's value as a `match` whose
/// arms are guarded by its conditions: each branch is a block arm, so it may
/// run statements before its value, nest another chain, or leave. `None` when
/// the arm is left as it is.
fn tail_if_as_value(arm: &MatchArm, symbols: &mut SymbolTable) -> Option<MatchArm> {
    if arm.expr.is_some() || !symbols.in_function() {
        return None;
    }
    let Some(Stmt::If(tail)) = arm.stmts.last() else {
        return None;
    };
    tail.else_branch.as_ref()?;
    let value = if_as_match(tail, symbols);
    let mut stmts = arm.stmts.clone();
    stmts.pop();
    Some(MatchArm {
        pattern: arm.pattern.clone(),
        guard: arm.guard.clone(),
        stmts,
        expr: Some(value),
        span: arm.span,
    })
}

/// `if c { a } else if d { b } else { e }` as
/// `match true { _ if c => { a }, _ if d => { b }, _ => { e } }`. The guards
/// are checked as `if` conditions are, and the match's span is recorded so
/// that its arms must agree without numeric widening, as an `if` tail's
/// branches must. A final `else if` without `else` stays a statement, so that
/// arm is `()`, as in a function body.
fn if_as_match(stmt: &IfStmt, symbols: &mut SymbolTable) -> Expr {
    let wildcard = |span: Span| {
        MatchPattern::Simple(Ident {
            name: "_".into(),
            span,
        })
    };
    let branch = |guard: Option<Expr>, mut stmts: Vec<Stmt>, span: Span| {
        let expr = match stmts.pop() {
            Some(Stmt::Expr(e)) => Some(e),
            Some(other) => {
                stmts.push(other);
                None
            }
            None => None,
        };
        MatchArm {
            pattern: wildcard(span),
            guard,
            stmts,
            expr,
            span,
        }
    };
    let mut arms = Vec::new();
    let mut current = stmt;
    loop {
        arms.push(branch(
            Some(current.cond.clone()),
            current.then_branch.stmts.clone(),
            current.then_branch.span,
        ));
        match &current.else_branch {
            Some(ElseBranch::If(inner)) if inner.else_branch.is_some() => current = inner,
            Some(ElseBranch::If(inner)) => {
                arms.push(branch(None, vec![Stmt::If((**inner).clone())], inner.span));
                break;
            }
            Some(ElseBranch::Block(block)) => {
                arms.push(branch(None, block.stmts.clone(), block.span));
                break;
            }
            None => {
                arms.push(branch(None, vec![], current.span));
                break;
            }
        }
    }
    symbols.if_value_matches.insert(stmt.span);
    Expr::Match {
        scrutinee: Box::new(Expr::Bool {
            value: true,
            span: stmt.span,
        }),
        arms,
        span: stmt.span,
    }
}

/// What a block arm may not contain, if anything. Everywhere: `let`,
/// assigning a local, and expression statements. Inside a function also
/// `if`/`while`/`for` and `return`/`break`/`continue`, which act on the
/// enclosing function or loop. Reactive contexts (derived values, `watch`,
/// `on`) keep the expression-only form, so arms cannot add effects there.
fn unsupported_in_arm_block(
    stmt: &Stmt,
    symbols: &SymbolTable,
    in_function: bool,
) -> Option<&'static str> {
    match stmt {
        Stmt::Let(_) | Stmt::Expr(_) => None,
        // A closure's value is its body's value, and nothing checks a `return`
        // against the function type it is used as.
        Stmt::Return(_) if symbols.in_closure() => Some("`return` inside a closure"),
        Stmt::Return(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::If(_)
        | Stmt::While(_)
        | Stmt::For(_)
            if in_function =>
        {
            None
        }
        Stmt::Assign(a) => match symbols.lookup(&a.name.name).map(|s| s.kind) {
            Some(SymbolKind::Local { mutable: true }) | None => None,
            Some(SymbolKind::Local { mutable: false } | SymbolKind::Param) => {
                Some("assigning an immutable local (declare it with `let mut`)")
            }
            // The checker reports a derived or const target, as elsewhere.
            Some(_) if in_function => None,
            Some(_) => Some("assigning a module binding"),
        },
        Stmt::Emit(_) if in_function => None,
        Stmt::Emit(_) => Some("`emit`"),
        Stmt::Return(_) => Some("`return` outside a function"),
        Stmt::Break(_) => Some("`break` outside a function"),
        Stmt::Continue(_) => Some("`continue` outside a function"),
        Stmt::If(_) => Some("an `if` statement outside a function (use an `if` expression)"),
        Stmt::While(_) => Some("`while` outside a function"),
        Stmt::For(_) => Some("`for` outside a function"),
        Stmt::Watch(_) | Stmt::On(_) => Some("`watch`/`on`"),
    }
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
        Stmt::Assign(AssignStmt {
            name,
            path,
            op,
            value,
            span,
        }) => {
            let sym = resolve_name(&name.name, *span, symbols, diagnostics)?;
            // A closure holds a copy of what it captures: a write could not
            // reach the original.
            if symbols.is_captured(sym) {
                diagnostics.push(FluxError::AssignToImmutableLocal {
                    name: name.name.clone(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                return None;
            }
            check_writer(sym, &name.name, name.span, symbols, diagnostics);
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            // The path's indices are evaluated before the value.
            let mut steps = Vec::with_capacity(path.len());
            for step in path {
                steps.push(match step {
                    PlaceStep::Index(index) => HirPlaceStep::Index(lower_expr(
                        index,
                        symbols,
                        scope,
                        diagnostics,
                        &mut reads_reactive,
                    )?),
                    PlaceStep::Field(field) => HirPlaceStep::Field {
                        name: field.name.clone(),
                        span: field.span,
                    },
                });
            }
            let mut value = lower_expr(value, symbols, scope, diagnostics, &mut reads_reactive)?;
            let _ = reads_reactive;
            let whole = HirExpr::Ident {
                symbol: sym,
                span: name.span,
            };
            let assign = |value, path, op| {
                Some(vec![HirStmt::Assign(HirAssign {
                    name: sym,
                    path,
                    op,
                    value,
                    span: *span,
                })])
            };
            match (steps.as_slice(), op) {
                // `x += v` is `x = x + v`.
                ([], Some(op)) => {
                    value = HirExpr::Binary {
                        op: *op,
                        left: Box::new(whole),
                        right: Box::new(value),
                        span: *span,
                    };
                    assign(value, Vec::new(), None)
                }
                ([], None) => assign(value, Vec::new(), None),
                // `xs[i] = v` is `xs = set(xs, i, v)`: the builtin `set`
                // (arrays and maps), which updates in place.
                ([HirPlaceStep::Index(index)], None) => {
                    let set_fn = symbols.hidden_builtin("set", *span);
                    value = HirExpr::Call {
                        callee: Box::new(HirExpr::Ident {
                            symbol: set_fn,
                            span: *span,
                        }),
                        args: vec![whole, index.clone(), value],
                        span: *span,
                    };
                    assign(value, Vec::new(), None)
                }
                _ => assign(value, steps, *op),
            }
        }
        Stmt::Emit(EmitStmt { event, args, span }) => {
            let event_id =
                super::exprs::resolve_event(&event.name, event.span, symbols, diagnostics)?;
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
        // `place.m(…)` with a `mut self` method `m`: `place = place.m(…)`.
        // Which method `m` is depends on the receiver's type, known only to
        // the type checker: the call site is recorded, and if `m` turns out
        // not to change its receiver, the assignment is only the call.
        Stmt::Expr(whole @ Expr::Call { callee, args, span })
            if mut_call_place(callee, symbols).is_some() =>
        {
            let (root, steps, method) = mut_call_place(callee, symbols)?;
            let sym = resolve_name(&root.name, root.span, symbols, diagnostics)?;
            // A closure cannot assign what it captures: the plain call, which
            // the type checker rejects only if `m` changes its receiver.
            if symbols.is_captured(sym) {
                symbols.captured_call_sites.insert(*span);
                let mut reads_reactive = false;
                let scope = tracking_scope(ctx);
                return lower_expr(whole, symbols, scope, diagnostics, &mut reads_reactive)
                    .map(|e| vec![HirStmt::Expr(e)]);
            }
            // Whether `m` changes the place is known only after type checking:
            // an E027 here is kept for the checker (`deferred_writes`).
            check_writer_at(
                sym,
                &root.name,
                root.span,
                Some(*span),
                symbols,
                diagnostics,
            );
            let mut out = Vec::new();
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            // Each index of the place is evaluated once, before the
            // arguments: into a hidden local unless it is a literal.
            let mut path = Vec::with_capacity(steps.len());
            for (k, step) in steps.into_iter().enumerate() {
                match step {
                    PlaceStep::Index(index) => {
                        let value =
                            lower_expr(&index, symbols, scope, diagnostics, &mut reads_reactive)?;
                        if matches!(value, HirExpr::Int { .. }) {
                            path.push(HirPlaceStep::Index(value));
                            continue;
                        }
                        let at = index.span();
                        let tmp = symbols
                            .declare(
                                &format!("\0recv{}_{k}", span.start),
                                SymbolKind::Local { mutable: false },
                                at,
                            )
                            .ok()?;
                        out.push(HirStmt::Let(HirLet {
                            name: tmp,
                            ty: None,
                            value,
                            mutable: false,
                            span: at,
                        }));
                        path.push(HirPlaceStep::Index(HirExpr::Ident {
                            symbol: tmp,
                            span: at,
                        }));
                    }
                    PlaceStep::Field(field) => path.push(HirPlaceStep::Field {
                        name: field.name,
                        span: field.span,
                    }),
                }
            }
            // The receiver is the place's current value: read like the
            // assignment's own read of it, without the pure-function rule.
            let mut receiver = HirExpr::Ident {
                symbol: sym,
                span: root.span,
            };
            for step in &path {
                receiver = match step {
                    HirPlaceStep::Index(index) => HirExpr::Index {
                        base: Box::new(receiver),
                        index: Box::new(index.clone()),
                        span: *span,
                    },
                    HirPlaceStep::Field { name, span: at } => HirExpr::Field {
                        base: Box::new(receiver),
                        field: name.clone(),
                        span: *at,
                    },
                };
            }
            let args = args
                .iter()
                .map(|a| lower_expr(a, symbols, scope, diagnostics, &mut reads_reactive))
                .collect::<Option<Vec<_>>>()?;
            symbols.mut_call_sites.insert(*span);
            symbols.note_member(&method);
            let call = HirExpr::Call {
                callee: Box::new(HirExpr::Field {
                    base: Box::new(receiver),
                    field: method,
                    span: callee.span(),
                }),
                args,
                span: *span,
            };
            out.push(HirStmt::Assign(HirAssign {
                name: sym,
                path,
                op: None,
                value: call,
                span: *span,
            }));
            Some(out)
        }
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
            pattern,
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
            // `for (k, v) in …` starts each pass with `let (k, v) = <element>`.
            let destructured;
            let body = match pattern {
                Some(pattern) => {
                    let mut stmts = vec![Stmt::Let(LetStmt {
                        pub_: false,
                        mutable: false,
                        pattern: pattern.clone(),
                        ty: None,
                        value: Expr::Ident {
                            name: name.name.clone(),
                            span: name.span,
                        },
                        span: name.span,
                    })];
                    stmts.extend(body.stmts.iter().cloned());
                    destructured = Block {
                        stmts,
                        span: body.span,
                    };
                    &destructured
                }
                None => body,
            };
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

/// E027/E028: a write to a source outside an `on` handler, an `act fn`, or
/// module initialization. Reported once per source and writer.
fn check_writer(
    sym: SymbolId,
    name: &str,
    span: Span,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) {
    check_writer_at(sym, name, span, None, symbols, diagnostics);
}

/// [`check_writer`]; for `place.m(…)` at `call`, an E027 is not reported but
/// kept in `deferred_writes`, for the type checker to report if `m` turns out
/// to change its receiver.
fn check_writer_at(
    sym: SymbolId,
    name: &str,
    span: Span,
    call: Option<Span>,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) {
    // A `const` is not state: assigning one is E009, reported by the type checker.
    if symbols.get(sym).map(|s| s.kind) != Some(SymbolKind::Source)
        || symbols.constants.contains(&sym)
    {
        return;
    }
    // Another module's source is read-only here: it changes through that
    // module's events and `act fn`s (E032).
    let declared_in = symbols.get(sym).map(|s| s.span.source);
    let imported = symbols
        .get(sym)
        .is_some_and(|s| s.name.starts_with("__mod_"));
    if imported && declared_in.is_some_and(|file| file != span.source) {
        let error = FluxError::ForeignSourceWrite {
            name: crate::display_path(name),
            span: span_to_miette(span),
            source_id: span.source,
        };
        match call {
            Some(call) => {
                symbols.deferred_writes.insert(call, error);
            }
            None => diagnostics.push(error),
        }
        return;
    }
    let Some(writer) = symbols.writers.last_mut() else {
        return;
    };
    if matches!(writer.kind, WriterKind::Act | WriterKind::Handler) || writer.warned.contains(&sym)
    {
        return;
    }
    if call.is_none() || writer.kind == WriterKind::Watch {
        writer.warned.insert(sym);
    }
    // A watch that writes warns (E028); any other function is an error (E027).
    let kind = match writer.kind {
        WriterKind::Watch => {
            diagnostics.push_warning(FluxWarning::WatchWrites {
                name: name.to_string(),
                span: span_to_miette(span),
                source_id: span.source,
            });
            return;
        }
        kind => kind,
    };
    let help = match kind {
        WriterKind::Plain => format!(
            "write `act fn {}` so that callers see one change; `flux migrate` does it",
            writer.name
        ),
        WriterKind::Track => "a `track fn` is recomputed whenever what it reads changes; \
                 change state in an `act fn` or an `on` handler"
            .to_string(),
        _ => "change state in an `act fn` or an `on` handler".to_string(),
    };
    let error = FluxError::WriteOutsideAct {
        function: writer.name.clone(),
        name: name.to_string(),
        help,
        span: span_to_miette(span),
        source_id: span.source,
        fix_span: (kind == WriterKind::Plain)
            .then_some(writer.keyword)
            .flatten()
            .map(span_to_miette),
    };
    match call {
        Some(call) => {
            symbols.deferred_writes.insert(call, error);
        }
        None => diagnostics.push(error),
    }
}
