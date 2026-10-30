use std::fs;
use std::path::{Path, PathBuf};

mod common;

use flux_cli::pipeline::{compile, source_from};
use flux_diagnostics::FluxError;
use flux_driver::{check_file, compile_file, CompileOptions};
use flux_runtime::ExecutionMode;
use flux_runtime::RuntimeValue;

use common::{parse_test_mode, programs_dir, read_program, TestMode};

fn read_program_path(relative: &str) -> (PathBuf, String, String) {
    let path = programs_dir().join(relative);
    let contents = fs::read_to_string(&path).expect("read program");
    let display = common::display_path(&path);
    (path, display, contents)
}

#[test]
fn ok_basics() {
    run_ok(
        "ok/basics/basics.fl",
        &[("a", 5.0), ("b", 2.0), ("c", 10.0)],
    );
}

#[test]
fn ok_arithmetic() {
    run_ok(
        "ok/basics/arithmetic.fl",
        &[
            ("sum", 5.0),
            ("product", 6.0),
            ("diff", 1.0),
            ("ratio", 1.5),
        ],
    );
}

#[test]
fn ok_dynamic_if_deps() {
    run_ok("ok/reactive/dynamic_if_deps.fl", &[("z", 1.0)]);
}

#[test]
fn ok_if_max() {
    run_ok(
        "ok/control_flow/if_max.fl",
        &[("biggest", 5.0), ("smallest", 2.0)],
    );
}

#[test]
fn ok_let_mix() {
    run_ok(
        "ok/bindings/let_mix.fl",
        &[("x", 42.0), ("doubled", 84.0), ("triple", 126.0)],
    );
}

#[test]
fn ok_fn_call() {
    run_ok("ok/functions/fn_call.fl", &[("a", 3.0), ("b", 6.0)]);
}

#[test]
fn ok_bool_ops() {
    run_ok(
        "ok/basics/bool_ops.fl",
        &[
            ("and_tt", 1.0),
            ("or_tf", 1.0),
            ("not_f", 1.0),
            ("mixed", 1.0),
        ],
    );
}

#[test]
fn ok_events() {
    let (path, contents) = read_program("ok/reactive/events.fl");
    let source = source_from(path, contents);
    let mut compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/events.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    assert_eq!(compiled.executor.get_number("n"), Some(0.0));
    compiled.executor.emit("Inc", RuntimeValue::Unit);
    assert_eq!(compiled.executor.get_number("n"), Some(1.0));
}

#[test]
fn ok_watch() {
    run_ok("ok/reactive/watch.fl", &[("a", 5.0), ("side", 10.0)]);
}

#[test]
fn ok_rfn_call() {
    run_ok("ok/functions/rfn_call.fl", &[("a", 3.0), ("b", 6.0)]);
}

#[test]
fn ok_array_for() {
    run_ok("ok/control_flow/array_for.fl", &[("total", 3.0)]);
}

#[test]
fn ok_reassign() {
    run_ok("ok/bindings/reassign.fl", &[("a", 10.0), ("b", 11.0)]);
}

#[test]
fn ok_fn_infer() {
    run_ok("ok/functions/fn_infer.fl", &[("a", 3.0), ("b", 6.0)]);
}

#[test]
fn ok_short_circuit() {
    run_ok("ok/control_flow/short_circuit.fl", &[("ok", 1.0)]);
}

#[test]
fn ok_while_sum() {
    run_ok("ok/control_flow/while_sum.fl", &[("sum", 3.0)]);
}

#[test]
fn ok_for_range() {
    run_ok("ok/control_flow/for_range.fl", &[("total", 3.0)]);
}

#[test]
fn ok_snap() {
    run_ok("ok/bindings/snap.fl", &[("a", 10.0)]);
}

#[test]
fn ok_string_concat() {
    run_ok_strings("ok/stdlib/string_concat.fl", &[("msg", "Hello World")]);
}

#[test]
fn ok_if_init() {
    run_ok("ok/control_flow/if_init.fl", &[("a", 10.0)]);
}

#[test]
fn ok_watch_target() {
    let (path, contents) = read_program("ok/reactive/watch_target.fl");
    let source = source_from(path, contents);
    let mut compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/watch_target.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    assert_eq!(compiled.executor.get_number("side"), Some(10.0));
    compiled
        .executor
        .set_source("b", RuntimeValue::int_i64(100));
    assert_eq!(
        compiled.executor.get_number("side"),
        Some(10.0),
        "watch a should not rerun when b changes"
    );
    compiled.executor.set_source("a", RuntimeValue::int_i64(6));
    assert_eq!(
        compiled.executor.get_number("side"),
        Some(12.0),
        "watch a should rerun when a changes"
    );
}

