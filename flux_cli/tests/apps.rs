//! Integration tests for the real-world sample applications in `programs/apps/`.

use std::path::PathBuf;

use flux_cli::pipeline::{compile, source_from};
use flux_runtime::RuntimeValue;

fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs")
}

fn compile_app(name: &str) -> flux_cli::pipeline::Compiled {
    let path = programs_dir().join("apps").join(name);
    let contents = std::fs::read_to_string(&path).expect("read app program");
    compile(source_from(format!("apps/{name}"), contents))
        .unwrap_or_else(|e| panic!("apps/{name} should compile: {:?}", e.diagnostics.errors))
}

#[test]
fn expenses_alerts_when_budget_is_exceeded() {
    let mut compiled = compile_app("expenses.fl");
    assert_eq!(compiled.executor.get_number("total"), Some(95.0));
    assert_eq!(compiled.executor.get_number("remaining"), Some(5.0));
    assert_eq!(
        compiled.executor.get_value("over_budget"),
        Some(RuntimeValue::Bool(false))
    );

    compiled
        .executor
        .set_source("budget", RuntimeValue::int_i64(50));
    assert_eq!(compiled.executor.get_number("remaining"), Some(-45.0));
    assert_eq!(
        compiled.executor.get_value("over_budget"),
        Some(RuntimeValue::Bool(true))
    );
}

#[test]
fn tasks_tracks_pending_and_completion() {
    let mut compiled = compile_app("tasks.fl");
    assert_eq!(compiled.executor.get_number("pending"), Some(0.0));
    assert_eq!(compiled.executor.get_number("completion_pct"), Some(0.0));

    for _ in 0..3 {
        compiled.executor.emit("AddTask", RuntimeValue::Unit);
    }
    compiled.executor.emit("CompleteTask", RuntimeValue::Unit);

    assert_eq!(compiled.executor.get_number("pending"), Some(2.0));
    assert_eq!(compiled.executor.get_number("done"), Some(1.0));
    assert_eq!(compiled.executor.get_number("total"), Some(3.0));
    let completion = compiled
        .executor
        .get_number("completion_pct")
        .expect("completion_pct");
    assert!((completion - 100.0 / 3.0).abs() < 1e-9, "got {completion}");
}

#[test]
fn sales_rollup_applies_tax_and_averages() {
    let compiled = compile_app("sales_rollup.fl");
    let floats =
        |vs: &[f64]| RuntimeValue::Array(vs.iter().map(|n| RuntimeValue::float_f64(*n)).collect());
    assert_eq!(
        compiled.executor.get_value("taxed"),
        Some(floats(&[108.0, 216.0, 162.0, 324.0, 270.0]))
    );
    assert_eq!(compiled.executor.get_number("total"), Some(1080.0));
    assert_eq!(compiled.executor.get_number("avg"), Some(216.0));
}

#[test]
fn all_app_programs_exist_and_compile() {
    for name in ["expenses.fl", "tasks.fl", "sales_rollup.fl"] {
        let path = programs_dir().join("apps").join(name);
        assert!(path.is_file(), "missing apps/{name}");
        compile_app(name);
    }
}
