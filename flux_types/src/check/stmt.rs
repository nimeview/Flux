//! Statement- and binding-level checking (control flow, mutability, purity).

use super::state::{Scope, TypeCheckContext};

use std::collections::{HashMap, HashSet};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{
    BindingKind, HirAssign, HirBinding, HirBlock, HirElse, HirEmit, HirExpr, HirFor, HirIf,
    HirStmt, SymbolId,
};

use crate::types::Type;

use super::infer::{infer_expr, infer_expr_expected};

use super::resolve::resolve_hir_type;
use super::util::{default_unknown_variants, mentions_empty, types_compatible, unwrap_value_type};

fn require_bool(
    ctx: &TypeCheckContext,
    expr: &HirExpr,
    scope: &mut Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    let Some(ty) = infer_expr(ctx, expr, scope, names, source, diagnostics) else {
        return;
    };
    let ty = unwrap_value_type(&ty);
    if !ty.is_error() && ty != Type::Bool {
        diagnostics.push(FluxError::TypeMismatch {
            expected: "Bool".into(),
            found: ty.display(),
            span: span_to_miette(expr.span()),
            source_id: expr.span().source,
        });
    }
}

pub(super) fn check_binding(
    ctx: &TypeCheckContext,
    binding: &HirBinding,
    types: &Scope<'_>,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    let expected_inner = types
        .get(&binding.name)
        .and_then(|t| t.unwrap_reactive().cloned())
        .unwrap_or(Type::Error);

    let found = infer_expr_expected(
        ctx,
        &binding.value,
        binding.ty.as_ref().map(|_| &expected_inner),
        types,
        names,
        source,
        diagnostics,
    );
    if let (exp, Some(got)) = (expected_inner, found) {
        if !exp.is_error() && !got.is_error() && !types_compatible(&exp, &got) {
            diagnostics.push(FluxError::TypeMismatch {
                expected: exp.display(),
                found: got.display(),
                span: span_to_miette(binding.span),
                source_id: binding.span.source,
            });
        }
    }
}

/// Derived values must be pure: no control flow (E008), no side effects
/// (E017), and no `snap` (E008), which the value would not follow, however a
/// function reaches them. A `const` initializer must have no side effects
/// (E017). Runs after inference, which resolves method calls.
pub(super) fn check_purity(
    ctx: &TypeCheckContext,
    program: &flux_hir::HirProgram,
    diagnostics: &mut DiagnosticBag,
) {
    let method_calls = ctx.method_calls.borrow();
    let purity = super::effects::analyze(program, ctx.builtin_symbols(), &method_calls);
    for item in &program.items {
        let flux_hir::HirItem::Binding(binding) = item else {
            continue;
        };
        let derived = matches!(binding.kind, BindingKind::Derived);
        let is_const = program
            .names
            .get(&binding.name)
            .is_some_and(|n| ctx.const_bindings.contains(n));
        if !derived && !is_const {
            continue;
        }
        let mut check = PureCheck {
            purity: &purity,
            method_calls: &method_calls,
            program,
            builtins: ctx.builtin_symbols(),
            names: &program.names,
            aliases: HashMap::new(),
            derived,
            diagnostics,
        };
        check.expr(&binding.value, false);
    }
}

struct PureCheck<'a> {
    purity: &'a super::effects::Purity,
    method_calls: &'a super::effects::MethodCalls,
    program: &'a flux_hir::HirProgram,
    builtins: &'a HashSet<SymbolId>,
    names: &'a HashMap<SymbolId, String>,
    /// The functions each local (an arm binding or an arm block's `let`) holds.
    aliases: HashMap<SymbolId, super::effects::Held>,
    /// A derived value, or else a `const` initializer.
    derived: bool,
    diagnostics: &'a mut DiagnosticBag,
}