#[test]
fn ok_rfn_reactive() {
    let (path, contents) = read_program("ok/functions/rfn_reactive.fl");
    let source = source_from(path, contents);
    let mut compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/rfn_reactive.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    assert_eq!(compiled.executor.get_number("b"), Some(15.0));
    compiled
        .executor
        .set_source("base", RuntimeValue::int_i64(20));
    assert_eq!(
        compiled.executor.get_number("b"),
        Some(25.0),
        "derived calling rfn should track reactive reads in rfn body"
    );
}

#[test]
fn ok_types_in_pipeline() {
    let (path, contents) = read_program("ok/basics/basics.fl");
    let source = source_from(path, contents);
    let compiled = compile(source).unwrap();
    assert!(compiled.types.contains_key("a"));
    assert!(compiled.types.contains_key("c"));
}

#[test]
fn ok_zero_arg_call() {
    run_ok("ok/functions/zero_arg_call.fl", &[("x", 42.0)]);
}

#[test]
fn ok_return_fn() {
    run_ok("ok/functions/return_fn.fl", &[("result", 7.0)]);
}

#[test]
fn ok_break_continue() {
    run_ok("ok/control_flow/break_continue.fl", &[("result", 10.0)]);
}

#[test]
fn ok_generic_id() {
    run_ok("ok/generics/generic_id.fl", &[("result", 42.0)]);
}

#[test]
fn ok_closure() {
    run_ok("ok/closures/closure.fl", &[("result", 10.0)]);
}

#[test]
fn ok_closure_capture() {
    let (path, contents) = read_program("ok/closures/closure_capture.fl");
    let source = source_from(path, contents);
    let mut compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/closure_capture.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    assert_eq!(compiled.executor.get_number("count"), Some(2.0));
    compiled
        .executor
        .set_source("threshold", RuntimeValue::int_i64(1));
    assert_eq!(
        compiled.executor.get_number("count"),
        Some(4.0),
        "big should recompute when the captured `threshold` source changes"
    );
}

#[test]
fn ok_trait_display() {
    run_ok_strings("ok/traits/trait_display.fl", &[("msg", "int")]);
}

#[test]
fn ok_trait_dispatch() {
    run_ok_strings(
        "ok/traits/trait_dispatch.fl",
        &[("from_int", "int"), ("from_string", "string")],
    );
}

#[test]
fn ok_push_pop_string() {
    run_ok("ok/stdlib/push_pop_string.fl", &[("count", 3.0)]);
}

#[test]
fn ok_generic_call() {
    run_ok("ok/generics/generic_call.fl", &[("int_result", 42.0)]);
    run_ok_strings("ok/generics/generic_call.fl", &[("str_result", "hi")]);
}

#[test]
fn ok_generic_syntax() {
    let (path, contents) = read_program("ok/generics/generic_syntax.fl");
    let source = source_from(path, contents);
    assert!(compile(source).is_ok());
}

#[test]
fn ok_enum_match() {
    run_ok("ok/types/enum_match.fl", &[("code", 20.0)]);
}

#[test]
fn ok_struct_lit() {
    let (path, contents) = read_program("ok/types/struct_lit.fl");
    let source = source_from(path, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/struct_lit.fl to compile, got errors: {:?}",
            e.diagnostics.errors
        )
    });
    assert!(compiled.executor.get_value("p").is_some());
}

