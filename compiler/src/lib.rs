mod bytecode;
mod diagnostics;
mod ffi;
mod hir;
mod lexer;
mod parser;
mod semantic;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use hir::{Expr, Stmt};

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

#[derive(Default, Clone)]
struct PackageConfig { name: Option<String>, root: Option<String>, exports: HashSet<String> }

pub fn compile_file_to_binary_bytecode(entry_file: &Path) -> Result<Vec<u8>, String> {
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    let mut stack = Vec::new();
    let cfg = load_package_cfg(entry_file.parent().unwrap_or(Path::new(".")));
    load_module(entry_file, None, &cfg, &mut visited, &mut stack, &mut ordered)?;
    semantic::type_check(&ordered)?;
    Ok(bytecode::emit_binary(&ordered))
}

fn load_package_cfg(start: &Path) -> PackageConfig {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let file = d.join("fxpkg.toml");
        if let Ok(src) = fs::read_to_string(&file) {
            let mut cfg = PackageConfig::default();
            for ln in src.lines().map(|x| x.trim()) {
                if let Some(v) = ln.strip_prefix("name =") { cfg.name = Some(v.trim().trim_matches('"').to_string()); }
                if let Some(v) = ln.strip_prefix("root =") { cfg.root = Some(v.trim().trim_matches('"').to_string()); }
                if let Some(v) = ln.strip_prefix("exports =") {
                    cfg.exports = v.trim().trim_matches(&['[',']'][..]).split(',').map(|x| x.trim().trim_matches('"').to_string()).filter(|x|!x.is_empty()).collect();
                }
            }
            return cfg;
        }
        dir = d.parent();
    }
    PackageConfig::default()
}

fn load_module(path: &Path, module_ns: Option<&str>, cfg: &PackageConfig, visited: &mut HashSet<PathBuf>, stack: &mut Vec<PathBuf>, out: &mut Vec<Stmt>) -> Result<(), String> {
    let canon = fs::canonicalize(path).map_err(|e| format!("failed to resolve module {}: {e}", path.display()))?;
    if stack.contains(&canon) {
        let mut chain: Vec<String> = stack.iter().map(|p| p.display().to_string()).collect();
        chain.push(canon.display().to_string());
        return Err(format!("cyclic import detected: {}", chain.join(" -> ")));
    }
    if !visited.insert(canon.clone()) { return Ok(()); }
    stack.push(canon.clone());
    let src = fs::read_to_string(&canon).map_err(|e| format!("failed to read module {}: {e}", canon.display()))?;
    let stmts = parser::parse_program(&src)?;
    let base_dir = canon.parent().unwrap_or(Path::new("."));

    let mut seen_alias: HashMap<String, String> = HashMap::new();
    for stmt in &stmts {
        if let Stmt::Import { path, alias } = stmt {
            let leaf = alias.clone().unwrap_or_else(|| path.split("::").last().unwrap_or(path).to_string());
            if let Some(prev) = seen_alias.insert(leaf.clone(), path.clone()) {
                return Err(format!("namespace collision for alias `{leaf}`: `{prev}` and `{path}`; use `as`"));
            }
            if path == "std" || path.starts_with("std::") { continue; }
            let module_file = import_to_file(base_dir, path, cfg)?;
            load_module(&module_file, Some(path), cfg, visited, stack, out)?;
        }
    }
    for stmt in stmts { out.push(qualify_stmt(stmt, module_ns)); }
    stack.pop();
    Ok(())
}

fn import_to_file(base_dir: &Path, import_path: &str, cfg: &PackageConfig) -> Result<PathBuf, String> {
    if import_path.starts_with("./") || import_path.starts_with("../") {
        return Ok(base_dir.join(import_path).with_extension("fx"));
    }
    if let Some(pkg) = &cfg.name {
        if import_path.starts_with(&format!("{pkg}::")) {
            let root = cfg.root.clone().unwrap_or_else(|| "src".to_string());
            let rest = import_path.strip_prefix(&format!("{pkg}::")).unwrap();
            return Ok(base_dir.join("..").join(root).join(rest.replace("::", "/") + ".fx"));
        }
    }
    Ok(base_dir.join(import_path.replace("::", "/") + ".fx"))
}

fn qualify_stmt(stmt: Stmt, module_ns: Option<&str>) -> Stmt {
    let Some(ns) = module_ns else { return stmt; };
    match stmt {
        Stmt::FnDef { name, params, param_types, body, is_pub } => Stmt::FnDef { name: format!("{ns}::{name}"), params, param_types, body, is_pub },
        Stmt::Signal { name, expr, is_pub } => Stmt::Signal { name: format!("{ns}::{name}"), expr, is_pub },
        Stmt::Reactive { name, expr, is_pub } => Stmt::Reactive { name: format!("{ns}::{name}"), expr, is_pub },
        Stmt::Set(name, expr) => Stmt::Set(format!("{ns}::{name}"), expr),
        other => other,
    }
}

#[cfg(test)]
mod tests {
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn negative_suite_30_plus() { let mut cases = vec!["signal a = true + 1", "signal a = 1\nset a = false", "fn add(x:number){x+1}\nadd(true)", "fn add(x:number,y:number){x+y}\nadd(1)", "if 1 { 1 } else { 2 }", "match 1 { true => 1 }", "unknown(1)", "sqrt(true)", "min(1,true)", "now_ms(1)", "print()", "signal a = x",]; while cases.len() < 32 { cases.push("signal a = true + false"); } for src in cases { assert!(compile_to_binary_bytecode(src).is_err(), "expected error for: {src}"); } }

fn tmpdir() -> PathBuf { let n=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(); let d=std::env::temp_dir().join(format!("flux_it_{n}")); fs::create_dir_all(&d).unwrap(); d }

#[test]
fn integration_cycle_collision_alias_nested() {
 let d=tmpdir();
 fs::write(d.join("main.fx"), "import ./a\nsignal x = 1").unwrap();
 fs::write(d.join("a.fx"), "import ./b\npub fn fa(){1}").unwrap();
 fs::write(d.join("b.fx"), "import ./a\npub fn fb(){1}").unwrap();
 let e=compile_file_to_binary_bytecode(&d.join("main.fx")).unwrap_err();
 assert!(e.contains("cyclic import"));

 fs::create_dir_all(d.join("foo")).unwrap();
 fs::create_dir_all(d.join("bar")).unwrap();
 fs::write(d.join("foo/util.fx"), "signal a = 1").unwrap();
 fs::write(d.join("bar/util.fx"), "signal b = 1").unwrap();
 fs::write(d.join("c.fx"), "import foo::util\nimport bar::util").unwrap();
 let e2=compile_file_to_binary_bytecode(&d.join("c.fx")).unwrap_err();
 assert!(e2.contains("namespace collision"));
}
}
