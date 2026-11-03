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
                        let help = if symbols.immutable_lets.contains(&sym) {
                            format!(
                                "`{name}` is a module `let` (a reactive source); if it is a \
                                 fixed value, declare it `const {name} = …` to read it here, \
                                 or use `snap({name})`"
                            )
                        } else {
                            format!(
                                "use `snap({name})` to take a snapshot, or move this code into \
                                 `watch`, `on`, or `rfn`"
                            )
                        };
                        diagnostics.push(FluxError::ReactiveInPureContext {
                            name: name.clone(),
                            span: span_to_miette(*span),
                            source_id: span.source,
                            help,
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
            let lowered_callee = lower_expr(callee, symbols, scope, diagnostics, reads_reactive)?;
            let lowered_args = args
                .iter()
                .map(|a| lower_expr(a, symbols, scope, diagnostics, reads_reactive))
                .collect::<Option<Vec<_>>>()?;
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
            lower_closure(params, body, *span, symbols, diagnostics, reads_reactive)
        }
        Expr::Cast { expr, ty, span } => {
            let inner = lower_expr(expr, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Cast {
                expr: Box::new(inner),
                ty: ty.clone(),
                span: *span,
            })
        }
        Expr::Index { base, index, span } => Some(HirExpr::Index {
            base: Box::new(lower_expr(
                base,
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            )?),
            index: Box::new(lower_expr(
                index,
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            )?),
            span: *span,
        }),
        Expr::Try { expr, span } => {
            let inner = lower_expr(expr, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Try {
                expr: Box::new(inner),
                span: *span,
            })
        }
    }
}

/// Lowers a closure literal into a synthetic top-level function. Each parameter's
/// type comes from (in priority order): an explicit annotation (`|x: String|`), a
/// contextual type supplied later by the checker, or an `Int` fallback.
#[allow(clippy::too_many_arguments)]
fn lower_closure(
    params: &[flux_syntax::ClosureParam],
    body: &Expr,
    span: Span,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
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
        .map(|p| flux_syntax::Param {
            name: p.name.clone(),
            // The checker replaces this placeholder from context, or confirms Int fallback.
            ty: p.ty.clone().unwrap_or_else(|| fallback.clone()),
            span: p.span,
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
    symbols.open_closure();
    let lowered = lower_fn_inner(
        &closure_name,
        &[],
        &fn_params,
        None,
        &fn_body,
        span,
        symbols,
        diagnostics,
        true,
    );
    let uses = symbols.close_closure();
    *reads_reactive |= uses.reads_reactive;
    let captures = uses.captures;
    if let Some(mut f) = lowered {
        f.is_closure = true;
        for (i, param) in f.params.iter_mut().enumerate() {
            param.unannotated = params[i].ty.is_none();
            param.defaulted_to_int = param.unannotated;
        }
        f.captures = captures.clone();
        symbols.pending_fns.push(HirItem::Fn(f));
    }
    let sym = resolve_name(&closure_name, span, symbols, diagnostics)?;
    Some(HirExpr::Closure {
        fn_symbol: sym,
        captures,
        span,
    })
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
            flux_syntax::StringPart::Expr(e) => HirExpr::ToText {
                expr: Box::new(lower_expr(e, symbols, scope, diagnostics, reads_reactive)?),
                span: e.span(),
            },
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
    match ty {
        TypeName::Simple { name, span } if type_params.iter().any(|p| p == name) => {
            HirType::TypeParam {
                name: name.clone(),
                span: *span,
            }
        }
        TypeName::Function { params, ret, span } => HirType::Function {
            params: params
                .iter()
                .map(|p| lower_param_type(p, type_params))
                .collect(),
            ret: ret
                .as_ref()
                .map(|r| Box::new(lower_param_type(r, type_params))),
            span: *span,
        },
        _ => lower_type(ty),
    }
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
    match symbols.lookup(name).map(|sym| sym.id) {
        Some(id) => {
            symbols.note_use(id);
            Some(id)
        }
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
