mod bytecode;
mod diagnostics;
mod ffi;
mod hir;
mod lexer;
mod parser;
mod semantic;

pub fn compile_to_text_bytecode(source: &str) -> Result<String, String> {
    let program = parser::parse_program(source)?;
    semantic::type_check(&program)?;
    Ok(bytecode::emit_text(&program))
}

pub fn compile_to_binary_bytecode(source: &str) -> Result<Vec<u8>, String> {
    let program = parser::parse_program(source)?;
    semantic::type_check(&program)?;
    Ok(bytecode::emit_binary(&program))
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


#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn type_error_on_bad_set() {
        let src = "signal a = 1\nset a = true\n";
        let err = compile_to_binary_bytecode(src).expect_err("should fail type check");
        assert!(err.contains("type mismatch"));
    }
}
