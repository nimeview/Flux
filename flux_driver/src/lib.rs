//! Shared compiler driver — single entry point for CLI, LSP and tests.

mod cache;
mod options;
mod paths;
mod pipeline;

pub use cache::{
    cache_key_for_path, cache_matches, deserialize_program, get_analysis_cache, serialize_program,
    CompileCache,
};
pub use options::{CompileOptions, CompilePhase};
pub use paths::{
    build_dir, cache_dir, ensure_project_dirs, project_dir, BUILD_DIR, CACHE_DIR, PROJECT_DIR,
};
pub use pipeline::{
    analyze_document, check_file, compile, compile_file, compile_with_mode, compile_with_options,
    source_from, Analyzed, Checked, CompileFailed, Compiled,
};
