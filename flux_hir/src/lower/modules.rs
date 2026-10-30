//! Lowering imported module exports into the root program symbol table.

use std::path::Path;

use flux_diagnostics::DiagnosticBag;
use flux_resolve::LinkedProgram;
use flux_syntax::{Item, Program};

use crate::nodes::HirItem;
use crate::symbol::{SymbolKind, SymbolTable};

use super::{lower_item, push_duplicate};

pub(super) fn register_imports(
    linked: &LinkedProgram,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    for import in &linked.imports {
        symbols.module_aliases.insert(import.alias.clone(), ());
        let module = &linked.modules[import.module_index];
        if is_stdlib_module_path(&module.path) {
            continue;
        }
        let exports = pub_items(&module.ast);
        let selected: Vec<&Item> = if import.symbols.is_empty() {
            exports
        } else {
            exports
                .into_iter()
                .filter(|item| {
                    item_export_name(item)
                        .map(|n| import.symbols.contains(&n))
                        .unwrap_or(false)
                })
                .collect()
        };

        for item in selected {
            let export_name = item_export_name(item).expect("pub export");
            let mangled = format!("__mod_{}__{export_name}", import.alias);
            if let Some(err) = declare_item_renamed(item, &mangled, symbols) {
                push_duplicate(diagnostics, item, err);
            }
            if let Some(sym) = symbols.lookup(&mangled) {
                let sym_id = sym.id;
                symbols
                    .qualified_symbols
                    .insert((import.alias.clone(), export_name.clone()), sym_id);
                if import.symbols.is_empty() || import.symbols.contains(&export_name) {
                    symbols.reexport_alias(&export_name, sym_id, import.span);
                }
            }
        }
    }
    !diagnostics.has_errors()
}

pub(super) fn lower_imported_items(
    linked: &LinkedProgram,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reassigned_sources: &std::collections::HashSet<String>,
) -> Vec<HirItem> {
    let mut items = Vec::new();
    for import in &linked.imports {
        let module = &linked.modules[import.module_index];
        if is_stdlib_module_path(&module.path) {
            continue;
        }
        for item in pub_items(&module.ast) {
            let export_name = item_export_name(item).expect("pub export");
            if !import.symbols.is_empty() && !import.symbols.contains(&export_name) {
                continue;
            }
            let mangled = format!("__mod_{}__{export_name}", import.alias);
            if let Some(hir_item) =
                lower_item_renamed(item, &mangled, symbols, diagnostics, reassigned_sources)
            {
                items.push(hir_item);
            }
            if let Item::Impl(impl_decl) = item {
                if let Some(extra) = super::decls::lower_impl_fns(impl_decl, symbols, diagnostics) {
                    items.extend(extra);
                }
            }
        }
    }
    items
}

fn is_stdlib_module_path(path: &Path) -> bool {
    path.parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "stdlib")
}

fn pub_items(program: &Program) -> Vec<&Item> {
    program
        .items
        .iter()
        .filter(|item| item_is_pub(item))
        .collect()
}

fn item_is_pub(item: &Item) -> bool {
    match item {
        Item::Fn(f) => f.pub_,
        Item::RFn(f) => f.pub_,
        Item::Struct(s) => s.pub_,
        Item::Enum(e) => e.pub_,
        Item::Trait(t) => t.pub_,
        Item::Event(e) => e.pub_,
        Item::Let(l) => l.pub_,
        Item::Derived(d) => d.pub_,
        Item::Const(c) => c.pub_,
        _ => false,
    }
}

fn item_export_name(item: &Item) -> Option<String> {
    match item {
        Item::Fn(f) => Some(f.name.name.clone()),
        Item::RFn(f) => Some(f.name.name.clone()),
        Item::Struct(s) => Some(s.name.name.clone()),
        Item::Enum(e) => Some(e.name.name.clone()),
        Item::Trait(t) => Some(t.name.name.clone()),
        Item::Event(e) => Some(e.name.name.clone()),
        Item::Let(l) => l.pattern.primary_name().map(|n| n.to_string()),
        Item::Derived(d) => d.pattern.primary_name().map(|n| n.to_string()),
        Item::Const(c) => Some(c.name.name.clone()),
        _ => None,
    }
}

