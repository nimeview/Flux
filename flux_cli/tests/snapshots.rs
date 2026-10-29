//! Output snapshot tests — compare runtime output against `.expected` files.

use std::fs;
use std::path::PathBuf;

use flux_cli::output::format_runtime;
use flux_cli::pipeline::{compile, source_from};

fn programs_ok_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs/ok")
}

fn snapshot_program(relative: &str) {
    let flux_path = programs_ok_dir().join(relative);
    let expected_path = flux_path.with_extension("expected");

    let contents = fs::read_to_string(&flux_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", flux_path.display()));
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display()));

    let display = flux_path
        .strip_prefix(programs_ok_dir().parent().unwrap())
        .unwrap_or(&flux_path)
        .display()
        .to_string();
    let source = source_from(display, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "snapshot {relative}: compile failed: {:?}",
            e.diagnostics.errors
        );
    });

    let actual = format_runtime(&compiled.executor);
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "snapshot mismatch for {relative}"
    );
}

#[test]
fn snapshot_basics() {
    snapshot_program("basics/basics.fl");
}

#[test]
fn snapshot_arithmetic() {
    snapshot_program("basics/arithmetic.fl");
}

#[test]
fn snapshot_enum_match() {
    snapshot_program("types/enum_match.fl");
}

#[test]
fn snapshot_fn_call() {
    snapshot_program("functions/fn_call.fl");
}

#[test]
fn snapshot_while_sum() {
    snapshot_program("control_flow/while_sum.fl");
}

#[test]
fn snapshot_for_range() {
    snapshot_program("control_flow/for_range.fl");
}

#[test]
fn snapshot_for_reactive() {
    snapshot_program("control_flow/for_reactive.fl");
}

#[test]
fn snapshot_let_scope() {
    snapshot_program("bindings/let_scope.fl");
}

#[test]
fn snapshot_nested_while() {
    snapshot_program("control_flow/nested_while.fl");
}

#[test]
fn snapshot_early_return() {
    snapshot_program("control_flow/early_return.fl");
}

#[test]
fn snapshot_enum_match_payload() {
    snapshot_program("types/enum_match_payload.fl");
}

#[test]
fn snapshot_option_match_payload() {
    snapshot_program("types/option_match_payload.fl");
}

#[test]
fn snapshot_stdlib_extended() {
    snapshot_program("stdlib/stdlib_extended.fl");
}
