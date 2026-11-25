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
        // `m::x`, `Shape::Circle`, `Point::new`.
        Expr::Ident { name, span } if name.contains("::") => {
            match resolve_path(name, *span, symbols, diagnostics)? {
                // A type function is declared under its full name.
                PathTarget::Name(plain) if plain == *name => {
                    let sym = resolve_name(name, *span, symbols, diagnostics)?;
                    Some(HirExpr::Ident {
                        symbol: sym,
                        span: *span,
                    })
                }
                PathTarget::Name(plain) => lower_expr(
                    &Expr::Ident {
                        name: plain,
                        span: *span,
                    },
                    symbols,
                    scope,
                    diagnostics,
                    reads_reactive,
                ),
                PathTarget::Module(alias, field) => {
                    lower_module_member(&alias, &field, *span, symbols, diagnostics)
                }
                PathTarget::Variant(enum_name, variant) => Some(HirExpr::EnumVariant {
                    enum_name,
                    variant,
                    payload: None,
                    span: *span,
                }),
            }
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
                                 `watch`, `on`, `track fn`, or `act fn`"
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
            // `Shape::Circle(2)` is `Circle(2)` once the path is checked.
            if let Expr::Ident {
                name,
                span: name_span,
            } = callee.as_ref()
            {
                if name.contains("::") {
                    let target = resolve_path(name, *name_span, symbols, diagnostics)?;
                    // `Enum::Variant(payload)`: that enum's variant.
                    if let PathTarget::Variant(enum_name, variant) = &target {
                        let payload = match args.as_slice() {
                            [] => None,
                            [one] => Some(Box::new(lower_expr(
                                one,
                                symbols,
                                scope,
                                diagnostics,
                                reads_reactive,
                            )?)),
                            _ => {
                                diagnostics.push(FluxError::Parse {
                                    message: format!("variant `{name}` carries one value"),
                                    span: span_to_miette(*span),
                                    source_id: span.source,
                                });
                                return None;
                            }
                        };
                        return Some(HirExpr::EnumVariant {
                            enum_name: enum_name.clone(),
                            variant: variant.clone(),
                            payload,
                            span: *span,
                        });
                    }
                    if let PathTarget::Name(plain) = target {
                        if plain == *name {
                            // A type function: the callee resolves as a name.
                            let callee =
                                lower_expr(callee, symbols, scope, diagnostics, reads_reactive)?;
                            let args = args
                                .iter()
                                .map(|a| lower_expr(a, symbols, scope, diagnostics, reads_reactive))
                                .collect::<Option<Vec<_>>>()?;
                            return Some(HirExpr::Call {
                                callee: Box::new(callee),
                                args,
                                span: *span,
                            });
                        }
                        let call = Expr::Call {
                            callee: Box::new(Expr::Ident {
                                name: plain,
                                span: *name_span,
                            }),
                            args: args.clone(),
                            span: *span,
                        };
                        return lower_expr(&call, symbols, scope, diagnostics, reads_reactive);
                    }
                }
            }
            if let Expr::Ident { name, .. } = callee.as_ref() {
                if name == "snap" {
                    // An `act fn` reads without tracking, so `snap(x)` there is
                    // `x`; allowing it keeps a `fn` that becomes `act fn` valid.
                    let in_act_fn = symbols
                        .writers
                        .last()
                        .is_some_and(|w| w.kind == crate::symbol::WriterKind::Act);
                    if scope.context() != crate::CompileContext::PureFn && !in_act_fn {
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
        // `(op.f)` before a call: the field's value, so that the call is not a
        // method call.
        Expr::Paren { inner, span } => {
            let value = lower_expr(inner, symbols, scope, diagnostics, reads_reactive)?;
            Some(HirExpr::Call {
                callee: Box::new(HirExpr::Ident {
                    symbol: symbols.hidden_builtin(GROUPED, *span),
                    span: *span,
                }),
                args: vec![value],
                span: *span,
            })
        }
        // `(a, b)` is a call of the hidden builtin [`TUPLE`].
        Expr::Tuple { elems, span } => {
            if elems.len() < 2 {
                diagnostics.push(FluxError::Parse {
                    message: "a tuple has two or more values; write `(x)` to group one value"
                        .to_string(),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
                return None;
            }
            let args = elems
                .iter()
                .map(|e| lower_expr(e, symbols, scope, diagnostics, reads_reactive))
                .collect::<Option<Vec<_>>>()?;
            Some(HirExpr::Call {
                callee: Box::new(HirExpr::Ident {
                    symbol: symbols.hidden_builtin(TUPLE, *span),
                    span: *span,
                }),
                args,
                span: *span,
            })
        }
        // `base with { x: v }` is a call of the hidden builtin
        // [`WITH_FIELDS`] with the base, then each field's name and value.
        Expr::With { base, fields, span } => {
            let mut args = vec![lower_expr(
                base,
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            )?];
            for (i, field) in fields.iter().enumerate() {
                if let Some(prev) = fields[..i].iter().find(|f| f.name.name == field.name.name) {
                    diagnostics.push(FluxError::DuplicateDefinition {
                        name: field.name.name.clone(),
                        prev_span: span_to_miette(prev.name.span),
                        span: span_to_miette(field.name.span),
                        source_id: field.name.span.source,
                    });
                }
                args.push(HirExpr::String {
                    value: field.name.name.clone(),
                    span: field.name.span,
                });
                args.push(lower_expr(
                    &field.value,
                    symbols,
                    scope,
                    diagnostics,
                    reads_reactive,
                )?);
            }
            Some(HirExpr::Call {
                callee: Box::new(HirExpr::Ident {
                    symbol: symbols.hidden_builtin(WITH_FIELDS, *span),
                    span: *span,
                }),
                args,
                span: *span,
            })
        }
        // `Self { … }` inside an `impl`.
        Expr::StructLit { name, fields, span }
            if name.name == "Self" && symbols.self_type.is_some() =>
        {
            let lit = Expr::StructLit {
                name: flux_syntax::Ident {
                    name: symbols.self_type.clone().unwrap_or_default(),
                    span: name.span,
                },
                fields: fields.clone(),
                span: *span,
            };
            lower_expr(&lit, symbols, scope, diagnostics, reads_reactive)
        }
        Expr::StructLit { name, fields, span } if name.name.contains("::") => {
            let plain = match resolve_path(&name.name, name.span, symbols, diagnostics)? {
                // `Shape::Rect { w: 2 }`: that enum's struct variant.
                PathTarget::Variant(enum_name, variant) => {
                    return variant_fields(
                        enum_name,
                        variant,
                        fields,
                        *span,
                        symbols,
                        scope,
                        diagnostics,
                        reads_reactive,
                    );
                }
                PathTarget::Name(plain) => plain,
                // A type is program-wide; the module must still export it.
                PathTarget::Module(alias, plain) => {
                    if !symbols
                        .module_types
                        .contains(&(alias.clone(), plain.clone()))
                    {
                        diagnostics.push(FluxError::UnknownExport {
                            module: alias,
                            name: plain,
                            span: span_to_miette(name.span),
                            source_id: name.span.source,
                        });
                        return None;
                    }
                    plain
                }
            };
            if plain.contains("::") {
                diagnostics.push(FluxError::UnknownType {
                    name: name.name.clone(),
                    span: span_to_miette(name.span),
                    source_id: name.span.source,
                });
                return None;
            }
            let lit = Expr::StructLit {
                name: flux_syntax::Ident {
                    name: plain,
                    span: name.span,
                },
                fields: fields.clone(),
                span: *span,
            };
            lower_expr(&lit, symbols, scope, diagnostics, reads_reactive)
        }
        // `Rect { w: 2 }` where `Rect` is a variant and not a struct.
        Expr::StructLit { name, fields, span }
            if symbols.lookup(&name.name).is_none()
                && symbols.enum_variants.contains_key(&name.name) =>
        {
            let enum_name = symbols.enum_variants[&name.name].clone();
            variant_fields(
                enum_name,
                name.name.clone(),
                fields,
                *span,
                symbols,
                scope,
                diagnostics,
                reads_reactive,
            )
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
                .filter_map(|a| {
                    let a = normalize_arm(a, symbols, diagnostics)?;
                    lower_match_arm(&a, symbols, scope, diagnostics, reads_reactive)
                })
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
                    // The older spelling of `m::x`.
                    diagnostics.push_warning(flux_diagnostics::FluxWarning::Deprecated {
                        message: format!("`{alias}.{field}` is now written `{alias}::{field}`"),
                        replacement: format!("{alias}::{field}"),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                    return lower_module_member(alias, field, *span, symbols, diagnostics);
                }
            }
            let base_expr = lower_expr(base, symbols, scope, diagnostics, reads_reactive)?;
            symbols.note_member(field);
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
        false,
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
        TypeName::Tuple { elems, span } => HirType::Generic {
            name: TUPLE.to_string(),
            args: elems
                .iter()
                .map(|e| lower_param_type(e, type_params))
                .collect(),
            span: *span,
        },
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
                name: crate::display_path(name),
                span: span_to_miette(span),
                source_id: span.source,
            });
            None
        }
    }
}

/// What a `a::b` path names.
enum PathTarget {
    /// A name to look up as usual: a variant (`Shape::Circle` is `Circle`)
    /// or a type function (`Point::new`, declared under that name).
    Name(String),
    /// An item of an imported module.
    Module(String, String),
    /// `Enum::Variant`, checked: the variant of exactly that enum.
    Variant(String, String),
}

fn resolve_path(
    name: &str,
    span: Span,
    symbols: &SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<PathTarget> {
    let (first, rest) = name.split_once("::")?;
    // Inside an `impl`, `Self::f` is `Type::f`.
    if first == "Self" {
        let Some(target) = symbols.self_type.clone() else {
            diagnostics.push(FluxError::Parse {
                message: "`Self` is only known inside an `impl`".to_string(),
                span: span_to_miette(span),
                source_id: span.source,
            });
            return None;
        };
        return resolve_path(&format!("{target}::{rest}"), span, symbols, diagnostics);
    }
    let fail = |message: String, diagnostics: &mut DiagnosticBag| {
        diagnostics.push(FluxError::Parse {
            message,
            span: span_to_miette(span),
            source_id: span.source,
        });
    };
    if rest.contains("::") {
        fail(
            format!("`{name}`: a path has two parts, as in `math::sqrt`"),
            diagnostics,
        );
        return None;
    }
    if symbols.module_aliases.contains_key(first) {
        return Some(PathTarget::Module(first.to_string(), rest.to_string()));
    }
    if let Some(variants) = symbols.enum_variant_sets.get(first) {
        if variants.contains(rest) {
            return Some(PathTarget::Variant(first.to_string(), rest.to_string()));
        }
        // An enum's type function (`Shape::unit()`).
        if symbols.lookup(name).is_some() {
            return Some(PathTarget::Name(name.to_string()));
        }
        fail(
            format!(
                "enum `{}` has no variant `{rest}`",
                crate::display_path(first)
            ),
            diagnostics,
        );
        return None;
    }
    Some(PathTarget::Name(name.to_string()))
}

/// `m::x`: the item `x` of the module imported as `m`.
fn lower_module_member(
    alias: &str,
    field: &str,
    span: Span,
    symbols: &SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirExpr> {
    if let Some(&sym) = symbols
        .qualified_symbols
        .get(&(alias.to_string(), field.to_string()))
    {
        return Some(HirExpr::Ident { symbol: sym, span });
    }
    diagnostics.push(FluxError::UnknownExport {
        module: alias.to_string(),
        name: field.to_string(),
        span: span_to_miette(span),
        source_id: span.source,
    });
    None
}

/// `arm` with each `Enum::Variant` in its pattern written as `Variant`, once
/// checked.
pub(super) fn normalize_arm(
    arm: &flux_syntax::MatchArm,
    symbols: &SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<flux_syntax::MatchArm> {
    let mut arm = arm.clone();
    arm.pattern = normalize_pattern(&arm.pattern, symbols, diagnostics)?;
    Some(arm)
}

fn normalize_pattern(
    pattern: &flux_syntax::MatchPattern,
    symbols: &SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<flux_syntax::MatchPattern> {
    use flux_syntax::MatchPattern;
    let name = |ident: &flux_syntax::Ident, diagnostics: &mut DiagnosticBag| {
        if !ident.name.contains("::") {
            return Some(ident.clone());
        }
        match resolve_path(&ident.name, ident.span, symbols, diagnostics)? {
            PathTarget::Variant(_, plain) => Some(flux_syntax::Ident {
                name: plain,
                span: ident.span,
            }),
            PathTarget::Name(_) | PathTarget::Module(..) => {
                diagnostics.push(FluxError::Parse {
                    message: format!("`{}` is not a variant", ident.name),
                    span: span_to_miette(ident.span),
                    source_id: ident.span.source,
                });
                None
            }
        }
    };
    Some(match pattern {
        MatchPattern::Simple(v) => MatchPattern::Simple(name(v, diagnostics)?),
        MatchPattern::Literal(_) => pattern.clone(),
        MatchPattern::Tuple {
            variant,
            bindings,
            nested,
            span,
        } => MatchPattern::Tuple {
            variant: name(variant, diagnostics)?,
            bindings: bindings.clone(),
            nested: match nested {
                Some(inner) => Some(Box::new(normalize_pattern(inner, symbols, diagnostics)?)),
                None => None,
            },
            span: *span,
        },
        MatchPattern::Struct {
            variant,
            fields,
            span,
        } => MatchPattern::Struct {
            variant: name(variant, diagnostics)?,
            fields: fields.clone(),
            span: *span,
        },
        MatchPattern::TupleValue { elems, span } => MatchPattern::TupleValue {
            elems: elems
                .iter()
                .map(|e| normalize_pattern(e, symbols, diagnostics))
                .collect::<Option<Vec<_>>>()?,
            span: *span,
        },
    })
}

/// A struct variant built with its fields: the variant, with the fields as a
/// struct named [`crate::VARIANT_FIELDS`] for its payload.
#[allow(clippy::too_many_arguments)]
fn variant_fields(
    enum_name: String,
    variant: String,
    fields: &[flux_syntax::FieldInit],
    span: flux_diagnostics::Span,
    symbols: &mut SymbolTable,
    scope: ScopeKind,
    diagnostics: &mut DiagnosticBag,
    reads_reactive: &mut bool,
) -> Option<HirExpr> {
    let fields = fields
        .iter()
        .filter_map(|f| {
            let value = lower_expr(&f.value, symbols, scope, diagnostics, reads_reactive)?;
            Some((f.name.name.clone(), value))
        })
        .collect();
    Some(HirExpr::EnumVariant {
        enum_name,
        variant,
        payload: Some(Box::new(HirExpr::StructLit {
            name: crate::VARIANT_FIELDS.to_string(),
            fields,
            span,
        })),
        span,
    })
}
