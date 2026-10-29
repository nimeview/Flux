//! Per-compilation type registries.
//!
//! [`TypeCheckContext`] mirrors data already present on [`HirProgram`] but is
//! cached for fast lookup during recursive inference. A fresh context is built
//! at the start of each `typecheck_program` run — no global or thread-local state.

use std::collections::HashMap;

use flux_hir::{HirItem, HirProgram, SymbolId};

use crate::types::Type;

use super::resolve::{enum_payload_to_type, resolve_hir_type_simple};

/// Lookup tables populated from a single HIR program for one type-check pass.
#[derive(Debug, Default, Clone)]
pub struct TypeCheckContext {
    enum_payloads: HashMap<(String, String), Type>,
    user_types: HashMap<String, Type>,
    type_aliases: HashMap<String, flux_hir::HirType>,
    trait_impls: HashMap<(String, String), SymbolId>,
    pub(crate) const_bindings: std::collections::HashSet<String>,
}

impl TypeCheckContext {
    pub fn from_program(program: &HirProgram) -> Self {
        let mut ctx = Self {
            type_aliases: program.type_aliases.clone(),
            const_bindings: program.const_bindings.clone(),
            ..Self::default()
        };

        for item in &program.items {
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

        ctx.trait_impls = program.trait_impls.clone();
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
}
