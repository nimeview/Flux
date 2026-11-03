//! Per-compilation type registries.
//!
//! [`TypeCheckContext`] mirrors data already present on [`HirProgram`] but is
//! cached for fast lookup during recursive inference. A fresh context is built
//! at the start of each `typecheck_program` run — no global or thread-local state.

use std::collections::HashMap;

use flux_hir::{HirItem, HirProgram, SymbolId, SymbolKind};

use crate::types::Type;

use super::resolve::{enum_payload_to_type, resolve_hir_type_simple};

/// The types visible to some code: the program's symbols, shared, plus its
/// own locals. Each block gets a copy of its scope, so that copy must not
/// include the whole program: checking was quadratic in the number of items.
#[derive(Debug)]
pub(crate) struct Scope<'a> {
    module: &'a HashMap<SymbolId, Type>,
    locals: HashMap<SymbolId, Type>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static SCOPE_LOCAL_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for Scope<'_> {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SCOPE_LOCAL_COPIES.with(|n| n.set(n.get() + self.locals.len()));
        Self {
            module: self.module,
            locals: self.locals.clone(),
        }
    }
}

impl<'a> Scope<'a> {
    pub(crate) fn new(module: &'a HashMap<SymbolId, Type>) -> Self {
        Self {
            module,
            locals: HashMap::new(),
        }
    }

    pub(crate) fn get(&self, id: &SymbolId) -> Option<&Type> {
        self.locals.get(id).or_else(|| self.module.get(id))
    }

    pub(crate) fn insert(&mut self, id: SymbolId, ty: Type) {
        self.locals.insert(id, ty);
    }

    pub(crate) fn module(&self) -> &'a HashMap<SymbolId, Type> {
        self.module
    }
}

/// Lookup tables populated from a single HIR program for one type-check pass.
#[derive(Debug, Default, Clone)]
pub struct TypeCheckContext {
    enum_payloads: HashMap<(String, String), Type>,
    user_types: HashMap<String, Type>,
    type_aliases: HashMap<String, flux_hir::HirType>,
    trait_impls: HashMap<(String, String), SymbolId>,
    builtin_symbols: std::collections::HashSet<SymbolId>,
    effectful_functions: std::collections::HashSet<SymbolId>,
    /// Closure bodies are checked where created, with contextual parameter
    /// types and the creation scope's capture types.
    capturing_closures: HashMap<SymbolId, flux_hir::HirFn>,
    pub(crate) defaulted_closure_params: std::cell::RefCell<std::collections::HashSet<SymbolId>>,
    pub(crate) closure_params: std::cell::RefCell<HashMap<SymbolId, Type>>,
    pub(crate) closure_types: std::cell::RefCell<HashMap<SymbolId, Type>>,
    pub(crate) preparing_closures: bool,
    pub(crate) generic_parameters: std::cell::RefCell<std::collections::HashSet<String>>,
    #[cfg(test)]
    pub(crate) closure_body_checks: std::cell::Cell<usize>,
    #[cfg(test)]
    pub(crate) closure_check_limit: std::cell::Cell<Option<usize>>,
    pub(crate) current_function: std::cell::Cell<Option<SymbolId>>,
    pub(crate) const_bindings: std::collections::HashSet<String>,
    /// `recv.method(…)` call spans resolved to their impl function.
    pub(crate) method_calls:
        std::cell::RefCell<HashMap<(Option<SymbolId>, flux_diagnostics::Span), SymbolId>>,
    /// Declared return type of the function being checked, if it has one;
    /// checks a `return` wherever it appears, including inside match arms.
    pub(crate) fn_return: std::cell::RefCell<Option<Type>>,
}

impl TypeCheckContext {
    pub fn from_program(program: &HirProgram) -> Self {
        let mut ctx = Self {
            type_aliases: program.type_aliases.clone(),
            const_bindings: program.const_bindings.clone(),
            ..Self::default()
        };

        for item in &program.items {
            if let HirItem::Fn(f) = item {
                ctx.defaulted_closure_params.get_mut().extend(
                    f.params
                        .iter()
                        .filter(|p| p.defaulted_to_int)
                        .map(|p| p.name),
                );
            }
            if let HirItem::Enum(e) = item {
                let enum_name = program.names.get(&e.name).cloned().unwrap_or_default();
                for (variant, payload) in &e.variants {
                    if let Some(p) = payload {
                        let ty = enum_payload_to_type(p);
                        ctx.enum_payloads
                            .insert((enum_name.clone(), variant.clone()), ty);
                    }
                }
            }
        }

        for item in &program.items {
            match item {
                HirItem::Struct(s) => {
                    let name = program.names.get(&s.name).cloned().unwrap_or_default();
                    let fields = s
                        .fields
                        .iter()
                        .map(|(n, t)| (n.clone(), resolve_hir_type_simple(t)))
                        .collect();
                    ctx.user_types
                        .insert(name.clone(), Type::Struct { name, fields });
                }
                HirItem::Enum(e) => {
                    let name = program.names.get(&e.name).cloned().unwrap_or_default();
                    let variants = e
                        .variants
                        .iter()
                        .map(|(n, p)| (n.clone(), p.as_ref().map(enum_payload_to_type)))
                        .collect();
                    ctx.user_types
                        .insert(name.clone(), Type::Enum { name, variants });
                }
                _ => {}
            }
        }

        // Field and payload types were resolved without the user types, so a
        // named type is a placeholder `Enum { name, variants: [] }`; replace
        // those with the registered struct/enum (bounded for recursive types,
        // which `expand` continues on demand).
        let mut registered = ctx.user_types.clone();
        for (name, aliased) in &ctx.type_aliases {
            registered
                .entry(name.clone())
                .or_insert_with(|| resolve_hir_type_simple(aliased));
        }
        let aliases = registered.clone();
        for ty in ctx.user_types.values_mut() {
            *ty = fill_user_types(ty, &aliases, 4);
        }
        let mut registered = ctx.user_types.clone();
        for (name, aliased) in &ctx.type_aliases {
            registered.entry(name.clone()).or_insert_with(|| {
                aliases
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| resolve_hir_type_simple(aliased))
            });
        }
        for ty in ctx.enum_payloads.values_mut() {
            *ty = fill_user_types(ty, &registered, 4);
        }

