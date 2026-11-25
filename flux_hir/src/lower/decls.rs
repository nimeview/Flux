//! Lowering of top-level declarations: structs, enums, traits, impls, events,
//! and module-level bindings / source reassignments.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_syntax::{
    ConstBinding, DerivedStmt, EventDecl, Expr, Ident, IfStmt, ImplDecl, LetPattern, LetStmt,
    TraitDecl, TypeName,
};
use std::collections::HashMap;

use crate::context::ScopeKind;
use crate::nodes::{lower_type, *};
use crate::symbol::{BindingKind, SymbolKind, SymbolTable, Writer, WriterKind};

use super::constfold::fold_const_hir_expr;
use super::exprs::lower_expr;
use super::funcs::lower_fn_as;
use super::stmts::lower_if;

pub(super) fn lower_trait(decl: &TraitDecl, symbols: &SymbolTable) -> Option<HirTrait> {
    let sym = symbols.lookup(&decl.name.name)?;
    let methods = decl
        .methods
        .iter()
        .map(|m| {
            (
                m.name.name.clone(),
                m.params.iter().map(|p| lower_type(&p.ty)).collect(),
                m.ret.as_ref().map(lower_type),
            )
        })
        .collect();
    Some(HirTrait {
        name: sym.id,
        methods,
        span: decl.span,
    })
}

/// Whether `method` takes the value it is called on: its first parameter is
/// `self`, written `self`/`mut self` or with a type (`self: Square`).
fn has_receiver(method: &flux_syntax::FnDecl) -> bool {
    method.params.first().is_some_and(|p| p.name.name == "self")
}

/// Whether `method` takes `self` written without a type (`self`, `mut self`).
pub(super) fn takes_self(method: &flux_syntax::FnDecl) -> bool {
    method.params.first().is_some_and(|p| {
        p.name.name == "self" && matches!(&p.ty, TypeName::Simple { name, .. } if name == "Self")
    })
}

/// The name a type function of `impl Type { }` is called by: `Type::f`.
fn type_function_name(decl: &ImplDecl, method: &flux_syntax::FnDecl) -> String {
    format!("{}::{}", type_name_str(&decl.target_type), method.name.name)
}

/// Declares the type functions of an `impl Type { }` (methods without
/// `self`) under `Type::f`, before any body is lowered, so that every body
/// can call them.
pub(super) fn declare_type_functions(decl: &ImplDecl, symbols: &mut SymbolTable) {
    for method in decl.methods.iter().filter(|m| m.mut_self && takes_self(m)) {
        symbols.mut_method_names.insert(method.name.name.clone());
    }
    if decl.trait_name.is_some() {
        return;
    }
    for method in decl.methods.iter().filter(|m| !has_receiver(m)) {
        let _ = symbols.declare(
            &type_function_name(decl, method),
            SymbolKind::Function { reactive: false },
            method.name.span,
        );
    }
}

/// `ty` with `Self` replaced by `target`.
fn subst_self(ty: &TypeName, target: &TypeName) -> TypeName {
    let sub = |t: &TypeName| subst_self(t, target);
    match ty {
        TypeName::Simple { name, .. } if name == "Self" => target.clone(),
        TypeName::Simple { .. } => ty.clone(),
        TypeName::Generic { name, args, span } => TypeName::Generic {
            name: name.clone(),
            args: args.iter().map(sub).collect(),
            span: *span,
        },
        TypeName::Tuple { elems, span } => TypeName::Tuple {
            elems: elems.iter().map(sub).collect(),
            span: *span,
        },
        TypeName::Array { inner, span } => TypeName::Array {
            inner: Box::new(sub(inner)),
            span: *span,
        },
        TypeName::Option { inner, span } => TypeName::Option {
            inner: Box::new(sub(inner)),
            span: *span,
        },
        TypeName::Result { ok, err, span } => TypeName::Result {
            ok: Box::new(sub(ok)),
            err: Box::new(sub(err)),
            span: *span,
        },
        TypeName::Function { params, ret, span } => TypeName::Function {
            params: params.iter().map(sub).collect(),
            ret: ret.as_ref().map(|r| Box::new(sub(r))),
            span: *span,
        },
    }
}

