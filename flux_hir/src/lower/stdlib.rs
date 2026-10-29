//! Register standard-library intrinsics from `import io` / `import math` etc.

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_std::{export_in_std_module, std_module_exports, std_module_name};
use flux_syntax::ImportDecl;

use crate::symbol::{SymbolKind, SymbolTable};

use super::push_duplicate;

/// Register host intrinsics for a standard-library import declaration.
pub(super) fn register_stdlib_import(
    import: &ImportDecl,
    symbols: &mut SymbolTable,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let Some(module) = std_module_name(&import.path) else {
        return false;
    };
    let Some(exports) = std_module_exports(module) else {
        return false;
    };

    let alias = import.alias.clone().unwrap_or_else(|| module.to_string());
    symbols.module_aliases.insert(alias.clone(), ());

    let selected: Vec<&str> = if import.symbols.is_empty() {
        exports.to_vec()
    } else {
        import.symbols.iter().map(|s| s.as_str()).collect()
    };

    let import_all_bare = import.symbols.is_empty() && import.alias.is_none();

    for name in selected {
        if !export_in_std_module(module, name) {
            diagnostics.push(FluxError::UnknownExport {
                name: name.to_string(),
                module: import.path.clone(),
                span: span_to_miette(import.span),
                source_id: source,
            });
            continue;
        }

        match symbols.declare(name, SymbolKind::Builtin, import.span) {
            Ok(sym_id) => {
                symbols
                    .qualified_symbols
                    .insert((alias.clone(), name.to_string()), sym_id);
                if import_all_bare || import.symbols.contains(&name.to_string()) {
                    symbols.reexport_alias(name, sym_id, import.span);
                }
            }
            Err(prev) => {
                push_duplicate(
                    diagnostics,
                    source,
                    &flux_syntax::Item::Import(import.clone()),
                    prev,
                );
            }
        }
    }

    true
}
