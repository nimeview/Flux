use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_ir::lower_to_ir;
use flux_runtime::{Executor, RuntimeValue};
use flux_syntax::parse_program;
use flux_types::typecheck_program;

fn compile(src: &str) -> Executor {
    let mut diags = DiagnosticBag::new();
    let source_id = SourceId(0);
    let ast = parse_program(source_id, src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    typecheck_program(&hir, source_id, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, source_id, &mut diags).unwrap();
    Executor::new(ir).expect("valid program")
}

#[test]
fn executes_fn_call_in_derived() {
    let ex = compile("fn double(x: Int) -> Int {\n  x * 2\n}\nlet a = 3\nlet b = double(a)\n");
    assert_eq!(ex.get_number("b"), Some(6.0));
}

#[test]
fn emit_recomputes_derived_incrementally() {
    let mut ex = compile(
        "event Tick\nlet mut base = 0\nlet doubled = base * 2\non Tick {\n  base = base + 1\n}\n",
    );
    assert_eq!(ex.get_number("doubled"), Some(0.0));
    ex.emit("Tick", flux_runtime::RuntimeValue::Unit);
    assert_eq!(ex.get_number("base"), Some(1.0));
    assert_eq!(ex.get_number("doubled"), Some(2.0));
}

#[test]
fn emit_batches_multiple_source_updates() {
    let mut ex = compile(
        "event Go\nlet mut a = 0\nlet mut b = 0\nlet sum = a + b\non Go {\n  a = 1\n  b = 2\n}\n",
    );
    ex.emit("Go", flux_runtime::RuntimeValue::Unit);
    assert_eq!(ex.get_number("a"), Some(1.0));
    assert_eq!(ex.get_number("b"), Some(2.0));
    assert_eq!(ex.get_number("sum"), Some(3.0));
}

#[test]
fn module_init_if_runs_before_first_recompute() {
    let ex = compile(
        "let use_true = true\nlet mut a = 0\nif use_true {\n    a = 10\n} else {\n    a = 5\n}\n",
    );
    assert_eq!(ex.get_number("a"), Some(10.0));
}

#[test]
fn watch_target_skips_unrelated_source_changes() {
    let mut ex = compile(
        "let a = 1\nlet mut b = 100\nlet out = a + 1\nlet mut log = 0\nwatch out {\n  log = log + 1\n}\n",
    );
    let after_init = ex.get_number("log").unwrap_or(0.0);
    ex.set_source("b", RuntimeValue::int_i64(200));
    assert_eq!(
        ex.get_number("log"),
        Some(after_init),
        "watch on `out` must not run when unrelated source `b` changes"
    );
    ex.set_source("a", RuntimeValue::int_i64(2));
    assert!(
        ex.get_number("log").unwrap_or(0.0) > after_init,
        "watch on `out` must run when dependency `a` changes"
    );
}