impl PureCheck<'_> {
    fn held(&self, expr: &HirExpr) -> super::effects::Held {
        super::effects::held(
            expr,
            &self.aliases,
            self.program,
            self.builtins,
            self.method_calls,
            None,
        )
    }

    /// `runs`: whether a function that is `expr`'s value is called here (as
    /// a callee or an argument), rather than only held.
    fn expr(&mut self, expr: &HirExpr, runs: bool) {
        use super::effects::{returned, targets, Node};
        match expr {
            HirExpr::Snap { span, expr } => {
                if self.derived {
                    self.diagnostics
                        .push(FluxError::InvalidControlFlowInDerived {
                            construct: "snap()".into(),
                            span: span_to_miette(*span),
                            source_id: span.source,
                        });
                } else {
                    self.expr(expr, runs);
                }
            }
            HirExpr::Call { callee, args, span } => {
                if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
                    if super::effects::is_constructor(*symbol, self.program, self.builtins) {
                        for arg in args {
                            self.expr(arg, runs);
                        }
                        return;
                    }
                }
                let (ran, name) = match callee.as_ref() {
                    HirExpr::Ident { symbol, .. } => (
                        targets(*symbol, &self.aliases),
                        self.names.get(symbol).cloned().unwrap_or_default(),
                    ),
                    HirExpr::Field { base, field, .. } => {
                        match self.method_calls.get(&(None, *span)) {
                            Some(method) => {
                                let method = *method;
                                self.expr(base, false);
                                (
                                    std::collections::HashSet::from([Node::Sym(method)]),
                                    field.clone(),
                                )
                            }
                            // Not a method: the function in field `field`.
                            None => {
                                self.expr(callee, true);
                                (Default::default(), String::new())
                            }
                        }
                    }
                    other => {
                        self.expr(other, true);
                        (Default::default(), String::new())
                    }
                };
                let mut nodes = ran.clone();
                if runs {
                    nodes.extend(returned(&ran));
                }
                self.report(&nodes, name, *span);
                // A function passed along may be called by whatever receives it.
                for arg in args {
                    self.expr(arg, true);
                }
            }
            HirExpr::Ident { symbol, span } => {
                if runs {
                    let name = self.names.get(symbol).cloned().unwrap_or_default();
                    self.report(&targets(*symbol, &self.aliases), name, *span);
                }
            }
            HirExpr::Closure {
                fn_symbol, span, ..
            } => {
                if runs {
                    let name = self.names.get(fn_symbol).cloned().unwrap_or_default();
                    self.report(
                        &std::collections::HashSet::from([Node::Sym(*fn_symbol)]),
                        name,
                        *span,
                    );
                }
            }
            HirExpr::Unary { expr, .. }
            | HirExpr::Cast { expr, .. }
            | HirExpr::ToText { expr, .. } => self.expr(expr, false),
            HirExpr::Try { expr, .. } | HirExpr::Field { base: expr, .. } => self.expr(expr, runs),
            HirExpr::Binary { left, right, .. } => {
                self.expr(left, false);
                self.expr(right, false);
            }
            HirExpr::Index { base, index, .. } => {
                self.expr(base, runs);
                self.expr(index, false);
            }
            HirExpr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.expr(cond, false);
                self.expr(then_expr, runs);
                self.expr(else_expr, runs);
            }
            HirExpr::Array { elems, .. } => elems.iter().for_each(|e| self.expr(e, runs)),
            HirExpr::StructLit { fields, .. } => {
                fields.iter().for_each(|(_, e)| self.expr(e, runs))
            }
            HirExpr::EnumVariant { payload, .. } => {
                if let Some(value) = payload {
                    self.expr(value, runs);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee, false);
                let parts = self.held(scrutinee);
                for arm in arms {
                    for binding in &arm.bindings {
                        self.aliases.insert(*binding, parts.clone());
                    }
                    if let Some(guard) = &arm.guard {
                        self.expr(guard, false);
                    }
                    for stmt in &arm.body.stmts {
                        match stmt {
                            HirStmt::Let(local) => {
                                self.expr(&local.value, false);
                                let held = self.held(&local.value);
                                self.aliases.insert(local.name, held);
                            }
                            HirStmt::Assign(assign) => {
                                self.expr(&assign.value, false);
                                let held = self.held(&assign.value);
                                self.aliases.entry(assign.name).or_default().extend(held);
                            }
                            HirStmt::Expr(e) => self.expr(e, false),
                            _ => {}
                        }
                    }
                    self.expr(&arm.expr, runs);
                }
            }
            HirExpr::Int { .. }
            | HirExpr::Float { .. }
            | HirExpr::Bool { .. }
            | HirExpr::String { .. } => {}
        }
    }

    /// Reports running `nodes` here if one has side effects or, in a derived
    /// value, reads a value with `snap`.
    fn report(&mut self, nodes: &super::effects::Held, name: String, span: flux_diagnostics::Span) {
        if nodes.iter().any(|n| self.purity.effectful.contains(n)) {
            self.diagnostics.push(FluxError::SideEffectInDerived {
                name,
                place: if self.derived {
                    "a derived expression".into()
                } else {
                    "a `const` initializer".into()
                },
                span: span_to_miette(span),
                source_id: span.source,
            });
        } else if self.derived && nodes.iter().any(|n| self.purity.snapshotting.contains(n)) {
            self.diagnostics
                .push(FluxError::InvalidControlFlowInDerived {
                    construct: format!("snap()` read inside `{name}"),
                    span: span_to_miette(span),
                    source_id: span.source,
                });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_block(
    ctx: &TypeCheckContext,
    block: &HirBlock,
    locals: &Scope<'_>,
    module: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    reactive: bool,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    immutable_locals: &mut HashSet<SymbolId>,
) {
    let mut scope = locals.clone();
    for stmt in &block.stmts {
        check_stmt(
            ctx,
            stmt,
            &mut scope,
            module,
            names,
            reactive,
            source,
            diagnostics,
            immutable_locals,
        );
    }
}

/// Check every explicit return against a function's declared result type.
/// `infer_block_return` is used to infer a function's implicit tail value; it
/// cannot validate returns nested inside control-flow branches.
pub(super) fn check_return_types(
    ctx: &TypeCheckContext,
    block: &HirBlock,
    locals: &Scope<'_>,
    expected: &Type,
    names: &HashMap<SymbolId, String>,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    let mut scope = locals.clone();
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let(binding) => {
                let ty = binding
                    .ty
                    .as_ref()
                    .and_then(|ty| resolve_hir_type(ctx, ty, diagnostics))
                    .or_else(|| infer_expr(ctx, &binding.value, &scope, names, source, diagnostics))
                    .unwrap_or(Type::Error);
                scope.insert(binding.name, ty);
            }
            HirStmt::Return(ret) => {
                let found = ret
                    .value
                    .as_ref()
                    .and_then(|value| infer_expr(ctx, value, &scope, names, source, diagnostics))
                    .unwrap_or(Type::Unit);
                let expected_value = unwrap_value_type(expected);
                let found_value = unwrap_value_type(&found);
                if !expected_value.is_error()
                    && !found_value.is_error()
                    && !types_compatible(&expected_value, &found_value)
                {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected_value.display(),
                        found: found_value.display(),
                        span: span_to_miette(ret.span),
                        source_id: ret.span.source,
                    });
                }
            }
            HirStmt::If(if_stmt) => {
                check_return_types(
                    ctx,
                    &if_stmt.then_branch,
                    &scope,
                    expected,
                    names,
                    source,
                    diagnostics,
                );
                if let Some(else_branch) = &if_stmt.else_branch {
                    match else_branch {
                        HirElse::If(nested) => {
                            let nested_block = HirBlock {
                                stmts: vec![HirStmt::If((**nested).clone())],
                                span: nested.span,
                            };
                            check_return_types(
                                ctx,
                                &nested_block,
                                &scope,
                                expected,
                                names,
                                source,
                                diagnostics,
                            );
                        }
                        HirElse::Block(block) => check_return_types(
                            ctx,
                            block,
                            &scope,
                            expected,
                            names,
                            source,
                            diagnostics,
                        ),
                    }
                }
            }
            HirStmt::While(loop_stmt) => check_return_types(
                ctx,
                &loop_stmt.body,
                &scope,
                expected,
                names,
                source,
                diagnostics,
            ),
            HirStmt::For(loop_stmt) => {
                let mut loop_scope = scope.clone();
                let iter_ty = infer_expr(ctx, &loop_stmt.iter, &scope, names, source, diagnostics)
                    .unwrap_or(Type::Error);
                let elem_ty = match unwrap_value_type(&iter_ty) {
                    Type::Array(inner) => *inner,
                    Type::Map { key, value } => Type::Tuple(vec![*key, *value]),
                    _ => Type::Error,
                };
                loop_scope.insert(loop_stmt.name, elem_ty);
                check_return_types(
                    ctx,
                    &loop_stmt.body,
                    &loop_scope,
                    expected,
                    names,
                    source,
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_if(
    ctx: &TypeCheckContext,
    stmt: &HirIf,
    scope: &mut Scope<'_>,
    module: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    reactive: bool,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    immutable_locals: &mut HashSet<SymbolId>,
) {
    require_bool(ctx, &stmt.cond, scope, names, source, diagnostics);
    check_block(
        ctx,
        &stmt.then_branch,
        scope,
        module,
        names,
        reactive,
        source,
        diagnostics,
        immutable_locals,
    );
    if let Some(el) = &stmt.else_branch {
        match el {
            HirElse::If(i) => check_if(
                ctx,
                i,
                scope,
                module,
                names,
                reactive,
                source,
                diagnostics,
                immutable_locals,
            ),
            HirElse::Block(b) => check_block(
                ctx,
                b,
                scope,
                module,
                names,
                reactive,
                source,
                diagnostics,
                immutable_locals,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn check_stmt(
    ctx: &TypeCheckContext,
    stmt: &HirStmt,
    scope: &mut Scope<'_>,
    module: &HashMap<SymbolId, Type>,
    names: &HashMap<SymbolId, String>,
    reactive: bool,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
    immutable_locals: &mut HashSet<SymbolId>,
) {
    match stmt {
        HirStmt::Let(s) => {
            let ty =
                s.ty.as_ref()
                    .and_then(|t| resolve_hir_type(ctx, t, diagnostics))
                    .unwrap_or_else(|| {
                        // Use scope (includes params + prior locals) not just module-level types.
                        let inferred = infer_expr(ctx, &s.value, scope, names, source, diagnostics)
                            .unwrap_or(Type::Error);
                        reject_empty_array_binding(inferred, s.span, diagnostics)
                    });
            scope.insert(s.name, ty);
            if !s.mutable {
                immutable_locals.insert(s.name);
            }
            if let Some(got) = infer_expr_expected(
                ctx,
                &s.value,
                s.ty.as_ref().and_then(|_| scope.get(&s.name)),
                scope,
                names,
                source,
                diagnostics,
            ) {
                if let Some(expected) = scope.get(&s.name) {
                    let expected = unwrap_value_type(expected);
                    let got = unwrap_value_type(&got);
                    if !expected.is_error() && !got.is_error() && !types_compatible(&expected, &got)
                    {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: expected.display(),
                            found: got.display(),
                            span: span_to_miette(s.span),
                            source_id: s.span.source,
                        });
                    }
                }
            }
        }
        HirStmt::Assign(assign) => 'assign: {
            let HirAssign {
                name, value, span, ..
            } = assign;
            // `x.m(…)` written as a statement on a place: it stores the
            // receiver back only when `m` is a `mut self` method; otherwise it
            // is just the call.
            let mut pre_got = None;
            if ctx.mut_call_sites.contains(span) {
                let got = infer_expr(ctx, value, scope, names, source, diagnostics)
                    .map(|ty| unwrap_value_type(&ty));
                let resolved = ctx
                    .method_calls
                    .borrow()
                    .get(&(ctx.current_function.get(), *span))
                    .copied();
                let changing = resolved.is_some_and(|id| ctx.mut_methods.contains(&id))
                    || ctx.changing_sites.borrow().contains(span);
                if !changing {
                    break 'assign;
                }
                pre_got = Some(got);
            }
            if let Some(local_name) = names.get(name) {
                // Only the module `const` itself: a local of the same name
                // (`let mut x` in a function, beside `const x`) is assignable.
                if ctx.const_bindings.contains(local_name) && module.contains_key(name) {
                    diagnostics.push(FluxError::AssignToImmutableLocal {
                        name: local_name.clone(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            if immutable_locals.contains(name) {
                let local_name = names
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| format!("binding #{name:?}"));
                diagnostics.push(FluxError::AssignToImmutableLocal {
                    name: local_name,
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
            } else if let Some(ty) = module.get(name).or_else(|| scope.get(name)) {
                if matches!(ty, Type::Read(_)) {
                    diagnostics.push(FluxError::AssignToDerived {
                        name: names
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| format!("#{}", name.0)),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
            // A path assigns a part: `p.x`, `xs[i].done`.
            let expected = if assign.path.is_empty() {
                module
                    .get(name)
                    .or_else(|| scope.get(name))
                    .map(unwrap_value_type)
            } else {
                infer_expr(ctx, &assign.place_expr(), scope, names, source, diagnostics)
                    .map(|ty| unwrap_value_type(&ty))
            };
            let got = match pre_got {
                Some(got) => got,
                None => infer_expr(ctx, value, scope, names, source, diagnostics)
                    .map(|ty| unwrap_value_type(&ty)),
            };
            // `part op= value` stores `part op value`.
            let got = match (assign.op, &expected, got) {
                (Some(op), Some(part), Some(got)) => {
                    super::infer::infer_binary(op, part, &got, *span, diagnostics)
                        .map(|ty| unwrap_value_type(&ty))
                }
                (_, _, got) => got,
            };
            if let (Some(expected), Some(got)) = (expected, got) {
                if !expected.is_error() && !got.is_error() && !types_compatible(&expected, &got) {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected.display(),
                        found: got.display(),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
            }
        }
        HirStmt::Emit(HirEmit { event, args, span }) => match module.get(event) {
            Some(Type::Event(values)) => {
                if args.len() != values.len() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: crate::check::count(values.len(), "value"),
                        found: crate::check::count(args.len(), "value"),
                        span: span_to_miette(*span),
                        source_id: span.source,
                    });
                }
                for (arg, want) in args.iter().zip(values) {
                    let got = infer_expr(ctx, arg, scope, names, source, diagnostics)
                        .unwrap_or(Type::Error);
                    if !want.is_error() && !got.is_error() && !types_compatible(want, &got) {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: want.display(),
                            found: got.display(),
                            span: span_to_miette(arg.span()),
                            source_id: arg.span().source,
                        });
                    }
                }
            }
            Some(_) | None => {
                diagnostics.push(FluxError::UnknownEvent {
                    name: format!("event #{event:?}"),
                    span: span_to_miette(*span),
                    source_id: span.source,
                });
            }
        },
        HirStmt::Expr(expr) => {
            let _ = infer_expr(ctx, expr, scope, names, source, diagnostics);
        }
        HirStmt::Return(r) => {
            let found = match &r.value {
                Some(v) => infer_expr(ctx, v, scope, names, source, diagnostics),
                None => Some(Type::Unit),
            };
            // Same check (and message) as `check_return_types`, which does not
            // see returns inside match arm blocks; duplicates are dropped.
            if let (Some(expected), Some(found)) = (ctx.fn_return.borrow().clone(), found) {
                let (expected, found) = (unwrap_value_type(&expected), unwrap_value_type(&found));
                if !expected.is_error() && !found.is_error() && !types_compatible(&expected, &found)
                {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: expected.display(),
                        found: found.display(),
                        span: span_to_miette(r.span),
                        source_id: r.span.source,
                    });
                }
            }
        }
        HirStmt::Break(_) | HirStmt::Continue(_) => {}
        HirStmt::If(s) => check_if(
            ctx,
            s,
            scope,
            module,
            names,
            reactive,
            source,
            diagnostics,
            immutable_locals,
        ),
        HirStmt::While(h) => {
            require_bool(ctx, &h.cond, scope, names, source, diagnostics);
            check_block(
                ctx,
                &h.body,
                scope,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
        HirStmt::For(HirFor {
            name, iter, body, ..
        }) => {
            let iter_ty =
                infer_expr(ctx, iter, scope, names, source, diagnostics).unwrap_or(Type::Error);
            let iter_ty = unwrap_value_type(&iter_ty);
            let elem = match iter_ty {
                Type::Array(inner) => *inner,
                // A map gives its entries as `(key, value)`, in key order.
                Type::Map { key, value } => Type::Tuple(vec![*key, *value]),
                other if !other.is_error() => {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: "an array or a map (use range(n) for numeric sequences)".into(),
                        found: other.display(),
                        span: span_to_miette(iter.span()),
                        source_id: iter.span().source,
                    });
                    Type::Error
                }
                _ => Type::Error,
            };
            scope.insert(*name, elem);
            check_block(
                ctx,
                body,
                scope,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
        HirStmt::Watch(w) => {
            check_block(
                ctx,
                &w.body,
                scope,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
        HirStmt::On(o) => {
            let mut local = Scope::new(module);
            crate::check::bind_handler_values(o, module.get(&o.event), &mut local, diagnostics);
            check_block(
                ctx,
                &o.body,
                &local,
                module,
                names,
                true,
                source,
                diagnostics,
                immutable_locals,
            );
        }
    }
}

/// A binding cannot take its type from a bare `[]`: there is no element type
/// to infer. Reports that and returns the type to use instead (elements `Error`).
pub(super) fn reject_empty_array_binding(
    ty: Type,
    span: flux_diagnostics::Span,
    diagnostics: &mut DiagnosticBag,
) -> Type {
    let ty = default_unknown_variants(&ty);
    if !mentions_empty(&ty) {
        return ty;
    }
    if matches!(unwrap_value_type(&ty), Type::Map { .. }) {
        diagnostics.push(FluxError::TypeMismatch {
            expected: "a type annotation, e.g. `let m: Map[String, Int] = map_new()`".into(),
            found: "an empty map with no key or value type".into(),
            span: span_to_miette(span),
            source_id: span.source,
        });
        return Type::Map {
            key: Box::new(Type::Error),
            value: Box::new(Type::Error),
        };
    }
    diagnostics.push(FluxError::TypeMismatch {
        expected: "an element type annotation, e.g. `let xs: [String] = []`".into(),
        found: "empty array `[]` with no element type".into(),
        span: span_to_miette(span),
        source_id: span.source,
    });
    Type::Array(Box::new(Type::Error))
}
