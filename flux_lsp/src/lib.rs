//! Flux Language Server.
//!
//! Two entry points are exposed:
//! - [`analyze_source`]: one-shot JSON diagnostics for a single file (`flux lsp <file>`,
//!   kept for scripting/CI use — prefer the stdio server for editor integration).
//! - [`server::run`]: a full stdio LSP server (diagnostics + hover + completion),
//!   wired up by `flux lsp-server`.

mod analysis;
pub mod documents;
pub mod server;

use serde::{Deserialize, Serialize};

pub use analysis::{analyze_document, describe_type, hover_for_name, word_at_offset, DocAnalysis};

#[derive(Debug, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub message: String,
    pub code: String,
}

/// Analyze Flux source and return LSP-style diagnostics (single-shot, JSON-dump mode).
pub fn analyze_source(path: &str, contents: &str) -> Vec<LspDiagnostic> {
    let path = std::path::Path::new(path);
    let path = if path.exists() { Some(path) } else { None };
    analysis::analyze_document(path, contents)
        .diagnostics
        .into_iter()
        .map(|d| LspDiagnostic {
            line: d.start_line,
            column: d.start_column,
            end_line: d.end_line,
            end_column: d.end_column,
            message: d.message,
            code: d.code,
        })
        .collect()
}