        ctx.trait_impls = program.trait_impls.clone();
        ctx.builtin_symbols = program
            .symbols
            .iter()
            .filter_map(|(id, symbol)| {
                (symbol.kind == SymbolKind::Builtin && flux_std::is_builtin_fn(&symbol.name))
                    .then_some(*id)
            })
            .collect();
        ctx.capturing_closures = program
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Fn(f) if f.is_closure => Some((f.name, f.clone())),
                _ => None,
            })
            .collect();
        ctx.effectful_functions =
            super::effects::effectful_functions(program, &ctx.builtin_symbols);
        ctx
    }

    pub fn enum_payload(&self, enum_name: &str, variant: &str) -> Option<&Type> {
        self.enum_payloads
            .get(&(enum_name.to_string(), variant.to_string()))
    }

    pub fn user_type(&self, name: &str) -> Option<&Type> {
        self.user_types.get(name)
    }

    pub fn type_alias(&self, name: &str) -> Option<&flux_hir::HirType> {
        self.type_aliases.get(name)
    }

    pub fn trait_impl(&self, type_tag: &str, method: &str) -> Option<SymbolId> {
        self.trait_impls
            .get(&(type_tag.to_string(), method.to_string()))
            .copied()
    }

    /// `ty` with a placeholder at its top replaced by the registered type.
    /// User types expand a few levels deep, so a recursive type ends in a
    /// placeholder; anything that looks inside a type expands it first.
    pub(crate) fn expand(&self, ty: Type) -> Type {
        match &ty {
            Type::Enum { name, variants } if variants.is_empty() => {
                self.user_type(name).cloned().unwrap_or(ty)
            }
            _ => ty,
        }
    }

    pub fn is_builtin_symbol(&self, id: SymbolId) -> bool {
        self.builtin_symbols.contains(&id)
    }

    pub(crate) fn capturing_closure(&self, id: SymbolId) -> Option<&flux_hir::HirFn> {
        self.capturing_closures.get(&id)
    }

    pub fn is_effectful_function(&self, id: SymbolId) -> bool {
        self.effectful_functions.contains(&id)
    }
}

/// `ty` with placeholder user-type names replaced by their registered types,
/// `depth` levels deep (placeholders below that stay as they are).
fn fill_user_types(ty: &Type, registered: &HashMap<String, Type>, depth: usize) -> Type {
    if depth == 0 {
        return ty.clone();
    }
    let next = |t: &Type| fill_user_types(t, registered, depth - 1);
    match ty {
        Type::Enum { name, variants } if variants.is_empty() => match registered.get(name) {
            Some(real) => next(real),
            None => ty.clone(),
        },
        Type::Struct { name, fields } => Type::Struct {
            name: name.clone(),
            fields: fields.iter().map(|(n, t)| (n.clone(), next(t))).collect(),
        },
        Type::Enum { name, variants } => Type::Enum {
            name: name.clone(),
            variants: variants
                .iter()
                .map(|(n, p)| (n.clone(), p.as_ref().map(next)))
                .collect(),
        },
        Type::Array(t) => Type::Array(Box::new(next(t))),
        Type::Option(t) => Type::Option(Box::new(next(t))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(next(ok)),
            err: Box::new(next(err)),
        },
        Type::Func { params, ret } => Type::Func {
            params: params.iter().map(next).collect(),
            ret: Box::new(next(ret)),
        },
        other => other.clone(),
    }
}

/// An unchecked generic parameter is unknown, not an Int assumption. Keeping
/// known outer shapes lets a template still check its independent expressions.
pub(crate) fn erase_type_variables(ty: Type) -> Type {
    match ty {
        Type::Var(_) => Type::Error,
        Type::Array(t) => Type::Array(Box::new(erase_type_variables(*t))),
        Type::Option(t) => Type::Option(Box::new(erase_type_variables(*t))),
        Type::Cell(t) => Type::Cell(Box::new(erase_type_variables(*t))),
        Type::Read(t) => Type::Read(Box::new(erase_type_variables(*t))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(erase_type_variables(*ok)),
            err: Box::new(erase_type_variables(*err)),
        },
        Type::Map { key, value } => Type::Map {
            key: Box::new(erase_type_variables(*key)),
            value: Box::new(erase_type_variables(*value)),
        },
        Type::Func { params, ret } => Type::Func {
            params: params.into_iter().map(erase_type_variables).collect(),
            ret: Box::new(erase_type_variables(*ret)),
        },
        other => other,
    }
}
