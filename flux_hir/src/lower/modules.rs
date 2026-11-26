//! Lowering imported modules, each in its own namespace.

use std::path::Path;

use flux_diagnostics::DiagnosticBag;
use flux_resolve::LinkedProgram;
use flux_syntax::{Item, Program};

use crate::context::ScopeKind;
use crate::nodes::HirItem;
use crate::symbol::{SymbolKind, SymbolTable};

use super::{lower_item, push_duplicate};

/// Internal name of item `name` of module `index` (modules other than the
/// root). Unique per module, so private names never collide.
fn mangled(index: usize, name: &str) -> String {
    format!("__mod_{index}__{name}")
}

fn is_type_item(item: &Item) -> bool {
    matches!(
        item,
        Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::TypeAlias(_) | Item::Impl(_)
    )
}

/// Pass 1 for imported modules: declare every item of every non-root module.
/// Values and functions get module-unique names (private ones too), and
/// types already have theirs (`type_scope`). Runs before any root name exists.
pub(super) fn declare_module_items(
    linked: &LinkedProgram,
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
) {
    for (index, module) in linked.modules.iter().enumerate().skip(1) {
        if is_stdlib_module_path(&module.path) {
            continue;
        }
        for item in &module.ast.items {
            match item {
                // A module's handlers, watches, and top-level `if`s are
                // lowered with its other items, in its own scope.
                Item::Watch(_) | Item::On(_) | Item::If(_) => {}
                Item::Impl(decl) => super::decls::declare_type_functions(decl, symbols),
                Item::Import(_) | Item::TypeAlias(_) => {}
                Item::Struct(_) | Item::Enum(_) | Item::Trait(_) => {
                    if let Some(err) = super::declare_item(item, symbols) {
                        push_duplicate(diagnostics, item, err);
                    }
                    // Which bare variants a module can use is set per module
                    // when it is lowered (`type_scope::Scoped::variants`);
                    // `Enum::Variant` works through these sets.
                    if let Item::Enum(e) = item {
                        for v in &e.variants {
                            symbols
                                .enum_variant_sets
                                .entry(e.name.name.clone())
                                .or_default()
                                .insert(v.name.name.clone());
                        }
                    }
                }
                _ => {
                    let Some(name) = item_export_name(item) else {
                        continue;
                    };
                    let declared = match item {
                        Item::Live(l) => super::live::declare_live(
                            l,
                            &mangled(index, &name),
                            symbols,
                            diagnostics,
                        ),
                        Item::Extern(e) => {
                            super::declare_extern(&mangled(index, &name), e.name.span, symbols)
                        }
                        _ => declare_item_renamed(item, &mangled(index, &name), symbols),
                    };
                    if let Some(err) = declared {
                        push_duplicate(diagnostics, item, err);
                    }
                }
            }
        }
    }
}

/// Makes the names `import` brings in visible in the current scope (and its
/// `alias.name` form in the current qualified view).
fn bind_import(
    import: &flux_resolve::BoundImport,
    linked: &LinkedProgram,
    symbols: &mut SymbolTable,
) {
    symbols.module_aliases.insert(import.alias.clone(), ());
    let module = &linked.modules[import.module_index];
    if import.module_index == 0 || is_stdlib_module_path(&module.path) {
        return;
    }
    for item in pub_items(&module.ast) {
        if is_type_item(item) {
            // Type names are program-global; `alias::Type { … }` checks that
            // the module exports it.
            if let Some(name) = item_export_name(item) {
                symbols.module_types.insert((import.alias.clone(), name));
            }
            continue;
        }
        let Some(name) = item_export_name(item) else {
            continue;
        };
        if !import.symbols.is_empty() && !import.symbols.contains(&name) {
            continue;
        }
        let Some(id) = symbols
            .lookup(&mangled(import.module_index, &name))
            .map(|s| s.id)
        else {
            continue;
        };
        symbols
            .qualified_symbols
            .insert((import.alias.clone(), name.clone()), id);
        symbols.reexport_alias(&name, id, import.span);
    }
}

/// The root module's imports, bound into the root scope.
pub(super) fn bind_root_imports(linked: &LinkedProgram, symbols: &mut SymbolTable) {
    for import in linked.module_imports.first().into_iter().flatten() {
        bind_import(import, linked, symbols);
    }
}

