//! Expression lowering, closure desugaring and name resolution.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, Span};
use flux_syntax::{Block, Expr, Stmt, TypeName};

use crate::context::ScopeKind;
use crate::nodes::*;
use crate::symbol::{SymbolId, SymbolKind, SymbolTable};

use super::funcs::lower_fn_inner;
use super::stmts::lower_match_arm;

pub(super) fn lower_expr(
    expr: &Expr,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirExpr> {
    match expr {
        Expr::Int { value, span } => Some(HirExpr::Int {
            value: *value,
            span: *span,
        }),
        Expr::Float { value, span } => Some(HirExpr::Float {
            value: *value,
            span: *span,
        }),
        Expr::Bool { value, span } => Some(HirExpr::Bool {
            value: *value,
            span: *span,
        }),
        Expr::String { value, span } => Some(HirExpr::String {
            value: value.clone(),
            span: *span,
        }),
        Expr::InterpolatedString { parts, span } => {
            lower_interpolated_string(parts, *span, symbols, scope, diagnostics, reads_reactive)
        }
        Expr::Ident { name, span } => {
            if let Some(enum_name) = symbols.enum_variants.get(name) {
                return Some(HirExpr::EnumVariant {
                    enum_name: enum_name.clone(),
                    variant: name.clone(),
                    payload: None,
                    span: *span,
                });
            }
            let sym = resolve_name(name, *span, symbols, diagnostics)?;
            // Bare `None` (the builtin) is the value `None()`, not the constructor.
            if name == "None" && symbols.lookup(name).map(|s| s.kind) == Some(SymbolKind::Builtin) {
                return Some(HirExpr::Call {
                    callee: Box::new(HirExpr::Ident {
                        symbol: sym,
                        span: *span,
                    }),
                    args: Vec::new(),
                    span: *span,
                });
            }
            if !symbols.constants.contains(&sym)
                && is_reactive_kind(
                    symbols
                        .lookup(name)
                        .map(|s| s.kind)
                        .unwrap_or(SymbolKind::Local { mutable: false }),
                )
            {
                match scope.context() {
                    crate::CompileContext::PureFn => {
                        diagnostics.push(FluxError::ReactiveInPureContext {
                            name: name.clone(),
                            span: span_to_miette(*span),
                            source_id: span.source,
                        });
                        return None;
                    }
                    _ => *reads_reactive = true,
                }
            }
            Some(HirExpr::Ident {
                symbol: sym,
                span: *span,
            })
        }
        Expr::Call { callee, args, span } => {
            if let Expr::Ident { name, .. } = callee.as_ref() {
                if name == "snap" {
                    if scope.context() != crate::CompileContext::PureFn {
                        diagnostics.push(FluxError::InvalidControlFlowInDerived {
                            construct: "snap()".into(),
                            span: span_to_miette(*span),
                            source_id: span.source,
                        });
                        return None;
                    }
                    if args.len() != 1 {
                        diagnostics.push(FluxError::Parse {
                            message: "`snap` expects exactly one argument".into(),
                            span: span_to_miette(*span),
                            source_id: span.source,
                        });
                        return None;
                    }
                    let mut snap_reads = false;
                    let inner = lower_expr(
                        &args[0],
                        symbols,
                        ScopeKind::Module,
                        diagnostics,
                        &mut snap_reads,
                    )?;
                    let _ = snap_reads;
                    return Some(HirExpr::Snap {
                        expr: Box::new(inner),
                        span: *span,
                    });
                }
                if let Some(enum_name) = symbols.enum_variants.get(name).cloned() {
                    let payload = match args.len() {
                        0 => None,
                        1 => {
                            let p =
                                lower_expr(&args[0], symbols, scope, diagnostics, reads_reactive)?;
                            Some(Box::new(p))
                        }
                        _ => {
                            diagnostics.push(FluxError::Parse {
                                message: format!(
                                    "enum variant `{name}` expects at most one payload"
                                ),
                                span: span_to_miette(*span),
                                source_id: span.source,
                            });
                            return None;
                        }
                    };
                    return Some(HirExpr::EnumVariant {
                        enum_name,
                        variant: name.clone(),
                        payload,
                        span: *span,
                    });
                }
            }
            // Closures passed directly as HOF callback arguments (e.g. `map(nums, |x| ...)`)
            // get their untyped params inferred from the call-site context, since the
            // closure's own params carry no type info to check against otherwise.
            let closure_hints = if let Expr::Ident { name, .. } = callee.as_ref() {
                hint_types_for_call(name, args, symbols)
            } else {
                None
            };
            let lowered_callee = lower_expr(callee, symbols, scope, diagnostics, reads_reactive)?;
            let lowered_args = args
                .iter()
                .filter_map(|a| match a {
                    Expr::Closure { params, body, span } => lower_closure(
                        params,
                        body,
                        *span,
                        closure_hints.as_deref().unwrap_or(&[]),
                        symbols,
                        diagnostics,
                    ),
                    _ => lower_expr(a, symbols, scope, diagnostics, reads_reactive),
                })
                .collect();
            Some(HirExpr::Call {
                callee: Box::new(lowered_callee),
                args: lowered_args,
                span: *span,
            })
        }
        Expr::Unary { op, expr, span } => {
            let expr = lower_expr(expr, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Unary {
                op: *op,
                expr: Box::new(expr),
                span: *span,
            })
        }
        Expr::Binary {
            op,
            left,
            right,
            span,
        } => {
            let left = lower_expr(left, symbols, scope, diagnostics, reads_reactive)?;
            let right = lower_expr(right, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Binary {
                op: *op,
                left: Box::new(left),
                right: Box::new(right),
                span: *span,
            })
        }
        Expr::If {
            cond,
            then_expr,
            else_expr,
            span,
        } => {
            let cond = lower_expr(cond, symbols, scope, diagnostics, reads_reactive)?;
            let then_expr = lower_expr(then_expr, symbols, scope, diagnostics, reads_reactive)?;
            let else_expr = lower_expr(else_expr, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::If {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
                span: *span,
            })
        }
        Expr::Array { elems, span } => {
            let elems = elems
                .iter()
                .filter_map(|e| lower_expr(e, symbols, scope, diagnostics, reads_reactive))
                .collect();
            Some(HirExpr::Array { elems, span: *span })
        }
        Expr::StructLit { name, fields, span } => {
            let fields = fields
                .iter()
                .filter_map(|f| {
                    let value = lower_expr(&f.value, symbols, scope, diagnostics, reads_reactive)?;
                    Some((f.name.name.clone(), value))
                })
                .collect();
            Some(HirExpr::StructLit {
                name: name.name.clone(),
                fields,
                span: *span,
            })
        }
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => {
            let scrutinee = Box::new(lower_expr(
                scrutinee,
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            )?);
            let arms = arms
                .iter()
                .filter_map(|a| lower_match_arm(a, symbols, scope, diagnostics, reads_reactive))
                .collect();
            Some(HirExpr::Match {
                scrutinee,
                arms,
                span: *span,
            })
        }
        Expr::Field { base, field, span } => {
            if let Expr::Ident { name: alias, .. } = base.as_ref() {
                if symbols.module_aliases.contains_key(alias) {
                    if let Some(&sym) = symbols
                        .qualified_symbols
                        .get(&(alias.clone(), field.clone()))
                    {
                        return Some(HirExpr::Ident {
                            symbol: sym,
                            span: *span,
                        });
                    }
                    diagnostics.push(FluxError::UnknownExport {
                        module: alias.clone(),
                        name: field.clone(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    return None;
                }
            }
            let base_expr = lower_expr(base, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Field {
                base: Box::new(base_expr),
                field: field.clone(),
                span: *span,
            })
        }
        Expr::Closure { params, body, span } => {
            lower_closure(params, body, *span, &[], symbols, diagnostics)
        }
        Expr::Cast { expr, ty, span } => {
            let inner = lower_expr(expr, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Cast {
                expr: Box::new(inner),
                ty: ty.clone(),
                span: *span,
            })
        }
    }
}

/// Lowers a closure literal into a synthetic top-level function. Each parameter's
/// type comes from (in priority order): an explicit annotation (`|x: String|`), a
/// positional hint inferred from the call-site context (see `hint_types_for_call`),
/// or a fallback of `Int` for backward compatibility with untyped closures.
fn lower_closure(
    params: &[flux_syntax::ClosureParam],
    body: &Expr,
    span: Span,
    hints: &[Option<TypeName>],
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirExpr> {
    let closure_name = symbols.fresh_closure_name();
    let _ = symbols.declare(
        &closure_name,
        SymbolKind::Function { reactive: false },
        span,
    );
    let fallback = TypeName::Simple {
        name: "Int".into(),
        span,
    };
    let fn_params: Vec<flux_syntax::Param> = params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ty =
                p.ty.clone()
                    .or_else(|| hints.get(i).cloned().flatten())
                    .unwrap_or_else(|| fallback.clone());
            flux_syntax::Param {
                name: p.name.clone(),
                ty,
                span: p.span,
            }
        })
        .collect();
    let fn_body = Block {
        stmts: vec![Stmt::Expr(body.clone())],
        span,
    };
    // Closures are lowered as `reactive` functions (like `rfn`) so their bodies may
    // read module Source/Derived bindings — this is how captures are implemented.
    // `flux_ir::lower::collect_expr_reads` inlines the closure body's reads into the
    // calling derived's static dependencies, so e.g. `filter(items, |x| x > threshold)`
    // correctly recomputes when the captured `threshold` source changes.
    if let Some(f) = lower_fn_inner(
        &closure_name,
        &[],
        &fn_params,
        None,
        &fn_body,
        span,
        symbols,
        diagnostics,
        true,
    ) {
        symbols.pending_fns.push(HirItem::Fn(f));
    }
    let sym = resolve_name(&closure_name, span, symbols, diagnostics)?;
    Some(HirExpr::Closure {
        fn_symbol: sym,
        span,
    })
}

/// Best-effort positional parameter type hints for a HOF builtin call, derived from
/// the already-lowered structure of the other arguments (e.g. the array element type
/// for `map`/`filter`, or the accumulator/element types for `reduce`).
fn hint_types_for_call(
    callee_name: &str,
    args: &[Expr],
    symbols: &SymbolTable,
) -> Option<Vec<Option<TypeName>>> {
    match callee_name {
        "map" | "filter" => {
            let elem = infer_syntax_elem_type(args.first()?, symbols);
            Some(vec![elem])
        }
        "reduce" => {
            let elem = infer_syntax_elem_type(args.first()?, symbols);
            let init = infer_syntax_scalar_type(args.get(1)?);
            Some(vec![init, elem])
        }
        _ => None,
    }
}

/// Records (or clears, when unknown) the element type HOF closures over `name`
/// should assume. An annotation such as `[String]` wins over the value.
pub(super) fn record_array_hint(
    symbols: &mut SymbolTable,
    name: &str,
    annotation: Option<&TypeName>,
    value: Option<&Expr>,
) {
    let hint = match annotation {
        Some(TypeName::Array { inner, .. }) => Some((**inner).clone()),
        Some(_) => None,
        None => value.and_then(|v| infer_syntax_elem_type(v, symbols)),
    };
    match hint {
        Some(elem) => {
            symbols.array_elem_hints.insert(name.to_string(), elem);
        }
        None => {
            symbols.array_elem_hints.remove(name);
        }
    }
}

pub(super) fn infer_syntax_elem_type(expr: &Expr, symbols: &SymbolTable) -> Option<TypeName> {
    match expr {
        Expr::Array { elems, .. } => infer_syntax_scalar_type(elems.first()?),
        Expr::Ident { name, .. } => symbols.array_elem_hints.get(name).cloned(),
        Expr::Call { callee, span, .. } => match callee.as_ref() {
            Expr::Ident { name, .. } if name == "str_split" => Some(TypeName::Simple {
                name: "String".into(),
                span: *span,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn infer_syntax_scalar_type(expr: &Expr) -> Option<TypeName> {
    match expr {
        Expr::Int { span, .. } => Some(TypeName::Simple {
            name: "Int".into(),
            span: *span,
        }),
        Expr::Float { span, .. } => Some(TypeName::Simple {
            name: "Float".into(),
            span: *span,
        }),
        Expr::Bool { span, .. } => Some(TypeName::Simple {
            name: "Bool".into(),
            span: *span,
        }),
        Expr::String { span, .. } => Some(TypeName::Simple {
            name: "String".into(),
            span: *span,
        }),
        Expr::InterpolatedString { span, .. } => Some(TypeName::Simple {
            name: "String".into(),
            span: *span,
        }),
        _ => None,
    }
}

fn lower_interpolated_string(
    parts: &[flux_syntax::StringPart],
    span: Span,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirExpr> {
    let mut expr: Option<HirExpr> = None;
    for part in parts {
        let piece = match part {
            flux_syntax::StringPart::Text(text) => HirExpr::String {
                value: text.clone(),
                span,
            },
            flux_syntax::StringPart::Ident(name) => lower_expr(
                &Expr::Ident {
                    name: name.clone(),
                    span,
                },
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            )?,
        };
        expr = Some(match expr {
            None => piece,
            Some(left) => HirExpr::Binary {
                op: flux_syntax::BinaryOp::Add,
                left: Box::new(left),
                right: Box::new(piece),
                span,
            },
        });
    }
    expr
}

pub(super) fn lower_param_type(ty: &TypeName, type_params: &[String]) -> HirType {
    if let TypeName::Simple { name, span } = ty {
        if type_params.iter().any(|p| p == name) {
            return HirType::TypeParam {
                name: name.clone(),
                span: *span,
            };
        }
    }
    lower_type(ty)
}

fn is_reactive_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Source | SymbolKind::Derived | SymbolKind::Function { reactive: true }
    )
}

pub(super) fn resolve_name(
    name: &str,
    span: flux_diagnostics::Span,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<SymbolId> {
    match symbols.lookup(name) {
        Some(sym) => Some(sym.id),
        None => {
            diagnostics.push(FluxError::UndefinedName {
                name: name.to_string(),
                span: span_to_miette(span),
                source_id: span.source,
            });
            None
        }
    }
}
