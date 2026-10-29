//! Lowering of top-level declarations: structs, enums, traits, impls, events,
//! and module-level bindings / source reassignments.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError};
use flux_syntax::{
    ConstBinding, DerivedStmt, EventDecl, Expr, Ident, IfStmt, ImplDecl, LetPattern, LetStmt,
    TraitDecl, TypeName,
};

use crate::context::ScopeKind;
use crate::nodes::{lower_type, *};
use crate::symbol::{BindingKind, SymbolKind, SymbolTable};

use super::constfold::fold_const_hir_expr;
use super::exprs::{infer_syntax_elem_type, lower_expr};
use super::funcs::lower_fn;
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

pub(super) fn lower_impl_fns(
    decl: &ImplDecl,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> Option<Vec<HirItem>> {
    let target_name = type_name_str(&decl.target_type);
    let mut items = Vec::new();
    for method in &decl.methods {
        let mangled = format!(
            "__impl_{}_{}_{}",
            decl.trait_name.name, target_name, method.name.name
        );
        if symbols
            .declare(
                &mangled,
                SymbolKind::Function { reactive: false },
                method.span,
            )
            .is_err()
        {
            continue;
        }
        let mut m = method.clone();
        m.name.name = mangled;
        if let Some(f) = lower_fn(&m, symbols, diagnostics, false) {
            symbols
                .trait_impl_index
                .insert((target_name.clone(), method.name.name.clone()), f.name);
            items.push(HirItem::Fn(f));
        }
    }
    Some(items)
}

fn type_name_str(ty: &TypeName) -> String {
    match ty {
        TypeName::Simple { name, .. } => name.clone(),
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
        .map(|f| (f.name.name.clone(), lower_type(&f.ty)))
        .collect();
    Some(HirStruct {
        name: sym.id,
        type_params: decl.type_params.clone(),
        fields,
        span: decl.span,
    })
}

fn lower_enum_payload(payload: &flux_syntax::EnumPayload) -> HirEnumPayload {
    match payload {
        flux_syntax::EnumPayload::Tuple(ty) => HirEnumPayload::Tuple(lower_type(ty)),
        flux_syntax::EnumPayload::Struct(fields) => HirEnumPayload::Struct(
            fields
                .iter()
                .map(|(n, t)| (n.name.clone(), lower_type(t)))
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
                v.payload.as_ref().map(lower_enum_payload),
            )
        })
        .collect();
    Some(HirEnum {
        name: sym.id,
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
    let _ = super::exprs::lower_expr(
        &stmt.value,
        symbols,
        ScopeKind::Module,
        diagnostics,
        &mut reads_reactive,
    );
    if diagnostics.has_errors() {
        return None;
    }
    if let Some(name) = stmt.pattern.primary_name() {
        if reassigned_sources.contains(name) && !reads_reactive {
            diagnostics.push(FluxError::ImmutableLetReadsReactive {
                name: name.to_string(),
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
        LetPattern::Struct {
            name,
            fields,
            span: pat_span,
        } => {
            let mut reads_reactive = false;
            let base = lower_expr(
                value,
                symbols,
                ScopeKind::Module,
                diagnostics,
                &mut reads_reactive,
            )?;
            let mut items = Vec::new();
            for field in fields {
                let field_expr = HirExpr::Field {
                    base: Box::new(base.clone()),
                    field: field.name.clone(),
                    span: *pat_span,
                };
                let sym_id = symbols.lookup(&field.name)?.id;
                let kind = if force_derived || reads_reactive {
                    BindingKind::Derived
                } else if mutable || fold_const_hir_expr(&field_expr).is_some() {
                    BindingKind::Source
                } else {
                    BindingKind::Derived
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
                    ty: None,
                    value,
                    span: *pat_span,
                }));
            }
            let _ = name;
            let first = items.first()?.clone();
            symbols.pending_bindings.extend(items.into_iter().skip(1));
            Some(first)
        }
    }
}

pub(super) fn lower_event(decl: &EventDecl, symbols: &SymbolTable) -> Option<HirItem> {
    let sym = symbols.lookup(&decl.name.name)?;
    let payload = decl.params.first().map(|p| EventField {
        name: p.name.name.clone(),
        ty: lower_type(&p.ty),
        span: p.span,
    });
    Some(HirItem::Event(HirEvent {
        name: sym.id,
        payload,
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
    if let Some(elem_ty) = infer_syntax_elem_type(value, symbols) {
        symbols.array_elem_hints.insert(name.name.clone(), elem_ty);
    }
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
    } else if mutable {
        BindingKind::Source
    } else if let Some(folded) = fold_const_hir_expr(&lowered) {
        lowered = folded;
        BindingKind::Source
    } else {
        BindingKind::Derived
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
