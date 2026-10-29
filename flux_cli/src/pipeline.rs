//! Backward-compatible re-exports — the canonical pipeline lives in `flux_driver`.

pub use flux_driver::{
    check_file, compile, compile_with_mode, compile_with_options, source_from, Checked,
    CompileFailed, CompileOptions, CompilePhase, Compiled,
};

use std::path::Path;

use flux_runtime::ExecutionMode;

/// Compile a `.fl` file from disk (legacy signature: path + execution mode).
pub fn compile_file(path: &Path, mode: ExecutionMode) -> Result<Compiled, CompileFailed> {
    flux_driver::compile_file(path, CompileOptions::run(mode))
}

/// Compile with full [`CompileOptions`] (cache, phase, mode).
pub fn compile_file_with_options(
    path: &Path,
    options: CompileOptions,
) -> Result<Compiled, CompileFailed> {
    flux_driver::compile_file(path, options)
}
