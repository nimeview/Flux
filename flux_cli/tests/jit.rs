//! JIT vs VM parity tests and basic benchmarks.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use std::sync::{Mutex, MutexGuard, OnceLock};

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_jit;
use flux_runtime::{ExecutionMode, RuntimeValue};

fn jit_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("jit test lock poisoned")
}

fn reset_jit() {
    flux_jit::reset_host_for_tests();
}

fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs")
}

fn read_program(relative: &str) -> (String, String) {
    let path = programs_dir().join(relative);
    let contents = fs::read_to_string(&path).expect("read program");
    let display = path
        .strip_prefix(programs_dir())
        .unwrap_or(&path)
        .display()
        .to_string();
    (display, contents)
}

fn assert_parity_values(relative: &str, expected: &[(&str, RuntimeValue)]) {
    reset_jit();
    let (path, contents) = read_program(relative);
    let source = source_from(path, contents);

    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm)
        .unwrap_or_else(|e| panic!("{relative} VM compile failed: {:?}", e.diagnostics.errors));
    let jit = compile_with_mode(source, ExecutionMode::Jit)
        .unwrap_or_else(|e| panic!("{relative} JIT compile failed: {:?}", e.diagnostics.errors));

    for (name, value) in expected {
        assert_eq!(
            vm.executor.get_value(name),
            Some(value.clone()),
            "{relative} VM: `{name}`"
        );
        assert_eq!(
            jit.executor.get_value(name),
            Some(value.clone()),
            "{relative} JIT: `{name}`"
        );
    }
}

fn assert_parity_numbers(relative: &str, expected: &[(&str, f64)]) {
    reset_jit();
    let (path, contents) = read_program(relative);
    let source = source_from(path, contents);

    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm)
        .unwrap_or_else(|e| panic!("{relative} VM compile failed: {:?}", e.diagnostics.errors));
    let jit = compile_with_mode(source, ExecutionMode::Jit)
        .unwrap_or_else(|e| panic!("{relative} JIT compile failed: {:?}", e.diagnostics.errors));

    for (name, value) in expected {
        assert_eq!(
            vm.executor.get_number(name),
            Some(*value),
            "{relative} VM: `{name}`"
        );
        assert_eq!(
            jit.executor.get_number(name),
            Some(*value),
            "{relative} JIT: `{name}`"
        );
    }
}

#[test]
fn jit_parity_int_preserving_add() {
    let _guard = jit_test_lock();
    assert_parity_values(
        "ok/basics/arithmetic.fl",
        &[("sum", RuntimeValue::int_i64(5))],
    );
}

#[test]
fn jit_parity_array_for() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/control_flow/array_for.fl", &[("total", 3.0)]);
}

#[test]
fn jit_parity_enum_match() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/types/enum_match.fl", &[("code", 20.0)]);
    let (path, contents) = read_program("ok/types/enum_match.fl");
    let jit = compile_with_mode(source_from(path, contents), ExecutionMode::Jit).unwrap();
    let stats = jit.executor.jit_stats().expect("jit stats");
    assert!(
        stats.compiled > 0,
        "expected JIT compilation, fallbacks: {:?}",
        stats.fallback_nodes
    );
    assert!(
        !stats.fallback_nodes.iter().any(|n| n == "code"),
        "enum match node should be JIT-compiled"
    );
}

#[test]
fn jit_parity_enum_match_payload() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/types/enum_match_payload.fl", &[("result", 42.0)]);
    let (path, contents) = read_program("ok/types/enum_match_payload.fl");
    let jit = compile_with_mode(source_from(path, contents), ExecutionMode::Jit).unwrap();
    let stats = jit.executor.jit_stats().expect("jit stats");
    assert!(
        !stats.fallback_nodes.iter().any(|n| n == "result"),
        "enum payload match should be JIT-compiled, fallbacks: {:?}",
        stats.fallback_nodes
    );
}

#[test]
fn jit_parity_string_concat() {
    let _guard = jit_test_lock();
    assert_parity_values(
        "ok/stdlib/string_concat.fl",
        &[("msg", RuntimeValue::String("Hello World".into()))],
    );
    let (path, contents) = read_program("ok/stdlib/string_concat.fl");
    let jit = compile_with_mode(source_from(path, contents), ExecutionMode::Jit).unwrap();
    let stats = jit.executor.jit_stats().expect("jit stats");
    assert!(
        !stats.fallback_nodes.iter().any(|n| n == "msg"),
        "string concat should be JIT-compiled, fallbacks: {:?}",
        stats.fallback_nodes
    );
}
#[test]
fn jit_parity_game_score() {
    let _guard = jit_test_lock();
    reset_jit();
    let (path, contents) = read_program("demo/game_score.fl");
    let source = source_from(path, contents);
    let mut vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let mut jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();
    vm.executor.emit("ScoreHome", RuntimeValue::Unit);
    jit.executor.emit("ScoreHome", RuntimeValue::Unit);
    assert_eq!(vm.executor.get_number("home"), Some(3.0));
    assert_eq!(jit.executor.get_number("home"), Some(3.0));
}

#[test]
fn jit_parity_basics() {
    let _guard = jit_test_lock();
    assert_parity_numbers(
        "ok/basics/basics.fl",
        &[("a", 5.0), ("b", 2.0), ("c", 10.0)],
    );
}

