mod context;
mod lower;
mod nodes;
mod references;
mod symbol;
mod unused;

#[cfg(test)]
mod tests;

pub use context::{CompileContext, ScopeKind};
pub use lower::{lower_linked_program, lower_program};
pub use nodes::*;
pub use references::{
    build_reference_index, rename_spans, symbol_at_offset, word_at_offset, ReferenceIndex,
};
pub use symbol::{BindingKind, Symbol, SymbolId, SymbolKind, SymbolTable};
pub use unused::check_unused_bindings;