#[test]
fn ok_map_basic() {
    let (path, contents) = read_program("ok/stdlib/map_basic.fl");
    let source = source_from(path, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/map_basic.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    let ints =
        |vs: &[i64]| RuntimeValue::Array(vs.iter().map(|n| RuntimeValue::int_i64(*n)).collect());
    assert_eq!(
        compiled.executor.get_value("doubled"),
        Some(ints(&[2, 4, 6, 8, 10]))
    );
    assert_eq!(
        compiled.executor.get_value("times3"),
        Some(ints(&[3, 6, 9, 12, 15]))
    );
    assert_eq!(
        compiled.executor.get_value("via_fn"),
        Some(ints(&[2, 4, 6, 8, 10]))
    );
}

#[test]
fn ok_filter_basic() {
    let (path, contents) = read_program("ok/stdlib/filter_basic.fl");
    let source = source_from(path, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/filter_basic.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    let ints =
        |vs: &[i64]| RuntimeValue::Array(vs.iter().map(|n| RuntimeValue::int_i64(*n)).collect());
    assert_eq!(
        compiled.executor.get_value("evens"),
        Some(ints(&[1, 2, 3, 4, 5, 6]))
    );
    assert_eq!(
        compiled.executor.get_value("odds"),
        Some(ints(&[1, 2, 3, 4, 5, 6]))
    );
    assert_eq!(compiled.executor.get_value("big"), Some(ints(&[4, 5, 6])));
    assert_eq!(compiled.executor.get_value("small"), Some(ints(&[1, 2, 3])));
}

#[test]
fn ok_reduce_basic() {
    run_ok("ok/stdlib/reduce_basic.fl", &[("total", 5.0)]);
}

#[test]
fn ok_higher_order() {
    let (path, contents) = read_program("ok/closures/higher_order.fl");
    let source = source_from(path, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/higher_order.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    let ints =
        |vs: &[i64]| RuntimeValue::Array(vs.iter().map(|n| RuntimeValue::int_i64(*n)).collect());
    assert_eq!(
        compiled.executor.get_value("squared"),
        Some(ints(&[1, 4, 9, 16, 25]))
    );
    assert_eq!(compiled.executor.get_value("big"), Some(ints(&[16, 25])));
    assert_eq!(compiled.executor.get_number("total"), Some(5.0));
}

#[test]
fn ok_str_join() {
    run_ok_strings("ok/stdlib/str_join.fl", &[("joined", "hello-world-flux")]);
}

#[test]
fn ok_import_math() {
    let path = programs_dir().join("ok/modules/import_math.fl");
    let _contents = std::fs::read_to_string(&path).expect("read import_math");
    let compiled =
        compile_file(&path, CompileOptions::run(ExecutionMode::Vm)).unwrap_or_else(|e| {
            panic!("import_math.fl should compile: {:?}", e.diagnostics.errors);
        });
    assert_eq!(compiled.executor.get_number("y"), Some(10.0));
}

#[test]
fn fail_reactive_in_pure() {
    run_fail("fail/reactive/reactive_in_pure.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::ReactiveInPureContext { .. }))
    });
}

#[test]
fn fail_assign_derived() {
    run_fail("fail/bindings/assign_derived.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::AssignToDerived { .. }))
    });
}

#[test]
fn fail_snap_in_derived() {
    run_fail("fail/reactive/snap_in_derived.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::InvalidControlFlowInDerived { .. }))
    });
}

#[test]
fn fail_duplicate_def() {
    run_fail("fail/syntax/duplicate_def.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::DuplicateDefinition { .. }))
    });
}

#[test]
fn fail_type_mismatch() {
    run_fail("fail/types/type_mismatch.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::TypeMismatch { .. }))
    });
}

#[test]
fn fail_unknown_type() {
    run_fail("fail/types/unknown_type.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::UnknownType { .. }))
    });
}

#[test]
fn fail_compare_mixed() {
    run_fail("fail/types/compare_mixed.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::TypeMismatch { .. }))
    });
}

#[test]
fn fail_assign_immutable() {
    run_fail("fail/bindings/assign_immutable.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::AssignToImmutableLocal { .. }))
    });
}

#[test]
fn fail_cyclic_deps() {
    run_fail("fail/reactive/cyclic_deps.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::CyclicDependency { .. }))
    });
}

#[test]
fn fail_watch_in_fn() {
    run_fail("fail/reactive/watch_in_fn.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::ReactiveEffectInFunction { .. }))
    });
}

fn run_ok(relative: &str, expected: &[(&str, f64)]) {
    let (path_buf, display, contents) = read_program_path(relative);
    if parse_test_mode(&contents) == TestMode::CheckOnly {
        check_file(&path_buf).expect("check-only program should typecheck");
        return;
    }
    let source = source_from(display, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected {relative} to compile, got errors: {:?}",
            e.diagnostics.errors
        )
    });
    for (name, value) in expected {
        assert_eq!(
            compiled.executor.get_number(name),
            Some(*value),
            "{relative}: binding `{name}`"
        );
    }
}

fn run_ok_strings(relative: &str, expected: &[(&str, &str)]) {
    let (path_buf, display, contents) = read_program_path(relative);
    if parse_test_mode(&contents) == TestMode::CheckOnly {
        check_file(&path_buf).expect("check-only program should typecheck");
        return;
    }
    let source = source_from(display, contents);
    let compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected {relative} to compile, got errors: {:?}",
            e.diagnostics.errors
        )
    });
    for (name, value) in expected {
        assert_eq!(
            compiled.executor.get_string(name),
            Some(value.to_string()),
            "{relative}: binding `{name}`"
        );
    }
}

