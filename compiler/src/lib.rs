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
    let lowered = semantic::resolve_namespaces(&program);
    Ok(bytecode::emit_text(&lowered))
}

pub fn compile_to_binary_bytecode(source: &str) -> Result<Vec<u8>, String> {
    let program = parser::parse_program(source)?;
    semantic::type_check(&program)?;
    let lowered = semantic::resolve_namespaces(&program);
    Ok(bytecode::emit_binary(&lowered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_signal_and_reactive() {
        let src = "import std\nsignal a = 10\nreactive b = a * 2\nstd::print(b)\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("signal a = 10"));
        assert!(out.contains("reactive b = a 2 *"));
        assert!(out.contains("import std"));
        assert!(out.contains("expr b @call:std::print:1"));
    }


    #[test]
    fn parses_if_and_fn_and_lambda() {
        let src = "fn add1(x){ x + 1 }\nsignal a = if true { 1 } else { 2 }\nprint(add1(a))\n";
        let out = compile_to_text_bytecode(src).expect("should compile text");
        assert!(out.contains("signal a"));
    }


    #[test]
    fn parses_import_alias_namespace_call() {
        let src = "import std as s\nsignal a = 9\nexpr = 1\ns::print(s::sqrt(a))\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("import std as s"));
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
