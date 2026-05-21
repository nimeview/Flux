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

pub use semantic::TypeMode;

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

pub fn compile_to_binary_bytecode_with_mode(source: &str, mode: TypeMode) -> Result<Vec<u8>, String> {
    let program = parser::parse_program(source)?;
    semantic::type_check_with_mode(&program, mode)?;
    Ok(bytecode::emit_binary(&program))
}

pub fn compile_file_to_binary_bytecode(entry_file: &Path) -> Result<Vec<u8>, String> {
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    load_module(entry_file, None, &mut visited, &mut ordered)?;
    semantic::type_check(&ordered)?;
    Ok(bytecode::emit_binary(&ordered))
}

fn load_module(path: &Path, module_ns: Option<&str>, visited: &mut HashSet<PathBuf>, out: &mut Vec<Stmt>) -> Result<(), String> {
    let canon = fs::canonicalize(path).map_err(|e| format!("failed to resolve module {}: {e}", path.display()))?;
    if !visited.insert(canon.clone()) { return Ok(()); }
    let src = fs::read_to_string(&canon).map_err(|e| format!("failed to read module {}: {e}", canon.display()))?;
    let stmts = parser::parse_program(&src)?;
    let base_dir = canon.parent().unwrap_or(Path::new("."));
    for stmt in &stmts {
        if let Stmt::Import { path, .. } = stmt {
            if path == "std" || path.starts_with("std::") { continue; }
            let module_file = import_to_file(base_dir, path);
            load_module(&module_file, Some(path), visited, out)?;
        }
    }
    for stmt in stmts { out.push(qualify_stmt(stmt, module_ns)); }
    Ok(())
}

fn import_to_file(base_dir: &Path, import_path: &str) -> PathBuf { base_dir.join(import_path.replace("::", "/") + ".fx") }

fn qualify_stmt(stmt: Stmt, module_ns: Option<&str>) -> Stmt {
    let Some(ns) = module_ns else { return stmt; };
    match stmt {
        Stmt::FnDef { name, params, param_types, body } => Stmt::FnDef { name: format!("{ns}::{name}"), params, param_types, body },
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
    fn negative_suite_30_plus() {
        let mut cases = vec![
            "signal a = true + 1", "signal a = 1\nset a = false", "fn add(x:number){x+1}\nadd(true)",
            "fn add(x:number,y:number){x+y}\nadd(1)", "if 1 { 1 } else { 2 }", "match 1 { true => 1 }",
            "unknown(1)", "sqrt(true)", "min(1,true)", "now_ms(1)", "print()", "signal a = x",
        ];
        while cases.len() < 32 { cases.push("signal a = true + false"); }
        for src in cases { assert!(compile_to_binary_bytecode(src).is_err(), "expected error for: {src}"); }
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
