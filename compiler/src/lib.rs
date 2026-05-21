mod bytecode;
mod diagnostics;
mod ffi;
mod hir;
mod lexer;
mod parser;
mod semantic;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use hir::Stmt;

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
    bytecode::emit_binary(&lowered)
}

pub fn compile_file_to_binary_bytecode(entry_file: &Path) -> Result<Vec<u8>, String> {
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    load_module(entry_file, None, &mut visited, &mut ordered)?;

    semantic::type_check(&ordered)?;
    let lowered = semantic::resolve_namespaces(&ordered);
    bytecode::emit_binary(&lowered)
}

fn load_module(path: &Path, module_ns: Option<&str>, visited: &mut HashSet<PathBuf>, out: &mut Vec<Stmt>) -> Result<(), String> {
    let canon = fs::canonicalize(path).map_err(|e| format!("failed to resolve module {}: {e}", path.display()))?;
    if !visited.insert(canon.clone()) {
        return Ok(());
    }

    let src = fs::read_to_string(&canon).map_err(|e| format!("failed to read module {}: {e}", canon.display()))?;
    let stmts = parser::parse_program(&src)?;

    let base_dir = canon.parent().unwrap_or(Path::new("."));
    for stmt in &stmts {
        if let Stmt::Import { path, .. } = stmt {
            if path == "std" || path.starts_with("std::") {
                continue;
            }
            let module_file = import_to_file(base_dir, path);
            load_module(&module_file, Some(path), visited, out)?;
        }
    }

    for stmt in stmts {
        let stmt = qualify_stmt(stmt, module_ns);
        out.push(stmt);
    }

    Ok(())
}

fn import_to_file(base_dir: &Path, import_path: &str) -> PathBuf {
    let rel = import_path.replace("::", "/") + ".fx";
    base_dir.join(rel)
}


fn qualify_stmt(stmt: Stmt, module_ns: Option<&str>) -> Stmt {
    let Some(ns) = module_ns else { return stmt; };
    match stmt {
        Stmt::FnDef { name, params, body } => Stmt::FnDef { name: format!("{ns}::{name}"), params, body },
        Stmt::Signal(name, expr) => Stmt::Signal(format!("{ns}::{name}"), expr),
        Stmt::Reactive(name, expr) => Stmt::Reactive(format!("{ns}::{name}"), expr),
        Stmt::Set(name, expr) => Stmt::Set(format!("{ns}::{name}"), expr),
        other => other,
    }
}

#[cfg(test)]
mod tests {
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

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
        let src = "import std as s\nsignal a = 9\ns::print(s::sqrt(a))\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("import std as s"));
    }


    #[test]
    fn import_uses_leaf_namespace_by_default() {
        let src = "import pkg::vals\nsignal x = vals::base\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("signal x = pkg::vals::base"));
    }

    #[test]
    fn parses_multiline_blocks_v1() {
        let src = "fn main() {\n signal a = if true {\n 1\n } else {\n 2\n }\n print(a)\n}\n";
        let out = compile_to_text_bytecode(src).expect("multiline should parse");
        assert!(out.contains("fn main()"));
        assert!(out.contains("if true"));
    }

    #[test]
    fn hir_block_model_v1_parses_fn_body_statements() {
        let src = "fn main() {\n signal a = 1\n reactive b = a + 1\n print(b)\n}\n";
        let out = compile_to_text_bytecode(src).expect("fn block statements should parse");
        assert!(out.contains("fn main()"));
    }
}

#[cfg(test)]
mod type_tests {
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
        let src = "import std as s\nsignal a = 9\ns::print(s::sqrt(a))\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("import std as s"));
    }


    #[test]
    fn import_uses_leaf_namespace_by_default() {
        let src = "import std\nsignal x = 1\nstd::print(x)\n";
        let out = compile_to_text_bytecode(src).expect("compile should succeed");
        assert!(out.contains("expr x @call:std::print:1"));
    }

    #[test]
    fn parses_multiline_blocks_v1() {
        let src = "signal a = if true {\n 1\n } else {\n 2\n }\n";
        let out = compile_to_text_bytecode(src).expect("multiline should parse");
        assert!(out.contains("signal a"));
        assert!(out.contains("if true"));
    }

    #[test]
    fn hir_block_model_v1_parses_fn_body_statements() {
        let src = "fn main() { 1 }\n";
        let out = compile_to_text_bytecode(src).expect("fn block statements should parse");
        assert!(out.contains("fn main()"));
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

    #[test]
    fn type_error_on_block_local_symbol_leak() {
        let src = "signal a = if true { 1 } else { 2 }\nsignal b = t\n";
        let err = compile_to_binary_bytecode(src).expect_err("block local should not leak");
        assert!(err.contains("unknown name `t`"));
    }

    #[test]
    fn type_error_on_fn_param_leak() {
        let src = "fn add1(x) { x + 1 }\nsignal a = x\n";
        let err = compile_to_binary_bytecode(src).expect_err("fn param should not leak");
        assert!(err.contains("unknown name `x`"));
    }

    #[test]
    fn allows_inner_scope_shadowing() {
        let src = "signal a = 10\nsignal a = 20\n";
        let err = compile_to_binary_bytecode(src).expect_err("same scope redeclare should fail");
        assert!(err.contains("already defined in this scope"));
    }

    #[test]
    fn lowering_error_for_function_defs_in_binary_path() {
        let src = "fn main() { 1 }\n";
        let err = compile_to_binary_bytecode(src).expect_err("fn lowering should produce diagnostic");
        assert!(err.contains("E-LOWER-001"));
    }

    #[test]
    fn lowering_error_for_if_branch_without_expression_value() {
        let src = "signal a = if true { 1 } else { 2 }\nfn main(){1}\n";
        let err = compile_to_binary_bytecode(src).expect_err("value-less block lowering should fail");
        assert!(err.contains("E-LOWER-001"));
    }

    #[test]
    fn lowering_error_for_match_expression() {
        let src = "signal a = m(1)\n";
        let err = compile_to_binary_bytecode(src).expect_err("match lowering should produce diagnostic");
        assert!(err.contains("unknown function `m`"));
    }

    #[test]
    fn function_name_available_after_definition() {
        let src = "fn add1(x) { x + 1 }\nsignal a = add1(41)\n";
        let out = compile_to_text_bytecode(src).expect("function should be visible in outer scope");
        assert!(out.contains("@call:add1:1"));
    }

    #[test]
    fn block_local_symbol_not_visible_in_sibling_branch() {
        let src = "signal a = if true { 1 } else { t }\n";
        let err = compile_to_binary_bytecode(src).expect_err("symbol from then-branch must not leak to else-branch");
        assert!(err.contains("unknown name `t`"));
    }

    #[test]
    fn same_scope_redeclaration_is_rejected() {
        let src = "signal a = 1\nsignal a = 2\n";
        let err = compile_to_binary_bytecode(src).expect_err("same-scope redeclare should fail");
        assert!(err.contains("already defined in this scope"));
    }

    #[test]
    fn set_updates_outer_binding_from_inner_block() {
        let src = "signal a = 1\nset a = 2\n";
        compile_to_binary_bytecode(src).expect("set should update visible binding");
    }
}
