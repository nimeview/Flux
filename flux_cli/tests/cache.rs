//! Compile cache tests

use std::fs;
use std::path::PathBuf;

use flux_cli::cache::{cache_matches, deserialize_program, serialize_program, CompileCache};
use flux_cli::pipeline::{compile, compile_file, source_from};
use flux_diagnostics::DiagnosticBag;
use flux_hir::lower_program;
use flux_ir::lower_to_ir;
use flux_resolve::resolve_file;
use flux_runtime::ExecutionMode;
use flux_syntax::parse_program;

fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs")
}

#[test]
fn cache_put_get_roundtrip() {
    let mut cache = CompileCache::new();
    let key = "test-key";
    cache.put(key, vec![1, 2, 3]);
    assert_eq!(cache.get(key), Some(vec![1, 2, 3]));
}

#[test]
fn serialize_program_has_nodes() {
    let src = "let a = 1\nlet b = 2\n";
    let mut diag = DiagnosticBag::new();
    let ast = parse_program(flux_diagnostics::SourceId(0), src, &mut diag).unwrap();
    let hir = lower_program(&ast, &mut diag).unwrap();
    let ir = lower_to_ir(&hir, flux_diagnostics::SourceId(0), &mut diag).unwrap();
    let blob = serialize_program(&ir);
    assert!(!blob.is_empty());
}

#[test]
fn compile_succeeds_with_cache_module() {
    let src = "let x = 42\n";
    let source = source_from("cache_test.fl", src);
    let compiled = compile(source).expect("compile");
    assert_eq!(compiled.executor.get_number("x"), Some(42.0));
}

#[test]
fn cache_key_resolved_changes_when_dependency_changes() {
    let root = tempfile::tempdir().expect("tempdir");
    let math_path = root.path().join("math.fl");
    let main_path = root.path().join("import_math.fl");
    fs::write(&math_path, "pub fn double(x: Int) -> Int { x * 2 }\n").unwrap();
    fs::write(
        &main_path,
        "import math as m\n\nlet x = 5\nlet y = m.double(x)\n",
    )
    .unwrap();

    let mut diag = DiagnosticBag::new();
    let resolved_v1 = resolve_file(&main_path, &mut diag).expect("resolve v1");
    let key_v1 = CompileCache::cache_key_resolved(&resolved_v1);

    fs::write(&math_path, "pub fn double(x: Int) -> Int { x * 3 }\n").unwrap();
    let resolved_v2 = resolve_file(&main_path, &mut diag).expect("resolve v2");
    let key_v2 = CompileCache::cache_key_resolved(&resolved_v2);

    assert_ne!(key_v1, key_v2, "dependency change must change cache key");
}

#[test]
fn cache_recompiles_after_dependency_change() {
    let root = tempfile::tempdir().expect("tempdir");
    let math_path = root.path().join("math.fl");
    let main_path = root.path().join("import_math.fl");
    fs::write(&math_path, "pub fn double(x: Int) -> Int { x * 2 }\n").unwrap();
    fs::write(
        &main_path,
        "import math as m\n\nlet x = 5\nlet y = m.double(x)\n",
    )
    .unwrap();

    let compiled_v1 = compile_file(&main_path, ExecutionMode::Vm).expect("compile v1");
    assert_eq!(compiled_v1.executor.get_number("y"), Some(10.0));

    fs::write(&math_path, "pub fn double(x: Int) -> Int { x * 3 }\n").unwrap();
    let compiled_v2 = compile_file(&main_path, ExecutionMode::Vm).expect("compile v2");
    assert_eq!(
        compiled_v2.executor.get_number("y"),
        Some(15.0),
        "must not use stale IR from dependency v1"
    );
}

#[test]
fn cache_program_roundtrip() {
    let src = "let a = 1\nlet b = 2\n";
    let mut diag = DiagnosticBag::new();
    let ast = parse_program(flux_diagnostics::SourceId(0), src, &mut diag).unwrap();
    let hir = lower_program(&ast, &mut diag).unwrap();
    let ir = lower_to_ir(&hir, flux_diagnostics::SourceId(0), &mut diag).unwrap();
    let blob = serialize_program(&ir);
    let restored = deserialize_program(&blob).expect("deserialize cached IR");
    assert_eq!(restored.nodes.len(), ir.nodes.len());
    assert!(cache_matches(&blob, &ir));
}

#[test]
fn cache_matches_rejects_stale_blob() {
    let src = "let a = 1\nlet b = 2\n";
    let mut diag = DiagnosticBag::new();
    let ast = parse_program(flux_diagnostics::SourceId(0), src, &mut diag).unwrap();
    let hir = lower_program(&ast, &mut diag).unwrap();
    let ir_v1 = lower_to_ir(&hir, flux_diagnostics::SourceId(0), &mut diag).unwrap();
    let blob_v1 = serialize_program(&ir_v1);

    let src2 = "let a = 1\nlet b = 2\nlet c = 3\n";
    let ast2 = parse_program(flux_diagnostics::SourceId(0), src2, &mut diag).unwrap();
    let hir2 = lower_program(&ast2, &mut diag).unwrap();
    let ir_v2 = lower_to_ir(&hir2, flux_diagnostics::SourceId(0), &mut diag).unwrap();

    assert!(!cache_matches(&blob_v1, &ir_v2));
    assert!(cache_matches(&serialize_program(&ir_v2), &ir_v2));
}

#[test]
fn import_math_cache_key_includes_dependency() {
    let path = programs_dir().join("ok/modules/import_math.fl");
    let mut diag = DiagnosticBag::new();
    let resolved = resolve_file(&path, &mut diag).expect("resolve import_math");
    assert_eq!(resolved.modules.len(), 2);
    let key = CompileCache::cache_key_resolved(&resolved);
    assert!(!key.is_empty());

    let math_only = CompileCache::hash_source(
        &fs::read_to_string(programs_dir().join("ok/modules/math.fl")).unwrap(),
    );
    assert_ne!(
        key, math_only,
        "resolved key must include more than root file"
    );
}

#[test]
fn cache_writes_under_project_flux_dir() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-project-cache");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("flux.toml"), "[package]\nname = \"cache_proj\"\n").unwrap();
    let main = root.join("main.fl");
    fs::write(&main, "let x = 7\n").unwrap();

    let compiled = compile_file(&main, ExecutionMode::Vm).expect("compile");
    assert_eq!(compiled.executor.get_number("x"), Some(7.0));

    let cache_dir = root.join(".flux").join("cache");
    assert!(
        cache_dir.exists(),
        "expected project-local cache dir at {}",
        cache_dir.display()
    );
    assert!(
        fs::read_dir(&cache_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false),
        "expected at least one cache entry in {}",
        cache_dir.display()
    );
    let _ = fs::remove_dir_all(&root);
}
