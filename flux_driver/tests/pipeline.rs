//! Driver pipeline phase tests.

use std::fs;
use std::path::PathBuf;

use flux_driver::{check_file, compile_file, CompileOptions, CompilePhase};
use flux_runtime::ExecutionMode;

#[test]
fn check_file_skips_executor() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("driver-check-test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let main = root.join("ok.fl");
    fs::write(&main, "let x = 1\nlet y = x + 1\n").unwrap();

    let checked = check_file(&main).expect("typecheck");
    assert!(checked.types.contains_key("y"));
    assert!(checked.types.contains_key("x"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn compile_file_rejects_check_phase() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("driver-compile-check-test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let main = root.join("ok.fl");
    fs::write(&main, "let x = 1\n").unwrap();

    let err = compile_file(&main, CompileOptions::check());
    assert!(err.is_err(), "compile_file must reject Check phase");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn compile_file_full_builds_executor() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("driver-full-test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let main = root.join("ok.fl");
    fs::write(&main, "let x = 2\n").unwrap();

    let compiled = compile_file(&main, CompileOptions::run(ExecutionMode::Vm)).expect("compile");
    assert_eq!(compiled.executor.get_number("x"), Some(2.0));
    assert_eq!(
        CompilePhase::Full,
        CompileOptions::run(ExecutionMode::Vm).phase
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn compile_file_reuses_cached_ir_without_relowering() {
    use flux_diagnostics::DiagnosticBag;
    use flux_driver::{deserialize_program, CompileCache};
    use flux_resolve::resolve_file;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("driver-cache-hit-test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let main = root.join("main.fl");
    fs::write(&main, "let x = 9\n").unwrap();

    let first = compile_file(&main, CompileOptions::run(ExecutionMode::Vm)).expect("first compile");
    assert_eq!(first.executor.get_number("x"), Some(9.0));

    let mut diag = DiagnosticBag::new();
    let resolved = resolve_file(&main, &mut diag).expect("resolve");
    let key = CompileCache::cache_key_resolved(&resolved);
    let cache = CompileCache::for_entry(&main);
    let blob = cache.get(&key).expect("cache entry after compile");
    assert!(
        deserialize_program(&blob).is_some(),
        "on-disk cache must store a full CompiledProgram"
    );

    let second = compile_file(&main, CompileOptions::run(ExecutionMode::Vm)).expect("cache hit");
    assert_eq!(second.executor.get_number("x"), Some(9.0));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_file_reuses_analysis_cache() {
    use flux_diagnostics::DiagnosticBag;
    use flux_driver::{get_analysis_cache, CompileCache};
    use flux_resolve::resolve_file;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("driver-analysis-cache-test");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let main = root.join("main.fl");
    fs::write(&main, "let n = 3\nlet m = n + 1\n").unwrap();

    let first = check_file(&main).expect("first check");
    assert_eq!(first.types.get("m"), Some(&"Read<Int64>".to_string()));

    let mut diag = DiagnosticBag::new();
    let resolved = resolve_file(&main, &mut diag).expect("resolve");
    let key = CompileCache::cache_key_resolved(&resolved);
    assert!(
        get_analysis_cache(&key).is_some(),
        "analysis cache populated"
    );

    let second = check_file(&main).expect("cached check");
    assert_eq!(second.types.get("m"), first.types.get("m"));
    let _ = fs::remove_dir_all(&root);
}
