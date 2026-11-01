use flux_diagnostics::Span;
use std::collections::HashMap;

use crate::ScopeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Source,
    Derived,
    Event,
    Local { mutable: bool },
    Param,
    Function { reactive: bool },
    Builtin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Source,
    Derived,
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    next_id: u32,
    scopes: Vec<ScopeFrame>,
    /// Every symbol id → name, including params/locals from popped scopes.
    all_names: HashMap<SymbolId, String>,
    /// Full symbol metadata for every declared id (survives scope pop).
    all_symbols: HashMap<SymbolId, Symbol>,
    /// Enum variant name → enum type name.
    pub enum_variants: HashMap<String, String>,
    pub closure_counter: u32,
    pub pending_fns: Vec<crate::nodes::HirItem>,
    pub pending_bindings: Vec<crate::nodes::HirItem>,
    /// Every `impl` seen, checked against its trait once all traits are lowered.
    pub pending_impls: Vec<flux_syntax::ImplDecl>,
    /// Module-level `const` binding names (immutable, no reassignment).
    pub const_bindings: std::collections::HashSet<String>,
    /// Best-effort element type of array-valued module bindings (by name), used to
    /// infer untyped closure parameter types at HOF call sites (e.g. `map(nums, |x| ...)`).
    pub array_elem_hints: HashMap<String, flux_syntax::TypeName>,
    /// `const` bindings: fixed at compile time, so reading one is not a
    /// reactive read (pure `fn`s may use them).
    pub constants: std::collections::HashSet<SymbolId>,
    /// Module `let` bindings declared without `mut` (still reactive sources;
    /// used only to suggest `const` in E002).
    pub immutable_lets: std::collections::HashSet<SymbolId>,
    /// Trait impl methods keyed by `(receiver type tag, method name)`, populated as
    /// `impl Trait for Type` blocks are lowered. Used for exact `x.method(...)` dispatch.
    pub trait_impl_index: HashMap<(String, String), SymbolId>,
    /// Module alias → true (e.g. `m` in `import math as m`).
    pub module_aliases: HashMap<String, ()>,
    /// Qualified import lookup: `(alias, export_name)` → symbol.
    pub qualified_symbols: HashMap<(String, String), SymbolId>,
}

#[derive(Debug)]
struct ScopeFrame {
    kind: ScopeKind,
    symbols: HashMap<String, Symbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            scopes: vec![ScopeFrame {
                kind: ScopeKind::Module,
                symbols: HashMap::new(),
            }],
            all_names: HashMap::new(),
            all_symbols: HashMap::new(),
            enum_variants: HashMap::new(),
            closure_counter: 0,
            pending_fns: Vec::new(),
            pending_bindings: Vec::new(),
            pending_impls: Vec::new(),
            const_bindings: std::collections::HashSet::new(),
            array_elem_hints: HashMap::new(),
            constants: std::collections::HashSet::new(),
            immutable_lets: std::collections::HashSet::new(),
            trait_impl_index: HashMap::new(),
            module_aliases: HashMap::new(),
            qualified_symbols: HashMap::new(),
        }
    }

    pub fn fresh_closure_name(&mut self) -> String {
        let name = format!("__closure_{}", self.closure_counter);
        self.closure_counter += 1;
        name
    }

    /// Whether lowering is inside a function body (pure or reactive).
    pub fn in_function(&self) -> bool {
        self.scopes
            .iter()
            .any(|s| matches!(s.kind, ScopeKind::Function { .. }))
    }

    pub fn current_context(&self) -> crate::CompileContext {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| match s.kind {
                ScopeKind::Block => None,
                other => Some(other.context()),
            })
            .unwrap_or(crate::CompileContext::Module)
    }

    pub fn push(&mut self, kind: ScopeKind) {
        self.scopes.push(ScopeFrame {
            kind,
            symbols: HashMap::new(),
        });
    }

    pub fn pop(&mut self) {
        self.scopes.pop();
    }

    pub fn declare(
        &mut self,
        name: &str,
        kind: SymbolKind,
        span: Span,
    ) -> Result<SymbolId, Symbol> {
        let id = SymbolId(self.next_id);
        self.next_id += 1;
        let symbol = Symbol {
            id,
            name: name.to_string(),
            kind,
            span,
        };
        if let Some(scope) = self.scopes.last_mut() {
            if let Some(prev) = scope.symbols.insert(name.to_string(), symbol.clone()) {
                return Err(prev);
            }
        }
        self.all_names.insert(id, name.to_string());
        self.all_symbols.insert(id, symbol.clone());
        Ok(id)
    }

    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.symbols.get(name) {
                return Some(sym);
            }
        }
        None
    }

    pub fn lookup_by_id(&self, id: SymbolId) -> Option<&Symbol> {
        self.get(id)
    }

    pub fn export_names(&self) -> HashMap<SymbolId, String> {
        self.all_names.clone()
    }

    pub fn export_symbols(&self) -> HashMap<SymbolId, Symbol> {
        self.all_symbols.clone()
    }

    pub fn get(&self, id: SymbolId) -> Option<&Symbol> {
        self.all_symbols.get(&id)
    }

    /// Makes `bare_name` refer to `symbol` in the innermost scope (imports and
    /// a module's own items). The alias carries the target's kind, so reading
    /// an imported source or calling an imported `rfn` stays reactive.
    pub fn reexport_alias(&mut self, bare_name: &str, symbol: SymbolId, span: Span) {
        let kind = self
            .all_symbols
            .get(&symbol)
            .map_or(SymbolKind::Builtin, |s| s.kind);
        if let Some(scope) = self.scopes.last_mut() {
            scope.symbols.insert(
                bare_name.to_string(),
                Symbol {
                    id: symbol,
                    name: bare_name.to_string(),
                    kind,
                    span,
                },
            );
        }
    }

    /// Updates the kind of the symbol `name` resolves to, including every
    /// alias of it (so a binding classified later stays consistent).
    pub fn update_kind(&mut self, name: &str, kind: SymbolKind) {
        let Some(id) = self.lookup(name).map(|s| s.id) else {
            return;
        };
        for scope in &mut self.scopes {
            for sym in scope.symbols.values_mut() {
                if sym.id == id {
                    sym.kind = kind;
                }
            }
        }
        if let Some(sym) = self.all_symbols.get_mut(&id) {
            sym.kind = kind;
        }
    }
}
