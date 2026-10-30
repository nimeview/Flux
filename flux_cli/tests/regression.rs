//! Regression suite — runs all ok/fail programs.

mod common;

use std::fs;

use flux_cli::pipeline::{compile, compile_file, source_from};
use flux_diagnostics::FluxError;
use flux_runtime::ExecutionMode;

use common::{
    collect_fl_files, compile_program_path, display_path, is_fail_regression_root, read_program,
};

#[test]
fn regression_all_ok_programs_compile() {
    for path in collect_fl_files("ok") {
        compile_program_path(&path);
    }
}

#[test]
fn regression_all_fail_programs_rejected() {
    for path in collect_fl_files("fail") {
        let contents = fs::read_to_string(&path).expect("read program");
        if !is_fail_regression_root(&path, &contents) {
            continue;
        }
        let result = if contents.contains("import ") {
            compile_file(&path, ExecutionMode::Vm).map(|_| ())
        } else {
            let display = display_path(&path);
            let source = source_from(display, contents);
            compile(source).map(|_| ())
        };
        match result {
            Ok(()) => panic!("regression: {} should fail compilation", path.display()),
            Err(err) => assert!(
                !err.diagnostics.errors.is_empty(),
                "regression: {} expected diagnostics",
                path.display()
            ),
        }
    }
}

#[test]
fn regression_fail_programs_have_expected_errors() {
    type ErrorPredicate = fn(&[FluxError]) -> bool;
    let checks: &[(&str, ErrorPredicate)] = &[
        ("fail/reactive/reactive_in_pure.fl", |e| {
            e.iter()
                .any(|x| matches!(x, FluxError::ReactiveInPureContext { .. }))
        }),
        ("fail/bindings/assign_derived.fl", |e| {
            e.iter()
                .any(|x| matches!(x, FluxError::AssignToDerived { .. }))
        }),
    ];

    for (rel, pred) in checks {
        let (display, contents) = read_program(rel);
        let source = source_from(display, contents);
        match compile(source) {
            Ok(_) => panic!("{rel} should fail"),
            Err(err) => assert!(
                pred(&err.diagnostics.errors),
                "{rel}: {:?}",
                err.diagnostics.errors
            ),
        }
    }
}