fn run_fail(relative: &str, predicate: impl FnOnce(&[FluxError]) -> bool) {
    let path = programs_dir().join(relative);
    match compile_file(&path, CompileOptions::default()) {
        Ok(_) => panic!("expected {relative} to fail compilation"),
        Err(err) => assert!(
            predicate(&err.diagnostics.errors),
            "{relative}: unexpected errors {:?}",
            err.diagnostics.errors
        ),
    }
}

#[test]
fn ok_type_alias() {
    run_ok(
        "ok/bindings/type_alias.fl",
        &[("id", 42.0), ("doubled", 84.0)],
    );
}

#[test]
fn ok_const_binding() {
    run_ok(
        "ok/bindings/const_binding.fl",
        &[("N", 42.0), ("doubled", 84.0)],
    );
}

#[test]
fn ok_match_guard() {
    run_ok("ok/types/match_guard.fl", &[("x", 5.0), ("result", 5.0)]);
}

#[test]
fn ok_string_interp() {
    run_ok_strings("ok/stdlib/string_interp.fl", &[("msg", "Hello, Flux!")]);
}

#[test]
fn ok_destructure_let() {
    run_ok(
        "ok/types/destructure_let.fl",
        &[("x", 3.0), ("y", 4.0), ("sum", 7.0)],
    );
}

#[test]
fn ok_closure_capture_derived() {
    let (path, contents) = read_program("ok/closures/closure_capture_derived.fl");
    let source = source_from(path, contents);
    let mut compiled = compile(source).unwrap_or_else(|e| {
        panic!(
            "expected ok/closure_capture_derived.fl to compile: {:?}",
            e.diagnostics.errors
        )
    });
    assert_eq!(compiled.executor.get_number("count"), Some(3.0));
    compiled.executor.set_source("x", RuntimeValue::int_i64(5));
    assert_eq!(
        compiled.executor.get_number("count"),
        Some(0.0),
        "filter should recompute when captured derived `doubled` changes"
    );
}

#[test]
fn fail_assign_const() {
    run_fail("fail/bindings/assign_const.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::AssignToImmutableLocal { .. }))
    });
}

#[test]
fn fail_let_mut_required() {
    run_fail("fail/bindings/let_mut_required.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::ImmutableLetReadsReactive { .. }))
    });
}

#[test]
fn fail_io_in_derived() {
    run_fail("fail/reactive/io_in_derived.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::SideEffectInDerived { .. }))
    });
}

#[test]
fn ok_io_read_file() {
    run_ok("ok/io/io_read_file.fl", &[]);
}

#[test]
fn ok_cat() {
    run_ok("ok/io/cat.fl", &[]);
}

#[test]
fn ok_append_file() {
    run_ok("ok/io/append_file.fl", &[]);
}

#[test]
fn ok_file_exists() {
    run_ok("ok/io/file_exists.fl", &[]);
}

#[test]
fn ok_read_line_program_compiles() {
    common::compile_program_path(&programs_dir().join("ok/io/read_line.fl"));
}

#[test]
fn ok_derived_keyword() {
    run_ok(
        "ok/bindings/derived_keyword.fl",
        &[("base", 10.0), ("total", 20.0)],
    );
}

#[test]
fn fail_print_in_derived() {
    run_fail("fail/reactive/print_in_derived.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::SideEffectInDerived { .. }))
    });
}

#[test]
fn fail_import_escape() {
    run_fail("fail/modules/import_escape.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::ImportEscape { .. }))
    });
}

#[test]
fn fail_unknown_export() {
    run_fail("fail/modules/unknown_export.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::UnknownExport { .. }))
    });
}

#[test]
fn fail_cyclic_import() {
    run_fail("fail/modules/cyclic_import.fl", |errors| {
        errors
            .iter()
            .any(|e| matches!(e, FluxError::CyclicImport { .. }))
    });
}

fn collect_fl_files(category: &str) -> Vec<String> {
    fn walk(base: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(base).unwrap_or_else(|e| panic!("read {}: {e}", base.display())) {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|ext| ext == "fl") {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }

    let mut files = Vec::new();
    let base = programs_dir().join(category);
    walk(&base, &programs_dir(), &mut files);
    files.sort();
    files
}

#[test]
fn all_ok_programs_exist() {
    let files = collect_fl_files("ok");
    assert_eq!(files.len(), 65, "unexpected ok program set: {files:?}");
}

#[test]
fn all_fail_programs_exist() {
    let files = collect_fl_files("fail");
    assert_eq!(files.len(), 20, "unexpected fail program set: {files:?}");
}

// silence unused Path import on some toolchains
const _: fn(&Path) = |_: &Path| {};
