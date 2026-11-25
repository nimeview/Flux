mod context;
mod cycles;
mod lower;
mod nodes;
mod references;
mod results;
mod symbol;
mod unused;

#[cfg(test)]
mod tests;

pub use context::{CompileContext, ScopeKind};
pub use lower::{lower_linked_program, lower_program};
pub use nodes::*;
pub use references::{
    build_reference_index, build_reference_index_with_methods, rename_spans, symbol_at_offset,
    word_at_offset, ReferenceIndex,
};
pub use symbol::{BindingKind, Symbol, SymbolId, SymbolKind, SymbolTable};
pub use unused::check_unused_bindings;

/// A name or path as the program wrote it: a type or function of an imported
/// module is kept as `__mod_<index>__<Name>`, shown as `Name`.
/// The prefix of a library trait's internal name (`Display` that a module
/// neither declares nor imports), so that it never meets a program's own.
pub const LIBRARY_TRAIT: &str = "\0lib:";

pub fn display_path(name: &str) -> String {
    let name = name.strip_prefix(LIBRARY_TRAIT).unwrap_or(name);
    name.split("::")
        .map(|part| {
            part.strip_prefix("__mod_")
                .and_then(|rest| rest.split_once("__"))
                .filter(|(index, _)| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
                .map_or(part, |(_, declared)| declared)
        })
        .collect::<Vec<_>>()
        .join("::")
}

/// `text` with every internal module prefix (`__mod_1__`) removed, so that a
/// message names types and functions as the program wrote them.
pub fn display_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("__mod_") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "__mod_".len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with("__") {
            rest = &after[digits + 2..];
        } else {
            out.push_str("__mod_");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}
