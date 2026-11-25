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
        LOCAL_TYPES.with(|types| types.borrow_mut().insert(id, ty.clone()));
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
    pub(crate) trait_impls: HashMap<(String, String), SymbolId>,
    /// `mut self` methods, and the call sites written as statements on a
    /// place (see `flux_hir::HirProgram::mut_call_sites`).
    pub(crate) mut_methods: std::collections::HashSet<SymbolId>,
    pub(crate) mut_call_sites: std::collections::HashSet<flux_diagnostics::Span>,
    pub(crate) captured_call_sites: std::collections::HashSet<flux_diagnostics::Span>,
    pub(crate) deferred_writes: HashMap<flux_diagnostics::Span, flux_diagnostics::FluxError>,
    /// Where each symbol is declared, for diagnostics about pattern bindings.
    pub(crate) symbol_spans: HashMap<SymbolId, flux_diagnostics::Span>,
    pub(crate) extension_sources: HashMap<SymbolId, flux_diagnostics::SourceId>,
    pub(crate) builtin_ids: HashMap<String, SymbolId>,
    pub(crate) file_imports:
        HashMap<flux_diagnostics::SourceId, std::collections::HashSet<flux_diagnostics::SourceId>>,
    builtin_symbols: std::collections::HashSet<SymbolId>,
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
    /// `place.m(…)` statements whose `m` is a library method that changes
    /// its receiver (`xs.push(1)`, `xs.sort()`): they store the result back.
    pub(crate) changing_sites:
        std::cell::RefCell<std::collections::HashSet<flux_diagnostics::Span>>,
    /// `op.f(x)` calls of the function in field `f`, by enclosing function and span.
    pub(crate) field_calls:
        std::cell::RefCell<std::collections::HashSet<(Option<SymbolId>, flux_diagnostics::Span)>>,
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
                    let args = s.type_params.iter().cloned().map(Type::Var).collect();
                    ctx.user_types
                        .insert(name.clone(), Type::Struct { name, args, fields });
                }
                HirItem::Enum(e) => {
                    let name = program.names.get(&e.name).cloned().unwrap_or_default();
                    let variants = e
                        .variants
                        .iter()
                        .map(|(n, p)| (n.clone(), p.as_ref().map(enum_payload_to_type)))
                        .collect();
                    let args = e.type_params.iter().cloned().map(Type::Var).collect();
                    ctx.user_types.insert(
                        name.clone(),
                        Type::Enum {
                            name,
                            args,
                            variants,
                        },
                    );
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
        ctx.mut_methods = program.mut_methods.clone();
        ctx.mut_call_sites = program.mut_call_sites.clone();
        ctx.captured_call_sites = program.captured_call_sites.clone();
        ctx.deferred_writes = program.deferred_writes.clone();
        ctx.symbol_spans = program
            .symbols
            .iter()
            .map(|(id, s)| (*id, s.span))
            .collect();
        ctx.extension_sources = program.extension_sources.clone();
        ctx.builtin_ids = program.builtin_ids.clone();
        ctx.file_imports = program.file_imports.clone();
        ctx.builtin_symbols = program
            .symbols
            .iter()
            .filter_map(|(id, symbol)| {
                (symbol.kind == SymbolKind::Builtin
                    && (flux_std::is_builtin_fn(&symbol.name)
                        || symbol.name == flux_hir::WITH_FIELDS
                        || symbol.name == flux_hir::TUPLE
                        || symbol.name == flux_hir::TUPLE_CHECK
                        || symbol.name == flux_hir::GROUPED))
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
        ctx
    }

    pub fn enum_payload(&self, enum_name: &str, variant: &str) -> Option<&Type> {
        self.enum_payloads
            .get(&(enum_name.to_string(), variant.to_string()))
    }

    pub fn user_type(&self, name: &str) -> Option<&Type> {
        self.user_types.get(name)
    }

    /// A user type with its fields or variants, for its type arguments
    /// (`Pair[Int]`'s `a` is `Int`); other types as they are.
    pub fn instance_of(&self, ty: &Type) -> Type {
        match ty {
            Type::Struct { name, args, .. } | Type::Enum { name, args, .. } if !name.is_empty() => {
                self.user_type(name)
                    .map(|template| crate::generic::instantiate(template, args))
                    .unwrap_or_else(|| ty.clone())
            }
            _ => ty.clone(),
        }
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
            Type::Enum {
                name,
                args,
                variants,
            } if variants.is_empty() => match self.user_type(name) {
                Some(template) => crate::generic::instantiate(template, args),
                None => ty,
            },
            _ => ty,
        }
    }

    pub fn is_builtin_symbol(&self, id: SymbolId) -> bool {
        self.builtin_symbols.contains(&id)
    }

    pub(crate) fn capturing_closure(&self, id: SymbolId) -> Option<&flux_hir::HirFn> {
        self.capturing_closures.get(&id)
    }

    /// The symbols of library functions, for the purity check.
    pub(crate) fn builtin_symbols(&self) -> &std::collections::HashSet<SymbolId> {
        &self.builtin_symbols
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
        Type::Enum {
            name,
            args,
            variants,
        } if variants.is_empty() => match registered.get(name) {
            Some(real) => next(&crate::generic::instantiate(real, args)),
            None => ty.clone(),
        },
        Type::Struct { name, args, fields } => Type::Struct {
            name: name.clone(),
            args: args.clone(),
            fields: fields.iter().map(|(n, t)| (n.clone(), next(t))).collect(),
        },
        Type::Enum {
            name,
            args,
            variants,
        } => Type::Enum {
            name: name.clone(),
            args: args.clone(),
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
        Type::Tuple(elems) => Type::Tuple(elems.into_iter().map(erase_type_variables).collect()),
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

thread_local! {
    /// Every local and parameter's type, kept for tools after checking ends. When a
    /// closure is checked again with a better parameter type, the last one stays.
    static LOCAL_TYPES: std::cell::RefCell<HashMap<SymbolId, Type>> =
        std::cell::RefCell::new(HashMap::new());
}

/// The local types recorded since the last call.
pub(crate) fn take_local_types() -> HashMap<SymbolId, Type> {
    LOCAL_TYPES.with(|types| std::mem::take(&mut *types.borrow_mut()))
}
