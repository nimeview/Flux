//! Numeric execution contract (see `flux_value::arith`), checked in both engines.
//!
//! Every case computes a derived binding `r`. The VM result is the reference;
//! the JIT must produce the same value or the same runtime error, and must have
//! compiled `r` natively rather than falling back to the VM.

use std::sync::{Mutex, MutexGuard, OnceLock};

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_diagnostics::FluxError;
use flux_runtime::{ExecutionMode, RuntimeValue};

fn jit_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug)]
enum Want {
    Value(&'static str),
    Error(&'static str),
}
use Want::{Error, Value};

const MAX: &str = "9223372036854775807";
const MIN: &str = "-9223372036854775807 - 1";

fn cases() -> Vec<(&'static str, String, Want)> {
    let c = |name, src: &str, want| (name, src.to_string(), want);
    vec![
        // Integer arithmetic stays Int; `/` truncates toward zero.
        c("int_add", "let mut a = 2\nlet r = a + 3", Value("5")),
        c(
            "int_div_truncates",
            "let mut a = 7\nlet r = a / 2",
            Value("3"),
        ),
        c(
            "int_div_negative",
            "let mut a = -7\nlet r = a / 2",
            Value("-3"),
        ),
        c("int_rem", "let mut a = 7\nlet r = a % 3", Value("1")),
        c(
            "int_rem_neg_dividend",
            "let mut a = -7\nlet r = a % 3",
            Value("-1"),
        ),
        c(
            "int_rem_neg_divisor",
            "let mut a = 7\nlet r = a % -3",
            Value("1"),
        ),
        // Integer errors.
        c(
            "div_by_zero",
            "let mut a = 1\nlet r = a / 0",
            Error("integer division by zero"),
        ),
        c(
            "rem_by_zero",
            "let mut a = 1\nlet r = a % 0",
            Error("integer division by zero"),
        ),
        (
            "add_overflow",
            format!("let mut a = {MAX}\nlet r = a + 1"),
            Error("integer overflow"),
        ),
        (
            "sub_overflow",
            format!("let mut a = {MIN}\nlet r = a - 1"),
            Error("integer overflow"),
        ),
        (
            "mul_overflow",
            "let mut a = 4611686018427387904\nlet r = a * 2".into(),
            Error("integer overflow"),
        ),
        (
            "min_div_neg_one",
            format!("let mut a = {MIN}\nlet r = a / -1"),
            Error("integer overflow"),
        ),
        (
            "min_rem_neg_one",
            format!("let mut a = {MIN}\nlet r = a % -1"),
            Value("0"),
        ),
        (
            "neg_min",
            format!("let mut a = {MIN}\nlet r = -a"),
            Error("integer overflow"),
        ),
        c(
            "neg_int_stays_int",
            "let mut a = 5\nlet r = -a / 2",
            Value("-2"),
        ),
        // Narrow widths keep their width and range.
        (
            "int32_overflow",
            "let mut a = 2147483647 as Int32\nlet r = a + 1 as Int32".into(),
            Error("integer overflow"),
        ),
        (
            "uint8_add",
            "let mut a = 200 as UInt8\nlet r = a + 55 as UInt8".into(),
            Value("255"),
        ),
        // Floats follow IEEE 754.
        c(
            "float_div_zero",
            "let mut a = 1.0\nlet r = a / 0.0",
            Value("inf"),
        ),
        c(
            "float_zero_div_zero",
            "let mut a = 0.0\nlet r = a / 0.0",
            Value("NaN"),
        ),
        c(
            "float_rem",
            "let mut a = 7.5\nlet r = a % 2.0",
            Value("1.5"),
        ),
        c(
            "int_plus_float",
            "let mut a = 1\nlet r = a + 1.5",
            Value("2.5"),
        ),
        c(
            "int_div_float",
            "let mut a = 1\nlet r = a / 2.0",
            Value("0.5"),
        ),
        // Comparisons: exact for ints, IEEE for floats.
        (
            "cmp_large_ints_exact",
            format!("let mut a = {MAX}\nlet r = a == {MAX} - 1"),
            Value("false"),
        ),
        (
            "cmp_large_ints_ordered",
            format!("let mut a = {MAX}\nlet r = a > a - 1"),
            Value("true"),
        ),
        c(
            "nan_not_equal_itself",
            "let mut a = 0.0\nlet n = a / 0.0\nlet r = n == n",
            Value("false"),
        ),
        c(
            "int_float_compare",
            "let mut a = 2\nlet r = a < 2.5",
            Value("true"),
        ),
        // Casts are checked.
        c(
            "cast_in_range",
            "let mut a = 200\nlet r = a as UInt8",
            Value("200"),
        ),
        (
            "cast_out_of_range",
            "let mut a = 300\nlet r = a as UInt8".into(),
            Error("numeric cast out of range"),
        ),
        (
            "cast_negative_to_unsigned",
            "let mut a = -1\nlet r = a as UInt64".into(),
            Error("numeric cast out of range"),
        ),
        c(
            "float_to_int_truncates",
            "let mut a = -2.7\nlet r = a as Int",
            Value("-2"),
        ),
        (
            "nan_to_int",
            "let mut a = 0.0\nlet r = (a / 0.0) as Int".into(),
            Error("numeric cast out of range"),
        ),
        // Functions use the same semantics, and errors propagate out of them.
        c(
            "fn_int_division",
            "fn half(x: Int) -> Int { x / 2 }\nlet mut a = 9\nlet r = half(a)",
            Value("4"),
        ),
        c(
            "fn_div_by_zero",
            "fn bad(x: Int) -> Int { x / 0 }\nlet mut a = 9\nlet r = bad(a) + 1",
            Error("integer division by zero"),
        ),
        // Mutable bindings accept any initializer, not only literals.
        c(
            "source_negative_init",
            "let mut a = -5\nlet r = a - 1",
            Value("-6"),
        ),
        c(
            "source_expression_init",
            "fn four() -> Int { 4 }\nlet mut a = 2 + four()\nlet r = a * 2",
            Value("12"),
        ),
    ]
}

/// Value of `r`, or the runtime error message. Also reports whether a JIT run
/// compiled `r` natively.
fn run(src: &str, mode: ExecutionMode) -> (Result<String, String>, bool) {
    let source = source_from("contract.fl", format!("{src}\n"));
    match compile_with_mode(source, mode) {
        Ok(compiled) => {
            let value = compiled
                .executor
                .get_value("r")
                .map(|v: RuntimeValue| v.display_string())
                .expect("binding `r`");
            let native = compiled
                .executor
                .jit_stats()
                .is_some_and(|s| !s.fallback_nodes.iter().any(|n| n == "r"));
            (Ok(value), native)
        }
        Err(failed) => {
            let runtime: Vec<_> = failed
                .diagnostics
                .errors
                .iter()
                .filter_map(|e| match e {
                    FluxError::Runtime { message, .. } => Some(message.clone()),
                    _ => None,
                })
                .collect();
            assert!(
                !runtime.is_empty(),
                "expected only runtime errors, got {:?}",
                failed.diagnostics.errors
            );
            (Err(runtime.join("; ")), false)
        }
    }
}

#[test]
fn vm_follows_numeric_contract() {
    let mut failures = Vec::new();
    for (name, src, want) in cases() {
        let (got, _) = run(&src, ExecutionMode::Vm);
        let ok = match (&want, &got) {
            (Value(v), Ok(g)) => v == g,
            (Error(e), Err(g)) => g.contains(e),
            _ => false,
        };
        if !ok {
            failures.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn jit_matches_vm_on_numeric_contract() {
    let _guard = jit_test_lock();
    let mut failures = Vec::new();
    for (name, src, _) in cases() {
        flux_jit::reset_host_for_tests();
        let (vm, _) = run(&src, ExecutionMode::Vm);
        let (jit, native) = run(&src, ExecutionMode::Jit);
        if vm != jit {
            failures.push(format!("{name}: vm {vm:?} != jit {jit:?}"));
        }
        // An error aborts startup before stats can be read; values must be native.
        if jit.is_ok() && !native {
            failures.push(format!("{name}: `r` fell back to the VM"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// A runtime error inside a handler stops it at the failing statement: later
/// assignments do not happen, in either engine, and the executor records it.
#[test]
fn handler_error_aborts_remaining_statements() {
    let _guard = jit_test_lock();
    let src = "event Tick(n: Int)\nlet mut a = 1\nlet mut b = 0\non Tick { n ->\n    b = 10 / n\n    a = 100\n}\n";
    for mode in [ExecutionMode::Vm, ExecutionMode::Jit] {
        flux_jit::reset_host_for_tests();
        let mut ex = compile_with_mode(source_from("handler.fl", src), mode)
            .expect("compiles")
            .executor;
        ex.emit("Tick", RuntimeValue::int_i64(2));
        assert_eq!(
            ex.get_value("b"),
            Some(RuntimeValue::int_i64(5)),
            "{mode:?}"
        );
        assert_eq!(
            ex.get_value("a"),
            Some(RuntimeValue::int_i64(100)),
            "{mode:?}"
        );
        assert!(ex.runtime_error().is_none(), "{mode:?}");

        ex.set_source("a", RuntimeValue::int_i64(1));
        ex.emit("Tick", RuntimeValue::int_i64(0));
        let err = ex.runtime_error().expect("division by zero is reported");
        assert_eq!(
            err.to_string(),
            "runtime error: integer division by zero in `on Tick`",
            "{mode:?}"
        );
        assert_eq!(
            ex.get_value("a"),
            Some(RuntimeValue::int_i64(1)),
            "{mode:?}"
        );
        assert_eq!(
            ex.get_value("b"),
            Some(RuntimeValue::int_i64(5)),
            "{mode:?}"
        );
    }
}

/// A runtime error in an `if` condition stops execution: neither branch runs,
/// so the `else` branch cannot assign or emit.
#[test]
fn error_in_if_condition_runs_no_branch() {
    let _guard = jit_test_lock();
    let src = "event Tick(n: Int)\nevent Other\nlet mut a = 0\nlet mut b = 0\nlet mut c = 0\non Tick { n ->\n    if 10 / n > 0 {\n        a = 1\n    } else {\n        b = 2\n        emit Other\n    }\n}\non Other {\n    c = 3\n}\n";
    for mode in [ExecutionMode::Vm, ExecutionMode::Jit] {
        flux_jit::reset_host_for_tests();
        let mut ex = compile_with_mode(source_from("if_trap.fl", src), mode)
            .expect("compiles")
            .executor;
        ex.emit("Tick", RuntimeValue::int_i64(0));
        let err = ex.runtime_error().expect("division by zero is reported");
        assert_eq!(err.kind, flux_runtime::ArithError::DivByZero, "{mode:?}");
        for name in ["a", "b", "c"] {
            assert_eq!(
                ex.get_value(name),
                Some(RuntimeValue::int_i64(0)),
                "{mode:?}: `{name}` changed after the error"
            );
        }
    }
}