/// Pass 2 for imported modules: lower each module's items inside its own
/// scope, which sees exactly its items, its stdlib imports, and its user
/// imports (each module also has its own `alias.name` view).
pub(super) fn lower_module_items(
    linked: &LinkedProgram,
    variants: &[std::collections::HashMap<String, String>],
    symbols: &mut SymbolTable,
    diagnostics: &mut DiagnosticBag,
    _root_reassigned: &std::collections::HashSet<String>,
) -> Vec<HirItem> {
    let mut items = Vec::new();
    for (index, module) in linked.modules.iter().enumerate().skip(1) {
        if is_stdlib_module_path(&module.path) {
            continue;
        }
        // Only a module's own code changes its sources.
        let reassigned_sources = &super::collect_reassigned_sources(&module.ast);
        let saved_aliases = std::mem::take(&mut symbols.module_aliases);
        let saved_qualified = std::mem::take(&mut symbols.qualified_symbols);
        let saved_variants = std::mem::replace(
            &mut symbols.enum_variants,
            variants.get(index).cloned().unwrap_or_default(),
        );
        symbols.push(ScopeKind::Module);

        for (import, target) in module.imports.iter().zip(&module.import_targets) {
            if target.is_none() {
                super::register_stdlib_import(import, symbols, diagnostics);
            }
        }
        for item in &module.ast.items {
            if is_type_item(item) {
                continue;
            }
            let Some(name) = item_export_name(item) else {
                continue;
            };
            if let Some(id) = symbols.lookup(&mangled(index, &name)).map(|s| s.id) {
                symbols.reexport_alias(&name, id, flux_diagnostics::Span::DUMMY);
            }
        }
        for import in &linked.module_imports[index] {
            bind_import(import, linked, symbols);
        }
        super::declare_prelude(symbols, &module.ast.items);

        for item in &module.ast.items {
            match item {
                Item::Import(_) | Item::TypeAlias(_) => {}
                Item::Watch(_) | Item::On(_) | Item::If(_) => {
                    if let Some(hir_item) =
                        super::lower_item(item, symbols, diagnostics, reassigned_sources)
                    {
                        items.push(hir_item);
                    }
                }
                Item::Impl(impl_decl) => {
                    if let Some(extra) =
                        super::decls::lower_impl_fns(impl_decl, symbols, diagnostics)
                    {
                        items.extend(extra);
                    }
                }
                Item::Struct(_) | Item::Enum(_) | Item::Trait(_) => {
                    if let Some(hir_item) = lower_type_item(item, symbols) {
                        items.push(hir_item);
                    }
                }
                _ => {
                    let Some(name) = item_export_name(item) else {
                        continue;
                    };
                    if let Some(hir_item) = lower_item_renamed(
                        item,
                        &mangled(index, &name),
                        symbols,
                        diagnostics,
                        reassigned_sources,
                    ) {
                        items.push(hir_item);
                        items.extend(std::mem::take(&mut symbols.pending_bindings));
                    }
                }
            }
        }

        symbols.pop();
        symbols.module_aliases = saved_aliases;
        symbols.qualified_symbols = saved_qualified;
        symbols.enum_variants = saved_variants;
    }
    items
}

fn lower_type_item(item: &Item, symbols: &mut SymbolTable) -> Option<HirItem> {
    match item {
        Item::Struct(decl) => super::decls::lower_struct(decl, symbols).map(HirItem::Struct),
        Item::Enum(decl) => super::decls::lower_enum(decl, symbols).map(HirItem::Enum),
        Item::Trait(decl) => super::decls::lower_trait(decl, symbols).map(HirItem::Trait),
        _ => None,
    }
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

pub(super) fn item_is_pub(item: &Item) -> bool {
    match item {
        Item::Fn(f) => f.pub_,
        Item::Live(l) => l.pub_,
        Item::Extern(e) => e.pub_,
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

pub(super) fn item_export_name(item: &Item) -> Option<String> {
    match item {
        Item::Fn(f) => Some(f.name.name.clone()),
        Item::Live(l) => Some(l.name.name.clone()),
        Item::Extern(e) => Some(e.name.name.clone()),
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

pub(super) fn declare_item_renamed(
    item: &Item,
    mangled: &str,
    symbols: &mut SymbolTable,
) -> Option<crate::symbol::Symbol> {
    use flux_syntax::{ConstBinding, EnumDecl, EventDecl, FnDecl, RFnDecl, StructDecl, TraitDecl};
    if let Item::Fn(f) = item {
        super::funcs::note_mut_params(symbols, mangled, f);
    }
    match item {
        Item::Event(EventDecl { name, .. }) => {
            symbols.declare(mangled, SymbolKind::Event, name.span).err()
        }
        Item::Fn(FnDecl { name, kind, .. }) => symbols
            .declare(
                mangled,
                SymbolKind::Function {
                    reactive: *kind == flux_syntax::FnKind::Track,
                },
                name.span,
            )
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
        Item::Const(ConstBinding { name, .. }) => {
            match symbols.declare(mangled, SymbolKind::Source, name.span) {
                Ok(id) => {
                    symbols.constants.insert(id);
                    None
                }
                Err(prev) => Some(prev),
            }
        }
        _ => None,
    }
}

pub(super) fn lower_item_renamed(
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
        Item::Event(decl) => super::decls::lower_event(decl, symbols, diagnostics),
        Item::Live(decl) => super::live::lower_live(decl, mangled, symbols, diagnostics),
        Item::Extern(decl) => {
            if !super::extern_params_ok(decl, diagnostics) {
                return None;
            }
            super::funcs::lower_fn(
                &super::extern_as_fn(decl, mangled),
                symbols,
                diagnostics,
                false,
            )
            .map(HirItem::Fn)
        }
        Item::Impl(_) | Item::Watch(_) | Item::On(_) | Item::If(_) | Item::Import(_) => None,
        Item::TypeAlias(_) | Item::Struct(_) | Item::Enum(_) | Item::Trait(_) => None,
    }
}
