//! Block, statement, match-arm and `if` lowering.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, Span};
use flux_syntax::{
    AssignStmt, Block, ElseBranch, EmitStmt, Expr, ForStmt, Ident, IfStmt, LetPattern, LetStmt,
    MatchArm, MatchPattern, ReturnStmt, Stmt, TypeName, UnaryOp, WhileStmt,
};

use crate::context::ScopeKind;
use crate::nodes::*;
use crate::symbol::{SymbolId, SymbolKind, SymbolTable};

use super::exprs::{lower_expr, record_array_hint, resolve_name};

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
            record_array_hint(symbols, &name.name, ty.as_ref(), Some(value));
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
        MatchPattern::Tuple { span, .. } | MatchPattern::Struct { span, .. } => Some(*span),
    }
}

fn is_variant_name(name: &str, symbols: &SymbolTable) -> bool {
    symbols.enum_variants.contains_key(name) || name == "None"
}

fn pattern_shape(pattern: &MatchPattern, symbols: &SymbolTable) -> PatShape {
    match pattern {
        MatchPattern::Literal(_) => PatShape::Literal,
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
    let tmp_name = "\0pat".to_string();
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

pub(super) fn lower_match_arm(
    arm: &flux_syntax::MatchArm,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirMatchArm> {
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
        Some(value) => lower_expr(value, symbols, scope, diagnostics, reads_reactive)?,
        None => HirExpr::Bool {
            value: false,
            span: arm.span,
        },
    };
    symbols.pop();
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
            Some(_) => Some("assigning a module binding"),
        },
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
            indices,
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
            let mut reads_reactive = false;
            let scope = tracking_scope(ctx);
            let mut value = lower_expr(value, symbols, scope, diagnostics, &mut reads_reactive)?;
            if !indices.is_empty() {
                // `x[i][j] = v` is `x = set(x, i, set(x[i], j, v))`: the
                // builtin `set` (arrays and maps), whose `x = set(x, …)` form
                // updates in place.
                let indices = indices
                    .iter()
                    .map(|i| lower_expr(i, symbols, scope, diagnostics, &mut reads_reactive))
                    .collect::<Option<Vec<_>>>()?;
                let set_fn = symbols.hidden_builtin("set", *span);
                let mut containers = vec![HirExpr::Ident {
                    symbol: sym,
                    span: name.span,
                }];
                for index in &indices[..indices.len() - 1] {
                    let outer = containers.last().cloned()?;
                    containers.push(HirExpr::Index {
                        base: Box::new(outer),
                        index: Box::new(index.clone()),
                        span: *span,
                    });
                }
                for (container, index) in containers.into_iter().zip(indices).rev() {
                    value = HirExpr::Call {
                        callee: Box::new(HirExpr::Ident {
                            symbol: set_fn,
                            span: *span,
                        }),
                        args: vec![container, index, value],
                        span: *span,
                    };
                }
            }
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
