pub mod ast;
pub mod parser;

pub use ast::*;
pub use parser::parse_program;

/// Standard Flux source file extension (without the leading dot).
pub const MODULE_EXTENSION: &str = "fl";

/// Returns `true` if `path` already ends with the module extension.
pub fn has_module_extension(path: &str) -> bool {
    path.ends_with(&format!(".{MODULE_EXTENSION}"))
}

/// Appends `.{MODULE_EXTENSION}` when missing.
pub fn with_module_extension(path: &str) -> String {
    if has_module_extension(path) {
        path.to_string()
    } else {
        format!("{path}.{MODULE_EXTENSION}")
    }
}
