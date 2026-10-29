/// Where code is being compiled — determines reactive semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileContext {
    /// Module top-level: `=` creates graph nodes.
    Module,
    /// Ordinary function body: locals only, no reactive reads.
    PureFn,
    /// watch / on / rfn body: dynamic dependency tracking.
    Tracking,
}

/// Lexical scope classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Module,
    Function { reactive: bool },
    Tracking,
    Block,
}

impl ScopeKind {
    pub fn context(self) -> CompileContext {
        match self {
            ScopeKind::Module => CompileContext::Module,
            ScopeKind::Function { reactive: true } => CompileContext::Tracking,
            ScopeKind::Function { reactive: false } => CompileContext::PureFn,
            ScopeKind::Tracking => CompileContext::Tracking,
            ScopeKind::Block => CompileContext::PureFn,
        }
    }
}