pub(super) fn lower_impl_fns(
    decl: &ImplDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<Vec<HirItem>> {
    let target_name = type_name_str(&decl.target_type);
    symbols.pending_impls.push(decl.clone());
    let mut items = Vec::new();
    for method in &decl.methods {
        let fail = |message: String, diagnostics: &mut DiagnosticBag| {
            diagnostics.push(FluxError::InvalidImpl {
                message,
                span: span_to_miette(method.name.span),
                source_id: method.name.span.source,
            });
        };
        if !method.type_params.is_empty() {
            fail(
                format!(
                    "method `{}` cannot have type parameters yet",
                    method.name.name
                ),
                diagnostics,
            );
            continue;
        }
        let mut m = method.clone();
        // `Self` in the signature is the type the impl is for.
        for p in &mut m.params {
            p.ty = subst_self(&p.ty, &decl.target_type);
        }
        m.ret = m.ret.as_ref().map(|r| subst_self(r, &decl.target_type));
        // `mut self`: `self` is a mutable local that starts as the receiver,
        // and the method's value is the changed receiver, which the call
        // site stores back.
        if method.mut_self && takes_self(method) {
            if let Some(ret) = &method.ret {
                diagnostics.push(FluxError::InvalidImpl {
                    message: format!(
                        "`{}` changes its receiver (`mut self`) and cannot also return a value",
                        method.name.name
                    ),
                    span: span_to_miette(type_span(ret)),
                    source_id: method.name.span.source,
                });
                continue;
            }
            let span = m.params[0].span;
            m.params[0].name.name = "\0self".to_string();
            let mut stmts = vec![flux_syntax::Stmt::Let(LetStmt {
                pub_: false,
                mutable: true,
                pattern: LetPattern::Ident(Ident {
                    name: "self".to_string(),
                    span,
                }),
                ty: None,
                value: Expr::Ident {
                    name: "\0self".to_string(),
                    span,
                },
                span,
            })];
            let mut body = m.body.stmts.clone();
            return_self(&mut body);
            stmts.extend(body);
            stmts.push(flux_syntax::Stmt::Expr(Expr::Ident {
                name: "self".to_string(),
                span: m.body.span,
            }));
            m.body.stmts = stmts;
            m.ret = Some(decl.target_type.clone());
        }
        let method_name = &method.name.name;
        let sym = if decl.trait_name.is_none() && !has_receiver(method) {
            // A type function, declared before bodies were lowered.
            m.name.name = type_function_name(decl, method);
            if !symbols.lowered_type_fns.insert(m.name.name.clone()) {
                fail(
                    format!("`{target_name}` already has a function `{method_name}`"),
                    diagnostics,
                );
                continue;
            }
            match symbols.lookup(&m.name.name) {
                Some(sym) => sym.id,
                None => continue,
            }
        } else {
            if symbols
                .trait_impl_index
                .contains_key(&(target_name.clone(), method_name.clone()))
            {
                fail(
                    format!("`{target_name}` already has a method `{method_name}`"),
                    diagnostics,
                );
                continue;
            }
            let trait_part = decl.trait_name.as_ref().map_or("", |t| t.name.as_str());
            m.name.name = format!("__impl_{trait_part}_{target_name}_{method_name}");
            match symbols.declare(
                &m.name.name,
                SymbolKind::Function { reactive: false },
                method.name.span,
            ) {
                Ok(id) => id,
                Err(_) => continue,
            }
        };
        let previous = symbols.self_type.replace(target_name.clone());
        let writer = Writer::new(WriterKind::Method, method_name, None);
        let lowered = lower_fn_as(&m, symbols, diagnostics, false, writer);
        symbols.self_type = previous;
        if let Some(f) = lowered {
            if method.mut_self && takes_self(method) {
                symbols.mut_methods.insert(f.name);
            }
            // A method added to a library type is seen only where its file is.
            if decl.trait_name.is_none() && is_library_type(&target_name) && has_receiver(method) {
                symbols
                    .extension_sources
                    .insert(f.name, method.name.span.source);
            }
            if has_receiver(method) || decl.trait_name.is_some() {
                symbols
                    .trait_impl_index
                    .insert((target_name.clone(), method_name.clone()), f.name);
            }
            let _ = sym;
            items.push(HirItem::Fn(f));
        }
    }
    Some(items)
}

/// E020: an `impl` names a declared trait and defines exactly its methods,
/// with the trait's parameter count, non-receiver parameter types, and return
/// type. Traits have no `Self`, so the receiver's type is not compared; an
/// omitted return type is inferred and not compared either.
pub(super) fn validate_impls(
    impls: &[ImplDecl],
    items: &[HirItem],
    names: &std::collections::HashMap<crate::SymbolId, String>,
    diagnostics: &mut DiagnosticBag,
) {
    let traits: std::collections::HashMap<&str, &HirTrait> = items
        .iter()
        .filter_map(|item| match item {
            HirItem::Trait(t) => names.get(&t.name).map(|n| (n.as_str(), t)),
            _ => None,
        })
        .collect();
    let mut push = |message: String, span: flux_diagnostics::Span| {
        diagnostics.push(FluxError::InvalidImpl {
            message: crate::display_text(&message),
            span: span_to_miette(span),
            source_id: span.source,
        });
    };
    for imp in impls {
        // `impl Type { }` has no trait to match.
        let Some(trait_ident) = &imp.trait_name else {
            continue;
        };
        let trait_name = &trait_ident.name;
        let target = type_name_str(&imp.target_type);
        // `Self`, in the trait or the impl, is the type.
        let target_hir = lower_type(&imp.target_type);
        let key = |ty: &HirType| type_key(&subst_self_hir(ty, &target_hir));
        let Some(declared) = traits.get(trait_name.as_str()) else {
            push(format!("unknown trait `{trait_name}`"), trait_ident.span);
            continue;
        };
        for method in &imp.methods {
            let name = &method.name.name;
            let Some((_, params, ret)) = declared.methods.iter().find(|(n, ..)| n == name) else {
                push(
                    format!("trait `{trait_name}` has no method `{name}`"),
                    method.name.span,
                );
                continue;
            };
            if params.len() != method.params.len() {
                push(
                    format!(
                        "`{name}` takes {} parameter(s), but trait `{trait_name}` declares {}",
                        method.params.len(),
                        params.len()
                    ),
                    method.name.span,
                );
                continue;
            }
            for (want, param) in params.iter().zip(&method.params).skip(1) {
                let got = lower_type(&param.ty);
                if key(want) != key(&got) {
                    push(
                        format!(
                            "parameter `{}` of `{name}` is `{}`, but trait `{trait_name}` declares `{}`",
                            param.name.name,
                            key(&got),
                            key(want)
                        ),
                        param.span,
                    );
                }
            }
            if let (Some(want), Some(got)) = (ret, method.ret.as_ref().map(lower_type)) {
                if key(want) != key(&got) {
                    push(
                        format!(
                            "`{name}` returns `{}`, but trait `{trait_name}` declares `{}`",
                            key(&got),
                            key(want)
                        ),
                        method.name.span,
                    );
                }
            }
        }
        for (name, ..) in &declared.methods {
            if !imp.methods.iter().any(|m| &m.name.name == name) {
                push(
                    format!("impl of `{trait_name}` for `{target}` is missing method `{name}`"),
                    imp.span,
                );
            }
        }
    }
}

/// A type's spelling with `Int`/`Int64` and `Float`/`Float64` unified.
fn type_key(ty: &HirType) -> String {
    match ty {
        HirType::Named { name, .. } | HirType::TypeParam { name, .. } => match name.as_str() {
            "Int64" => "Int".into(),
            "Float64" => "Float".into(),
            _ => name.clone(),
        },
        HirType::Generic { name, args, .. } => format!(
            "{name}[{}]",
            args.iter().map(type_key).collect::<Vec<_>>().join(", ")
        ),
        HirType::Array { inner, .. } => format!("[{}]", type_key(inner)),
        HirType::Option { inner, .. } => format!("Option[{}]", type_key(inner)),
        HirType::Result { ok, err, .. } => format!("Result[{}, {}]", type_key(ok), type_key(err)),
        HirType::Function { params, ret, .. } => format!(
            "fn({}) -> {}",
            params.iter().map(type_key).collect::<Vec<_>>().join(", "),
            ret.as_ref()
                .map_or_else(|| "Unit".to_string(), |r| type_key(r))
        ),
    }
}

fn type_name_str(ty: &TypeName) -> String {
    match ty {
        TypeName::Simple { name, .. } => name.clone(),
        TypeName::Tuple { elems, .. } => format!(
            "Tuple_{}",
            elems
                .iter()
                .map(type_name_str)
                .collect::<Vec<_>>()
                .join("_")
        ),
        TypeName::Generic { name, args, .. } => {
            format!(
                "{name}_{}",
                args.iter().map(type_name_str).collect::<Vec<_>>().join("_")
            )
        }
        TypeName::Array { inner, .. } => format!("Array_{}", type_name_str(inner)),
        TypeName::Option { inner, .. } => format!("Option_{}", type_name_str(inner)),
        TypeName::Result { ok, err, .. } => {
            format!("Result_{}_{}", type_name_str(ok), type_name_str(err))
        }
        TypeName::Function { params, ret, .. } => {
            let params: Vec<String> = params.iter().map(type_name_str).collect();
            let ret = ret
                .as_ref()
                .map_or_else(|| "Unit".to_string(), |r| type_name_str(r));
            format!("Fn_{}_{ret}", params.join("_"))
        }
    }
}

pub(super) fn lower_struct(
    decl: &flux_syntax::StructDecl,
    symbols: &mut SymbolTable,
) -> Option<HirStruct> {
    let sym = symbols.lookup(&decl.name.name)?;
    let fields = decl
        .fields
        .iter()
        .map(|f| {
            (
                f.name.name.clone(),
                crate::nodes::lower_type_with_params(&f.ty, &decl.type_params),
            )
        })
        .collect();
    Some(HirStruct {
        name: sym.id,
        type_params: decl.type_params.clone(),
        fields,
        span: decl.span,
    })
}

fn lower_enum_payload(payload: &flux_syntax::EnumPayload, params: &[String]) -> HirEnumPayload {
    let lower = |t| crate::nodes::lower_type_with_params(t, params);
    match payload {
        flux_syntax::EnumPayload::Tuple(ty) => HirEnumPayload::Tuple(lower(ty)),
        flux_syntax::EnumPayload::Struct(fields) => HirEnumPayload::Struct(
            fields
                .iter()
                .map(|(n, t)| (n.name.clone(), lower(t)))
                .collect(),
        ),
    }
}

pub(super) fn lower_enum(
    decl: &flux_syntax::EnumDecl,
    symbols: &mut SymbolTable,
) -> Option<HirEnum> {
    let sym = symbols.lookup(&decl.name.name)?;
    let variants = decl
        .variants
        .iter()
        .map(|v| {
            (
                v.name.name.clone(),
                v.payload
                    .as_ref()
                    .map(|p| lower_enum_payload(p, &decl.type_params)),
            )
        })
        .collect();
    Some(HirEnum {
        name: sym.id,
        type_params: decl.type_params.clone(),
        variants,
        span: decl.span,
    })
}

pub(super) fn lower_module_if(
    stmt: &IfStmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirIf> {
    lower_if(stmt, symbols, diagnostics)
}

pub(super) fn lower_const_binding(
    binding: &ConstBinding,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirBinding> {
    symbols.const_bindings.insert(binding.name.name.clone());
    let mut reads_reactive = false;
    let mut value = lower_expr(
        &binding.value,
        symbols,
        ScopeKind::Module,
        diagnostics,
        &mut reads_reactive,
    )?;
    if reads_reactive {
        diagnostics.push(FluxError::InvalidControlFlowInDerived {
            construct: "reactive read in const".into(),
            span: span_to_miette(binding.span),
            source_id: binding.span.source,
        });
        return None;
    }
    if let Some(folded) = fold_const_hir_expr(&value) {
        value = folded;
    }
    let sym_id = symbols.lookup(&binding.name.name)?.id;
    symbols.update_kind(&binding.name.name, SymbolKind::Source);
    Some(HirBinding {
        name: sym_id,
        kind: BindingKind::Source,
        ty: binding.ty.as_ref().map(lower_type),
        value,
        span: binding.span,
    })
}

pub(super) fn lower_module_let(
    stmt: &LetStmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reassigned_sources: &std::collections::HashSet<String>,
) -> Option<HirItem> {
    if stmt.mutable {
        return lower_module_binding(
            &stmt.pattern,
            stmt.ty.as_ref(),
            &stmt.value,
            stmt.span,
            true,
            false,
            symbols,
            diagnostics,
        );
    }
    let mut reads_reactive = false;
    let pending_fns = symbols.pending_fns.len();
    let _ = super::exprs::lower_expr(
        &stmt.value,
        symbols,
        ScopeKind::Module,
        diagnostics,
        &mut reads_reactive,
    );
    symbols.pending_fns.truncate(pending_fns);
    if diagnostics.has_errors() {
        return None;
    }
    if let Some(name) = stmt.pattern.primary_name() {
        if reassigned_sources.contains(name) && !reads_reactive {
            diagnostics.push(FluxError::ImmutableLetReadsReactive {
                name: name.to_string(),
                assigned: true,
                span: span_to_miette(stmt.span),
                source_id: stmt.span.source,
            });
            return None;
        }
    }
    lower_module_binding(
        &stmt.pattern,
        stmt.ty.as_ref(),
        &stmt.value,
        stmt.span,
        false,
        false,
        symbols,
        diagnostics,
    )
}

pub(super) fn lower_derived(
    stmt: &DerivedStmt,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirItem> {
    lower_module_binding(
        &stmt.pattern,
        stmt.ty.as_ref(),
        &stmt.value,
        stmt.span,
        false,
        true,
        symbols,
        diagnostics,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_module_binding(
    pattern: &LetPattern,
    ty: Option<&TypeName>,
    value: &Expr,
    span: flux_diagnostics::Span,
    mutable: bool,
    force_derived: bool,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirItem> {
    match pattern {
        LetPattern::Ident(name) => lower_binding(
            name,
            ty,
            value,
            span,
            mutable,
            force_derived,
            symbols,
            diagnostics,
        )
        .map(HirItem::Binding),
        LetPattern::Struct { span: pat_span, .. } | LetPattern::Tuple { span: pat_span, .. } => {
            let mut reads_reactive = false;
            let base = lower_expr(
                value,
                symbols,
                ScopeKind::Module,
                diagnostics,
                &mut reads_reactive,
            )?;
            // Each name with the expression for its part of the value.
            let mut parts = Vec::new();
            match pattern {
                LetPattern::Struct { fields, .. } => {
                    for field in fields {
                        let field_expr = HirExpr::Field {
                            base: Box::new(base.clone()),
                            field: field.name.clone(),
                            span: *pat_span,
                        };
                        parts.push((field.clone(), field_expr));
                    }
                }
                _ => super::stmts::tuple_parts(pattern, &base, symbols, &mut parts),
            }
            // `let (a, b): (A, B) = …`: each name gets its part of the type.
            let mut leaf_types = HashMap::new();
            if let (Some(ty), LetPattern::Tuple { .. }) = (ty, pattern) {
                annotated_leaves(pattern, ty, &mut leaf_types, diagnostics);
            }
            let mut items = Vec::new();
            for (field, field_expr) in parts {
                // A shape check: each name's part already checks its path.
                if field.name == "_" {
                    continue;
                }
                let sym_id = symbols.lookup(&field.name)?.id;
                let kind = if force_derived || (!mutable && reads_reactive) {
                    BindingKind::Derived
                } else {
                    BindingKind::Source
                };
                symbols.update_kind(
                    &field.name,
                    match kind {
                        BindingKind::Source => SymbolKind::Source,
                        BindingKind::Derived => SymbolKind::Derived,
                    },
                );
                let value = match kind {
                    BindingKind::Source => fold_const_hir_expr(&field_expr).unwrap_or(field_expr),
                    BindingKind::Derived => field_expr,
                };
                items.push(HirItem::Binding(HirBinding {
                    name: sym_id,
                    kind,
                    ty: leaf_types.get(&field.name).map(lower_type),
                    value,
                    span: *pat_span,
                }));
            }
            let first = items.first()?.clone();
            symbols.pending_bindings.extend(items.into_iter().skip(1));
            Some(first)
        }
    }
}

/// The part of the annotation `ty` each name of a tuple pattern gets; reports
/// an annotation whose shape differs from the pattern's.
fn annotated_leaves(
    pattern: &LetPattern,
    ty: &TypeName,
    out: &mut HashMap<String, TypeName>,
    diagnostics: &mut DiagnosticBag,
) {
    match (pattern, ty) {
        (LetPattern::Ident(name), _) if name.name != "_" => {
            out.insert(name.name.clone(), ty.clone());
        }
        (LetPattern::Ident(_), _) => {}
        (LetPattern::Tuple { elems, .. }, TypeName::Tuple { elems: types, .. })
            if elems.len() == types.len() =>
        {
            for (elem, ty) in elems.iter().zip(types) {
                annotated_leaves(elem, ty, out, diagnostics);
            }
        }
        (pattern, ty) => {
            let span = pattern.span();
            diagnostics.push(FluxError::TypeMismatch {
                expected: format!("a tuple type with {} values", pattern_width(pattern)),
                found: match ty {
                    TypeName::Tuple { elems, .. } => {
                        format!("a tuple type with {} values", elems.len())
                    }
                    _ => "a type that is not a tuple".to_string(),
                },
                span: span_to_miette(span),
                source_id: span.source,
            });
        }
    }
}

fn pattern_width(pattern: &LetPattern) -> usize {
    match pattern {
        LetPattern::Tuple { elems, .. } => elems.len(),
        _ => 1,
    }
}

pub(super) fn lower_event(
    decl: &EventDecl,
    symbols: &SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirItem> {
    let sym = symbols.lookup(&decl.name.name)?;
    for (i, p) in decl.params.iter().enumerate() {
        if let Some(prev) = decl.params[..i].iter().find(|q| q.name.name == p.name.name) {
            diagnostics.push(FluxError::DuplicateDefinition {
                name: p.name.name.clone(),
                prev_span: span_to_miette(prev.span),
                span: span_to_miette(p.span),
                source_id: p.span.source,
            });
        }
    }
    for (i, p) in decl.params.iter().enumerate() {
        if let Some(prev) = decl.params[..i].iter().find(|q| q.name.name == p.name.name) {
            diagnostics.push(FluxError::DuplicateDefinition {
                name: p.name.name.clone(),
                prev_span: span_to_miette(prev.span),
                span: span_to_miette(p.span),
                source_id: p.span.source,
            });
        }
    }
    let fields = decl
        .params
        .iter()
        .map(|p| EventField {
            name: p.name.name.clone(),
            ty: lower_type(&p.ty),
            span: p.span,
        })
        .collect();
    Some(HirItem::Event(HirEvent {
        name: sym.id,
        fields,
        span: decl.span,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_binding(
    name: &Ident,
    ty: Option<&TypeName>,
    value: &Expr,
    span: flux_diagnostics::Span,
    mutable: bool,
    force_derived: bool,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<HirBinding> {
    let sym_id = symbols.lookup(&name.name)?.id;
    let mut reads_reactive = false;
    let mut lowered = lower_expr(
        value,
        symbols,
        ScopeKind::Module,
        diagnostics,
        &mut reads_reactive,
    )?;

    let kind = if force_derived || (!mutable && reads_reactive) {
        BindingKind::Derived
    } else {
        if let Some(folded) = fold_const_hir_expr(&lowered) {
            lowered = folded;
        }
        BindingKind::Source
    };

    symbols.update_kind(
        &name.name,
        match kind {
            BindingKind::Source => SymbolKind::Source,
            BindingKind::Derived => SymbolKind::Derived,
        },
    );

    Some(HirBinding {
        name: sym_id,
        kind,
        ty: ty.map(lower_type),
        value: lowered,
        span,
    })
}

fn type_span(ty: &TypeName) -> flux_diagnostics::Span {
    match ty {
        TypeName::Simple { span, .. }
        | TypeName::Generic { span, .. }
        | TypeName::Tuple { span, .. }
        | TypeName::Array { span, .. }
        | TypeName::Option { span, .. }
        | TypeName::Result { span, .. }
        | TypeName::Function { span, .. } => *span,
    }
}

/// A type the language provides (as opposed to a `struct` or `enum`).
fn is_library_type(name: &str) -> bool {
    matches!(
        name,
        "Int"
            | "Int8"
            | "Int16"
            | "Int32"
            | "Int64"
            | "UInt8"
            | "UInt16"
            | "UInt32"
            | "UInt64"
            | "Float"
            | "Float32"
            | "Float64"
            | "Bool"
            | "Char"
            | "String"
    )
}

/// `ty` with `Self` replaced by `target`.
fn subst_self_hir(ty: &HirType, target: &HirType) -> HirType {
    let sub = |t: &HirType| subst_self_hir(t, target);
    match ty {
        HirType::Named { name, .. } if name == "Self" => target.clone(),
        HirType::Named { .. } | HirType::TypeParam { .. } => ty.clone(),
        HirType::Generic { name, args, span } => HirType::Generic {
            name: name.clone(),
            args: args.iter().map(sub).collect(),
            span: *span,
        },
        HirType::Array { inner, span } => HirType::Array {
            inner: Box::new(sub(inner)),
            span: *span,
        },
        HirType::Option { inner, span } => HirType::Option {
            inner: Box::new(sub(inner)),
            span: *span,
        },
        HirType::Result { ok, err, span } => HirType::Result {
            ok: Box::new(sub(ok)),
            err: Box::new(sub(err)),
            span: *span,
        },
        HirType::Function { params, ret, span } => HirType::Function {
            params: params.iter().map(sub).collect(),
            ret: ret.as_ref().map(|r| Box::new(sub(r))),
            span: *span,
        },
    }
}

/// In a `mut self` method, a bare `return` leaves with the receiver as it is
/// then: `return self`.
fn return_self(stmts: &mut [flux_syntax::Stmt]) {
    use flux_syntax::{ElseBranch, Stmt};
    fn in_expr(expr: &mut Expr) {
        if let Expr::Match { arms, .. } = expr {
            for arm in arms {
                return_self(&mut arm.stmts);
            }
        }
    }
    fn in_if(stmt: &mut IfStmt) {
        return_self(&mut stmt.then_branch.stmts);
        match &mut stmt.else_branch {
            Some(ElseBranch::Block(block)) => return_self(&mut block.stmts),
            Some(ElseBranch::If(next)) => in_if(next),
            None => {}
        }
    }
    for stmt in stmts {
        match stmt {
            Stmt::Return(ret) if ret.value.is_none() => {
                ret.value = Some(Expr::Ident {
                    name: "self".to_string(),
                    span: ret.span,
                });
            }
            Stmt::If(branch) => in_if(branch),
            Stmt::While(w) => return_self(&mut w.body.stmts),
            Stmt::For(f) => return_self(&mut f.body.stmts),
            Stmt::Expr(e) => in_expr(e),
            Stmt::Let(l) => in_expr(&mut l.value),
            _ => {}
        }
    }
}
