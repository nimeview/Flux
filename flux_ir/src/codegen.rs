use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::HirProgram;

use crate::{lower_to_ir, CompiledProgram};

/// Lowers typed HIR to an executable IR program.
pub trait Codegen {
    fn emit(
        &self,
        program: &HirProgram,
        source: SourceId,
        diagnostics: &mut DiagnosticBag,
    ) -> Option<CompiledProgram>;
}

/// Default IR codegen backend (stack-machine bytecode + reactive graph metadata).
#[derive(Debug, Default, Clone, Copy)]
pub struct IrCodegen;

impl Codegen for IrCodegen {
    fn emit(
        &self,
        program: &HirProgram,
        source: SourceId,
        diagnostics: &mut DiagnosticBag,
    ) -> Option<CompiledProgram> {
        lower_to_ir(program, source, diagnostics)
    }
}
