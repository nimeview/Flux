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
    Ok(bytecode::emit_binary(&lowered))
}

pub fn compile_file_to_binary_bytecode(entry_file: &Path) -> Result<Vec<u8>, String> {
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    load_module(entry_file, None, &mut visited, &mut ordered)?;

    semantic::type_check(&ordered)?;
    let lowered = semantic::resolve_namespaces(&ordered);
    Ok(bytecode::emit_binary(&lowered))
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
        let err = compile_to_text_bytecode(src).expect_err("type check should fail without imported symbol table");
        assert!(err.contains("unknown name `vals::base`"));
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

#[cfg(test)]
mod bytecode_v3_tests {
    use super::*;

    #[test]
    fn binary_if_uses_branch_and_jump() {
        let src = "signal a = if true { 1 } else { 2 }\n";
        let bc = compile_to_binary_bytecode(src).expect("binary compile");
        assert_eq!(&bc[0..4], b"FLXB");
        assert_eq!(bc[4], 3);
        assert!(bc.contains(&11));
        assert!(bc.contains(&12));
    }

    #[test]
    fn binary_match_contains_pattern_compare_opcodes() {
        let src = "signal a = match 2 { 1 => 10, 2 => 20, _ => 0 }\n";
        let bc = compile_to_binary_bytecode(src).expect("binary compile");
        assert!(bc.contains(&13));
        assert!(bc.contains(&16));
        assert!(bc.contains(&12));
    }

    #[test]
    fn binary_fail_fast_for_unsupported_runtime_constructs() {
        for src in [
            "signal a = \"hello\"\n",
            "signal a = (1, 2)\n",
            "signal a = |x| x + 1\n",
            "fn inc(x){ x + 1 }\nsignal a = 1\n",
        ] {
            let err = std::panic::catch_unwind(|| compile_to_binary_bytecode(src));
            assert!(err.is_err());
        }
    }
}
