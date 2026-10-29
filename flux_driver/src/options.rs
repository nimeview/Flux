//! Compile pipeline configuration.

use flux_runtime::ExecutionMode;

/// Which phases of the pipeline to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilePhase {
    /// Parse → HIR → types → IR → executor (default for `flux run`).
    #[default]
    Full,
    /// Through type-check (+ IR emit for side-effect diagnostics). Used by LSP.
    Analysis,
    /// Type-check only — no IR or executor.
    Check,
}

/// Options controlling a single compilation.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub mode: ExecutionMode,
    pub phase: CompilePhase,
    /// When true, skip IR lowering if on-disk cache matches source hash.
    pub use_cache: bool,
    /// When true, reuse in-memory HIR + types for unchanged source keys.
    pub analysis_cache: bool,
}

impl CompileOptions {
    pub fn run(mode: ExecutionMode) -> Self {
        Self {
            mode,
            phase: CompilePhase::Full,
            use_cache: true,
            analysis_cache: false,
        }
    }

    pub fn without_cache(mut self) -> Self {
        self.use_cache = false;
        self
    }

    pub fn check() -> Self {
        Self {
            mode: ExecutionMode::Vm,
            phase: CompilePhase::Check,
            use_cache: false,
            analysis_cache: true,
        }
    }

    /// LSP / IDE: type-check only, with analysis cache.
    pub fn ide() -> Self {
        Self::check()
    }

    pub fn analysis() -> Self {
        Self {
            mode: ExecutionMode::Vm,
            phase: CompilePhase::Analysis,
            use_cache: false,
            analysis_cache: false,
        }
    }
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self::run(ExecutionMode::Vm)
    }
}