#[test]
fn jit_parity_arithmetic() {
    let _guard = jit_test_lock();
    assert_parity_numbers(
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
fn jit_parity_bool_ops() {
    let _guard = jit_test_lock();
    assert_parity_numbers(
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
fn jit_parity_if_max() {
    let _guard = jit_test_lock();
    assert_parity_numbers(
        "ok/control_flow/if_max.fl",
        &[("biggest", 5.0), ("smallest", 2.0)],
    );
}

#[test]
fn jit_parity_short_circuit() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/control_flow/short_circuit.fl", &[("ok", 1.0)]);
}

#[test]
fn jit_parity_reassign() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/bindings/reassign.fl", &[("b", 11.0), ("a", 10.0)]);
}

#[test]
fn jit_parity_watch() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/reactive/watch.fl", &[("side", 10.0)]);
}

#[test]
fn jit_parity_fn_call() {
    let _guard = jit_test_lock();
    assert_parity_numbers("ok/functions/fn_call.fl", &[("b", 6.0)]);
}

#[test]
fn jit_parity_dynamic_if_deps() {
    let _guard = jit_test_lock();
    reset_jit();
    let src = "let mut flag = true\nlet x = 1\nlet mut y = 100\nlet z = if flag { x } else { y }\n";
    let source = source_from("dynamic.fl", src.to_string());
    let mut vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let mut jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();
    assert_eq!(vm.executor.get_value("z"), Some(RuntimeValue::int_i64(1)));
    assert_eq!(jit.executor.get_value("z"), Some(RuntimeValue::int_i64(1)));
    vm.executor.set_source("flag", RuntimeValue::Bool(false));
    jit.executor.set_source("flag", RuntimeValue::Bool(false));
    assert_eq!(vm.executor.get_value("z"), Some(RuntimeValue::int_i64(100)));
    assert_eq!(
        jit.executor.get_value("z"),
        Some(RuntimeValue::int_i64(100))
    );
}

#[test]
fn jit_parity_rfn_reactive() {
    let _guard = jit_test_lock();
    reset_jit();
    let (path, contents) = read_program("ok/functions/rfn_reactive.fl");
    let source = source_from(path, contents);

    let mut vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let mut jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();

    assert_eq!(vm.executor.get_number("b"), Some(15.0));
    assert_eq!(jit.executor.get_number("b"), Some(15.0));

    vm.executor.set_source("base", RuntimeValue::int_i64(20));
    jit.executor.set_source("base", RuntimeValue::int_i64(20));

    assert_eq!(vm.executor.get_number("b"), Some(25.0));
    assert_eq!(jit.executor.get_number("b"), Some(25.0));
}

#[test]
fn jit_parity_events() {
    let _guard = jit_test_lock();
    reset_jit();
    let (path, contents) = read_program("ok/reactive/events.fl");
    let source = source_from(path, contents);

    let mut vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let mut jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();

    vm.executor.emit("Inc", RuntimeValue::Unit);
    jit.executor.emit("Inc", RuntimeValue::Unit);

    assert_eq!(vm.executor.get_number("n"), Some(1.0));
    assert_eq!(jit.executor.get_number("n"), Some(1.0));
}

#[test]
fn jit_bench_counter_1000_updates() {
    let _guard = jit_test_lock();
    reset_jit();
    let src = r#"
event Click
let mut count = 0
let doubled = count * 2
on Click { count = count + 1 }
"#;
    let source = source_from("bench.fl", src);

    let mut vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let vm_start = Instant::now();
    for _ in 0..1000 {
        vm.executor.emit("Click", RuntimeValue::Unit);
    }
    let vm_elapsed = vm_start.elapsed();
    assert_eq!(vm.executor.get_number("count"), Some(1000.0));
    assert_eq!(vm.executor.get_number("doubled"), Some(2000.0));

    let mut jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();
    let jit_start = Instant::now();
    for _ in 0..1000 {
        jit.executor.emit("Click", RuntimeValue::Unit);
    }
    let jit_elapsed = jit_start.elapsed();
    assert_eq!(jit.executor.get_number("count"), Some(1000.0));
    assert_eq!(jit.executor.get_number("doubled"), Some(2000.0));

    // Sanity: both complete in reasonable time (< 5s each on CI)
    assert!(
        vm_elapsed.as_secs() < 5,
        "VM bench too slow: {vm_elapsed:?}"
    );
    assert!(
        jit_elapsed.as_secs() < 5,
        "JIT bench too slow: {jit_elapsed:?}"
    );
    reset_jit();
}

#[test]
fn jit_bench_deep_chain_100() {
    let _guard = jit_test_lock();
    reset_jit();
    let n = 100;
    let mut src = String::from("let a0 = 1\n");
    for i in 1..n {
        src.push_str(&format!("let a{i} = a{} + 1\n", i - 1));
    }

    let source = source_from("chain.fl", src);
    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();

    assert_eq!(vm.executor.get_number("a99"), Some(100.0));
    assert_eq!(jit.executor.get_number("a99"), Some(100.0));
}

#[test]
fn jit_bench_stdlib_math_range() {
    let _guard = jit_test_lock();
    reset_jit();
    let src = r#"
import collections
import math
let n = len(range(100))
let m = abs(0 - 42)
let s = sqrt(81.0)
"#;
    let source = source_from("bench_stdlib.fl", src);
    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();
    assert_eq!(vm.executor.get_number("n"), Some(100.0));
    assert_eq!(jit.executor.get_number("n"), Some(100.0));
    assert_eq!(vm.executor.get_number("m"), Some(42.0));
    assert_eq!(jit.executor.get_number("m"), Some(42.0));
    assert_eq!(vm.executor.get_number("s"), Some(9.0));
    assert_eq!(jit.executor.get_number("s"), Some(9.0));
    reset_jit();
}
