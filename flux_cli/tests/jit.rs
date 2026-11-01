//! JIT vs VM parity tests and basic benchmarks.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use std::sync::{Mutex, MutexGuard, OnceLock};

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, RuntimeValue};

fn jit_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
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
    // Locals are Cranelift variables, so match bindings compile natively.
    assert!(
        !stats.fallback_nodes.iter().any(|n| n == "code"),
        "enum match should be JIT-compiled, fallbacks: {:?}",
        stats.fallback_nodes
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

#[test]
fn jit_parity_locals_reassigned_in_loops_and_branches() {
    // Locals used to be plain SSA values: a loop header never saw the body's
    // reassignment, so `while i < n { i = i + 1 }` hung under the JIT.
    let _guard = jit_test_lock();
    reset_jit();
    let src = "\
fn count(n: Int) -> Int {\n    let mut i = 0\n    while i < n {\n        i = i + 1\n    }\n    i\n}\n\
fn pick(c: Int) -> Int {\n    let mut x = 1\n    if c > 0 {\n        x = 2\n    }\n    x\n}\n\
fn evens(n: Int) -> Int {\n    let mut i = 0\n    let mut acc = 0\n    while i < n {\n        let sq = i * i\n        if i % 2 == 0 {\n            acc = acc + sq\n        } else {\n            acc = acc - 1\n        }\n        i = i + 1\n    }\n    acc\n}\n\
fn grid(n: Int) -> Int {\n    let mut total = 0\n    let mut r = 0\n    while r < n {\n        let mut c = 0\n        while c < n {\n            total = total + r * c\n            c = c + 1\n        }\n        r = r + 1\n    }\n    total\n}\n\
fn sum_for(xs: [Int]) -> Int {\n    let mut t = 0\n    for x in xs {\n        t = t + x\n    }\n    t\n}\n\
let a = count(5)\nlet b = pick(3)\nlet c = pick(0)\nlet d = evens(10)\nlet e = grid(4)\nlet f = sum_for([1, 2, 3, 4])\n\
derived all = a + b + c + d + e + f\n";
    let source = source_from("<jit-locals>", src);
    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm)
        .unwrap_or_else(|e| panic!("VM compile failed: {:?}", e.diagnostics.errors));
    let jit = compile_with_mode(source, ExecutionMode::Jit)
        .unwrap_or_else(|e| panic!("JIT compile failed: {:?}", e.diagnostics.errors));
    for (name, want) in [
        ("a", 5),
        ("b", 2),
        ("c", 1),
        ("d", 115),
        ("e", 36),
        ("f", 10),
    ] {
        let want = Some(RuntimeValue::int_i64(want));
        assert_eq!(vm.executor.get_value(name), want, "VM `{name}`");
        assert_eq!(jit.executor.get_value(name), want, "JIT `{name}`");
    }
    let stats = jit.executor.jit_stats().expect("jit stats");
    for f in ["count", "pick", "evens", "grid"] {
        assert!(
            !stats.fallback_nodes.iter().any(|n| n == f),
            "`{f}` should be JIT-compiled: {stats:?}"
        );
    }
}

#[test]
fn jit_parity_call_does_not_clobber_handler_param() {
    // Calls pass arguments through shared slots; the handler's own parameter
    // must survive a call made before it is read.
    let _guard = jit_test_lock();
    reset_jit();
    let src = "fn sq(v: Int) -> Int { v * v }\nevent Add(x: Int)\nlet mut total = 0\non Add { x ->\n    total = sq(3) + x\n}\nderived t = total\n";
    for mode in [ExecutionMode::Vm, ExecutionMode::Jit] {
        let mut compiled = compile_with_mode(source_from("<param>", src), mode)
            .unwrap_or_else(|e| panic!("{mode:?} compile failed: {:?}", e.diagnostics.errors));
        eprintln!("mode {mode:?} compiled");
        compiled.executor.emit("Add", RuntimeValue::int_i64(10));
        eprintln!("mode {mode:?} emitted");
        assert_eq!(
            compiled.executor.get_value("total"),
            Some(RuntimeValue::int_i64(19)),
            "{mode:?}"
        );
    }
}