fn declare_item_renamed(
    item: &Item,
    mangled: &str,
    symbols: &mut SymbolTable,
) -> Option<crate::symbol::Symbol> {
    use flux_syntax::{ConstBinding, EnumDecl, EventDecl, FnDecl, RFnDecl, StructDecl, TraitDecl};
    match item {
        Item::Event(EventDecl { name, .. }) => {
            symbols.declare(mangled, SymbolKind::Event, name.span).err()
        }
        Item::Fn(FnDecl { name, .. }) => symbols
            .declare(mangled, SymbolKind::Function { reactive: false }, name.span)
            .err(),
        Item::RFn(RFnDecl { name, .. }) => symbols
            .declare(mangled, SymbolKind::Function { reactive: true }, name.span)
            .err(),
        Item::Struct(StructDecl { name, .. }) => symbols
            .declare(mangled, SymbolKind::Builtin, name.span)
            .err(),
        Item::Enum(EnumDecl { name, .. }) => symbols
            .declare(mangled, SymbolKind::Builtin, name.span)
            .err(),
        Item::Trait(TraitDecl { name, .. }) => symbols
            .declare(mangled, SymbolKind::Builtin, name.span)
            .err(),
        Item::Let(l) => symbols
            .declare(mangled, SymbolKind::Source, l.pattern.span())
            .err(),
        Item::Derived(d) => symbols
            .declare(mangled, SymbolKind::Source, d.pattern.span())
            .err(),
        Item::Const(ConstBinding { name, .. }) => symbols
            .declare(mangled, SymbolKind::Source, name.span)
            .err(),
        _ => None,
    }
}

fn lower_item_renamed(
    item: &Item,
    mangled: &str,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    reassigned_sources: &std::collections::HashSet<String>,
) -> Option<HirItem> {
    use flux_syntax::{DerivedStmt, FnDecl, LetStmt, RFnDecl};
    match item {
        Item::Fn(FnDecl { .. }) => {
            let mut renamed = match item {
                Item::Fn(f) => f.clone(),
                _ => unreachable!(),
            };
            renamed.name.name = mangled.to_string();
            super::funcs::lower_fn(&renamed, symbols, diagnostics, false).map(HirItem::Fn)
        }
        Item::RFn(RFnDecl { .. }) => {
            let mut renamed = match item {
                Item::RFn(f) => f.clone(),
                _ => unreachable!(),
            };
            renamed.name.name = mangled.to_string();
            super::funcs::lower_rfn(&renamed, symbols, diagnostics).map(HirItem::Fn)
        }
        Item::Let(LetStmt { .. }) => {
            let mut renamed = match item {
                Item::Let(l) => l.clone(),
                _ => unreachable!(),
            };
            renamed.pattern = flux_syntax::LetPattern::Ident(flux_syntax::Ident {
                name: mangled.to_string(),
                span: renamed.pattern.span(),
            });
            lower_item(
                &Item::Let(renamed),
                symbols,
                diagnostics,
                reassigned_sources,
            )
        }
        Item::Derived(DerivedStmt { .. }) => {
            let mut renamed = match item {
                Item::Derived(d) => d.clone(),
                _ => unreachable!(),
            };
            renamed.pattern = flux_syntax::LetPattern::Ident(flux_syntax::Ident {
                name: mangled.to_string(),
                span: renamed.pattern.span(),
            });
            super::decls::lower_derived(&renamed, symbols, diagnostics)
        }
        Item::Const(decl) => {
            let mut renamed = decl.clone();
            renamed.name.name = mangled.to_string();
            super::decls::lower_const_binding(&renamed, symbols, diagnostics).map(HirItem::Binding)
        }
        Item::Struct(decl) => super::decls::lower_struct(decl, symbols).map(HirItem::Struct),
        Item::Enum(decl) => {
            for v in &decl.variants {
                symbols
                    .enum_variants
                    .insert(v.name.name.clone(), mangled.to_string());
            }
            super::decls::lower_enum(decl, symbols).map(HirItem::Enum)
        }
        Item::Trait(decl) => super::decls::lower_trait(decl, symbols).map(HirItem::Trait),
        Item::Event(decl) => super::decls::lower_event(decl, symbols),
        Item::Impl(_) | Item::Watch(_) | Item::On(_) | Item::If(_) | Item::Import(_) => None,
        Item::TypeAlias(_) => None,
    }
}
