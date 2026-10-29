//! WASM codegen backend stub (v1.0).

use flux_ir::CompiledProgram;

/// WASM backend status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmBackendStatus {
    /// Backend is planned; emits no WASM yet.
    Stub,
}

/// Compile a Flux IR program to WASM bytes (stub — returns empty module).
pub fn compile_to_wasm(program: &CompiledProgram) -> (WasmBackendStatus, Vec<u8>) {
    let _ = program;
    (WasmBackendStatus::Stub, wasm_minimal_module())
}

fn wasm_minimal_module() -> Vec<u8> {
    // Minimal valid WASM module: (module)
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_emits_valid_header() {
        let (status, bytes) = compile_to_wasm(&flux_ir::CompiledProgram {
            nodes: vec![],
            init_stmts: vec![],
            functions: vec![],
            watches: vec![],
            on_handlers: vec![],
            events: vec![],
        });
        assert_eq!(status, WasmBackendStatus::Stub);
        assert_eq!(&bytes[0..4], b"\0asm");
    }
}