#[test]
fn jit_parity_calls_recursion_and_early_return() {
    // `return` used to fall through to the next statement under the JIT, and
    // a call between JIT functions overwrote the caller's parameters.
    let _guard = jit_test_lock();
    reset_jit();
    let src = "\
fn sign(x: Int) -> Int {\n    if x > 0 {\n        return 1\n    }\n    if x < 0 {\n        return -1\n    }\n    0\n}\n\
fn sq(v: Int) -> Int { v * v }\n\
fn after_call(a: Int, b: Int) -> Int { sq(7) + a * 10 + b }\n\
fn fib(n: Int) -> Int {\n    if n < 2 {\n        n\n    } else {\n        fib(n - 1) + fib(n - 2)\n    }\n}\n\
fn first_square_over(limit: Int) -> Int {\n    let mut i = 0\n    while i < 100 {\n        if sq(i) > limit {\n            return i\n        }\n        i = i + 1\n    }\n    -1\n}\n\
let a = sign(5)\nlet b = sign(-3)\nlet c = sign(0)\nlet d = after_call(2, 3)\nlet e = fib(15)\nlet f = first_square_over(50)\n\
derived all = a + b + c + d + e + f\n";
    let source = source_from("<jit-calls>", src);
    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm)
        .unwrap_or_else(|e| panic!("VM compile failed: {:?}", e.diagnostics.errors));
    let jit = compile_with_mode(source, ExecutionMode::Jit)
        .unwrap_or_else(|e| panic!("JIT compile failed: {:?}", e.diagnostics.errors));
    for (name, want) in [
        ("a", 1),
        ("b", -1),
        ("c", 0),
        ("d", 72),
        ("e", 610),
        ("f", 8),
    ] {
        let want = Some(RuntimeValue::int_i64(want));
        assert_eq!(vm.executor.get_value(name), want, "VM `{name}`");
        assert_eq!(jit.executor.get_value(name), want, "JIT `{name}`");
    }
    let stats = jit.executor.jit_stats().expect("jit stats");
    for f in ["sign", "after_call", "fib", "first_square_over"] {
        assert!(
            !stats.fallback_nodes.iter().any(|n| n == f),
            "`{f}` should be JIT-compiled: {stats:?}"
        );
    }
}

#[test]
fn jit_parity_values_without_a_tagged_form_run_on_the_vm() {
    // Structs, `Option`, and `Result` have no tagged form; compiled code used
    // to receive them as `()` and return `()` (e.g. `pass(Some(3))`).
    let _guard = jit_test_lock();
    reset_jit();
    let src = "import option { Some, None }\nimport result { Ok, Err }\nimport collections { len }\n\
struct P { x: Int }\nenum Box { Full(P), Empty }\n\
fn pass(o: Option[Int]) -> Option[Int] { o }\n\
fn passr(r: Result[Int, String]) -> Result[Int, String] { r }\n\
fn passp(p: P) -> P { p }\n\
fn pick(c: Int, a: Option[Int], b: Option[Int]) -> Option[Int] { if c > 0 { a } else { b } }\n\
fn count(xs: [Option[Int]]) -> Int { len(xs) }\n\
fn keep(b: Box) -> Box { b }\n\
event Put(p: P)\nlet mut last = P { x: 0 }\non Put { p ->\n    last = p\n}\n\
let mut maybe = Some(5)\nderived same = maybe\n\
let r1 = pass(Some(3))\nlet r2 = passr(Err(\"bad\"))\nlet r3 = passp(P { x: 7 })\nlet r4 = pick(1, Some(1), None)\nlet r5 = count([Some(1), None])\nlet r6 = keep(Full(P { x: 2 }))\n";
    let source = source_from("<untagged>", src);
    let mut vm = compile_with_mode(source.clone(), ExecutionMode::Vm)
        .unwrap_or_else(|e| panic!("VM compile failed: {:?}", e.diagnostics.errors));
    let mut jit = compile_with_mode(source, ExecutionMode::Jit)
        .unwrap_or_else(|e| panic!("JIT compile failed: {:?}", e.diagnostics.errors));
    let payload = vm.executor.get_value("r3").expect("struct value");
    vm.executor.emit("Put", payload.clone());
    jit.executor.emit("Put", payload);
    for name in ["r1", "r2", "r3", "r4", "r5", "r6", "last", "same"] {
        let want = vm.executor.get_value(name);
        assert!(
            !matches!(want, None | Some(RuntimeValue::Unit)),
            "VM `{name}`: {want:?}"
        );
        assert_eq!(jit.executor.get_value(name), want, "JIT `{name}`");
    }
}
