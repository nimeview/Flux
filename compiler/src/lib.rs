mod bytecode;
mod diagnostics;
mod ffi;
mod hir;
mod lexer;
mod parser;
mod semantic;

pub fn compile_to_text_bytecode(source: &str) -> Result<String, String> {
    let program = parser::parse_program(source)?;
    Ok(bytecode::emit_text(&program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_signal_and_reactive() {
        let src = "signal a = 10\nreactive b = a * 2\nprint(b)\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("signal a = 10"));
        assert!(out.contains("reactive b = a 2 *"));
        assert!(out.contains("expr b @call:print:1"));
    }
}
