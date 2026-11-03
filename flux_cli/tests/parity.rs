//! VM/JIT parity beyond numbers. The VM is the reference: each case must give
//! the same value (including its width) or the same error on both engines. A
//! case marked native must really run as compiled code, so that a fallback to
//! the VM cannot make the comparison pass by itself.

use std::sync::{Mutex, MutexGuard, OnceLock};

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, RuntimeValue};

fn jit_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

const PRELUDE: &str = "import collections { len, range, get }\n\
import math { abs, min, max, clamp, sqrt, floor, ceil, pow }\n\
import str { str_len, str_contains, str_starts_with, str_ends_with, str_to_lower, str_to_upper, to_string }\n\
import option { Some, None }\n\
enum Color {\n    Red,\n    Named(String),\n    Level(Int),\n    Wrap(Color)\n}\n\
fn inc(n: Int) -> Int {\n    n + 1\n}\n\
fn fact(n: Int) -> Int {\n    if n <= 1 {\n        1\n    } else {\n        n * fact(n - 1)\n    }\n}\n\
fn forever(n: Int) -> Int {\n    forever(n + 1) + 1\n}\n\
fn level(c: Color) -> Int {\n    match c {\n        Level(n) => n,\n        Wrap(inner) => level(inner) + 100,\n        _ => 0\n    }\n}\n";

/// Outcome of `t()` on one engine, and whether `t` ran natively.
fn outcome(src: &str, args: &[RuntimeValue], mode: ExecutionMode) -> (String, bool) {
    let mut compiled = match compile_with_mode(source_from("<parity>", src), mode) {
        Ok(compiled) => compiled,
        Err(failed) => return (format!("compile: {:?}", failed.diagnostics.errors), false),
    };
    let native = compiled
        .executor
        .jit_stats()
        .is_some_and(|stats| !stats.fallback_nodes.iter().any(|n| n == "t"));
    let result = compiled.executor.call("t", args);
    (
        match result {
            Ok(value) => format!("{value:?}"),
            Err(err) => format!("error: {err}"),
        },
        native,
    )
}

fn check(cases: &[(&str, bool)]) {
    let _guard = jit_test_lock();
    let mut failures = Vec::new();
    for (body, want_native) in cases {
        let src = format!("{PRELUDE}fn t() {{\n    {body}\n}}\n");
        flux_jit::reset_host_for_tests();
        let (vm, _) = outcome(&src, &[], ExecutionMode::Vm);
        flux_jit::reset_host_for_tests();
        let (jit, native) = outcome(&src, &[], ExecutionMode::Jit);
        if vm.starts_with("compile:") {
            failures.push(format!("{body}\n  does not compile: {vm}"));
        } else if vm != jit {
            failures.push(format!("{body}\n  vm:  {vm}\n  jit: {jit}"));
        } else if native != *want_native {
            failures.push(format!(
                "{body}\n  native = {native}, expected {want_native} ({vm})"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn integers_of_every_width() {
    check(&[
        ("200u8 + 55u8", true),
        ("200u8 + 100u8", true),
        ("0u8 - 1u8", true),
        ("100i8 + 27i8", true),
        ("100i8 + 28i8", true),
        ("-128i8 / -1i8", true),
        ("-128i8 % -1i8", true),
        ("2147483647i32 + 1i32", true),
        ("1i32 + 2i64", true),
        ("1u8 + 1i32", true),
        ("18446744073709551615u64", true),
        ("18446744073709551615u64 + 1u64", true),
        ("9223372036854775807 + 1", true),
        ("-9223372036854775808 / -1", true),
        ("-9223372036854775808 % -1", true),
        ("9223372036854775807 * 2", true),
        ("5 % -3", true),
        ("-5 / 2", true),
        ("-5 % 2", true),
        ("1 / 0", true),
        ("1 % 0", true),
        ("-(1u8 as Int)", true),
        ("-(-9223372036854775807 - 1)", true),
        ("1u8 < 2i32", true),
        ("255u8 == 255i32", true),
        ("300 as UInt8", true),
        ("-1 as UInt64", true),
        ("255u8 as Int8", true),
        ("255u8 as Int", true),
        ("127 as Int8", true),
    ]);
}

#[test]
fn floats() {
    check(&[
        ("1.5 + 2.25", true),
        ("1.0 / 0.0", true),
        ("-1.0 / 0.0", true),
        ("0.0 / 0.0", true),
        ("5.5 % 2.0", true),
        ("-5.5 % 2.0", true),
        ("1.0 % 0.0", true),
        ("100000000000000000000.0 * 100000000000000000000.0", true),
        ("0.1 + 0.2", true),
        ("0.1f32 + 0.2f32", true),
        ("1.5f32 + 1.5", true),
        ("1 + 2.5", true),
        ("(0.0 / 0.0) == (0.0 / 0.0)", true),
        ("(0.0 / 0.0) != (0.0 / 0.0)", true),
        ("(0.0 / 0.0) < 1.0", true),
        ("(0.0 / 0.0) > 1.0", true),
        ("-0.0 == 0.0", true),
        ("-(0.0)", true),
        ("1 == 1.0", true),
        ("1 < 1.5", true),
        ("9007199254740993 == 9007199254740992.0", true),
        ("2.9 as Int", true),
        ("-2.9 as Int", true),
        ("100000000000000000000.0 as Int", true),
        ("(0.0 / 0.0) as Int", true),
        ("3 as Float", true),
        ("0.1 as Float32", true),
        (
            "100000000000000000000000000000000000000000000.0 as Float32",
            true,
        ),
    ]);
}

#[test]
fn math_builtins() {
    check(&[
        ("abs(-3)", true),
        ("abs(-9223372036854775807 - 1)", true),
        ("abs(-1.5)", true),
        ("min(1, 2)", true),
        ("max(1, 2)", true),
        ("min(1.5, 2.5)", true),
        ("max(0.0 / 0.0, 1.0)", true),
        ("clamp(5, 0, 3)", true),
        ("clamp(-1, 0, 3)", true),
        ("clamp(1.5, 0.0, 1.0)", true),
        ("sqrt(4.0)", true),
        ("sqrt(-1.0)", true),
        ("floor(1.5)", true),
        ("ceil(1.5)", true),
        ("floor(-1.5)", true),
        ("pow(2, 10)", true),
        ("pow(2, 100)", true),
        ("pow(2, -1)", true),
        ("pow(2.0, 0.5)", true),
    ]);
}

#[test]
fn booleans_and_short_circuit() {
    check(&[
        ("!true", true),
        ("true && false", true),
        ("false && (1 / 0 == 1)", true),
        ("true || (1 / 0 == 1)", true),
        ("true && (1 / 0 == 1)", true),
        ("false || (1 / 0 == 1)", true),
        ("true == false", true),
        ("if 1 < 2 { 10 } else { 1 / 0 }", true),
        ("if 1 > 2 { 1 / 0 } else { 20 }", true),
        ("if 1 / 0 == 1 { 1 } else { 2 }", true),
    ]);
}

#[test]
fn strings() {
    check(&[
        ("\"a\" + \"b\"", true),
        ("\"\" + \"\"", true),
        ("\"h\u{e9}llo \" + \"\u{540d}\u{524d} \u{1F389}\"", true),
        ("\"a\" == \"a\"", true),
        ("\"a\" != \"a\"", true),
        ("\"a\" == \"b\"", true),
        ("\"a\" + \"b\" == \"ab\"", true),
        ("str_len(\"h\u{e9}llo\")", true),
        ("str_len(\"\")", true),
        ("len(\"abc\")", true),
        ("str_contains(\"abc\", \"b\")", true),
        ("str_starts_with(\"abc\", \"ab\")", true),
        ("str_ends_with(\"abc\", \"bc\")", true),
        ("str_to_lower(\"AbC\")", true),
        ("str_to_upper(\"h\u{e9}llo\")", true),
        ("let a = \"x\"\n    let b = a + a\n    b + b", true),
    ]);
}

/// Interpolation lowers to `to_string`, so each value form must print alike.
#[test]
fn to_string_and_interpolation() {
    check(&[
        ("\"v={1 + 1}\"", true),
        ("\"{1}{2} and {\"s\"}\"", true),
        ("to_string(9223372036854775807)", true),
        ("to_string(-1)", true),
        ("to_string(255u8)", true),
        ("to_string(1.0)", true),
        ("to_string(0.1 + 0.2)", true),
        ("to_string(1.0 / 0.0)", true),
        ("to_string(-1.0 / 0.0)", true),
        ("to_string(0.0 / 0.0)", true),
        ("to_string(true)", true),
        ("to_string(\"h\u{e9}llo\")", true),
        ("to_string([1, 2, 3])", true),
        ("to_string([\"a\", \"b\"])", true),
        ("to_string([[1.5], []])", true),
        ("to_string(Red)", true),
        ("to_string(Level(3))", true),
        ("to_string(Named(\"x\"))", true),
        ("to_string(Wrap(Wrap(Level(1))))", true),
        ("\"{1 / 0}\"", true),
        (
            "let mut s = \"\"\n    for i in range(0, 3) {\n        s = s + \"{i},\"\n    }\n    s",
            true,
        ),
    ]);
}

#[test]
fn arrays() {
    check(&[
        ("[1, 2][0]", true),
        ("[1, 2][1]", true),
        ("[1, 2][2]", true),
        ("[1, 2][0 - 1]", true),
        ("[[1], [2, 3]][1][1]", true),
        ("[[1], [2, 3]][1]", true),
        ("[1.5, 2.5][1]", true),
        ("[\"a\", \"b\"][1]", true),
        ("[true, false][1]", true),
        ("[1u8, 2u8][1]", true),
        ("[1u8, 2u8][1] + 255u8", true),
        ("len([1, 2, 3])", true),
        ("len([[1], [2]])", true),
        ("len(range(5))", true),
        ("range(3)", true),
        ("range(1, 4)", true),
        ("range(5, 0, -2)", true),
        ("range(0, 3, 0)", true),
        ("range(100000000000)", true),
        ("[1, 2] == [1, 2]", true),
        ("[1, 2] == [1, 3]", true),
        ("[[1], [2]] == [[1], [2]]", true),
        ("[\"a\"] == [\"a\"]", true),
        ("[0.0 / 0.0] == [0.0 / 0.0]", true),
        ("[1, 2]", true),
        (
            "let mut s = 0\n    for x in [1, 2, 3] {\n        s = s + x\n    }\n    s",
            true,
        ),
        (
            "let mut s = \"\"\n    for x in [\"a\", \"b\"] {\n        s = s + x\n    }\n    s",
            true,
        ),
        (
            "let mut s = 0\n    for x in range(0) {\n        s = s + 1\n    }\n    s",
            true,
        ),
        ("let xs = [1, 2]\n    let ys = xs\n    ys[0] + xs[1]", true),
    ]);
}

#[test]
fn enums() {
    check(&[
        ("Red", true),
        ("Named(\"x\")", true),
        ("Level(3)", true),
        ("Wrap(Wrap(Level(3)))", true),
        ("Red == Red", true),
        ("Level(1) == Level(1)", true),
        ("Level(1) == Level(2)", true),
        ("Named(\"a\") == Named(\"a\")", true),
        ("Red == Level(1)", true),
        ("level(Wrap(Wrap(Level(3))))", true),
        ("match Named(\"z\") {\n        Named(s) => s + \"!\",\n        _ => \"\"\n    }", true),
        ("match Level(2) {\n        Level(0) => 1,\n        Level(n) => n * 2,\n        _ => 0\n    }", true),
        ("[Red, Level(1)][1]", true),
        ("match [Red, Level(4)][1] {\n        Level(n) => n,\n        _ => 0\n    }", true),
        ("match Wrap(Level(5)) {\n        Wrap(Level(n)) => n,\n        _ => 0\n    }", true),
        ("match 3 {\n        1 => 10,\n        3 => 30,\n        _ => 0\n    }", true),
        ("match \"b\" {\n        \"a\" => 1,\n        \"b\" => 2,\n        _ => 0\n    }", true),
        ("match true {\n        true => 1,\n        _ => 0\n    }", true),
    ]);
}

#[test]
fn control_flow_and_calls() {
    check(&[
        ("inc(inc(1))", true),
        ("fact(10)", true),
        ("fact(25)", true),
        ("forever(0)", true),
        ("let mut i = 0\n    while i < 5 {\n        i = i + 1\n    }\n    i", true),
        ("let mut i = 0\n    while true {\n        i = i + 1\n        if i == 3 {\n            return i * 2\n        }\n    }\n    0", true),
        ("for x in [1, 2, 3] {\n        if x == 2 {\n            return x * 10\n        }\n    }\n    0", true),
        ("let x = 1\n    if true {\n        let x = 2\n        x + 1\n    } else {\n        x\n    }", true),
        ("let mut x = 1\n    if true {\n        x = 2\n    }\n    x", true),
        ("let mut x = 1\n    if false {\n        x = 2\n    } else {\n        x = 3\n    }\n    x", true),
        ("let mut x = 0\n    for i in range(3) {\n        let y = i * 2\n        x = x + y\n    }\n    x", true),
        ("let mut x = 0\n    let mut i = 0\n    while i < 3 {\n        let y = i\n        if y == 1 {\n            x = x + 10\n        }\n        i = i + 1\n    }\n    x", true),
        ("let mut s = 0\n    for a in [1, 2] {\n        for b in [10, 20] {\n            s = s + a * b\n        }\n    }\n    s", true),
        ("let x = if true { 1 } else { 2 }\n    x", true),
        ("let mut x = 1\n    x = x / 0\n    x", true),
        ("if true {\n        1\n    }\n    2", true),
        // Loop exits and `Option` run on the VM.
        ("let mut i = 0\n    while true {\n        i = i + 1\n        if i == 3 {\n            break\n        }\n    }\n    i", false),
        ("match get([1], 0) {\n        Some(n) => n,\n        None => 0\n    }", false),
    ]);
}

fn int(n: i64) -> RuntimeValue {
    RuntimeValue::int_i64(n)
}

fn text(s: &str) -> RuntimeValue {
    RuntimeValue::String(s.to_string())
}

fn color(variant: &str, payload: Option<RuntimeValue>) -> RuntimeValue {
    RuntimeValue::user_enum("Color".into(), variant.into(), payload)
}

/// Values from the host cross into compiled code and back unchanged.
#[test]
fn arguments_cross_the_engine_boundary_unchanged() {
    let _guard = jit_test_lock();
    let u8_value = RuntimeValue::Int {
        width: flux_value::IntWidth::U8,
        bits: 200,
    };
    let f32_value = RuntimeValue::Float {
        width: flux_value::FloatWidth::F32,
        bits: 1.5f32.to_bits() as u64,
    };
    let cases: Vec<(&str, RuntimeValue)> = vec![
        ("x: Int", int(-7)),
        ("x: Int", int(i64::MIN)),
        ("x: UInt8", u8_value),
        ("x: Float32", f32_value),
        ("x: Float", RuntimeValue::float_f64(f64::NAN)),
        ("x: Float", RuntimeValue::float_f64(-0.0)),
        ("x: Bool", RuntimeValue::Bool(true)),
        ("x: String", text("h\u{e9}llo \u{1F389}")),
        ("x: String", text("")),
        ("x: [Int]", RuntimeValue::array(vec![])),
        ("x: [Int]", RuntimeValue::array(vec![int(1), int(2)])),
        (
            "x: [[String]]",
            RuntimeValue::array(vec![RuntimeValue::array(vec![text("a")])]),
        ),
        ("x: Color", color("Red", None)),
        ("x: Color", color("Named", Some(text("n")))),
        (
            "x: Color",
            color("Wrap", Some(color("Level", Some(int(3))))),
        ),
        (
            "x: [Color]",
            RuntimeValue::array(vec![color("Level", Some(int(1))), color("Red", None)]),
        ),
        // No tagged form: the VM runs these, with the same result.
        (
            "x: Option[Int]",
            RuntimeValue::Option(Some(Box::new(int(1)))),
        ),
    ];
    let mut failures = Vec::new();
    for (param, value) in &cases {
        let src = format!("{PRELUDE}fn t({param}) {{\n    x\n}}\n");
        let args = std::slice::from_ref(value);
        flux_jit::reset_host_for_tests();
        let (vm, _) = outcome(&src, args, ExecutionMode::Vm);
        flux_jit::reset_host_for_tests();
        let (jit, _) = outcome(&src, args, ExecutionMode::Jit);
        if vm.starts_with("compile:") || vm != jit || vm != format!("{value:?}") {
            failures.push(format!("{param} {value:?}\n  vm:  {vm}\n  jit: {jit}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Stdout and status of a whole program on an engine.
fn run_program(path: &std::path::Path, cwd: &std::path::Path, engine: &str) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["run", engine, "--no-cache", "--quiet"])
        .arg(path)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn flux");
    format!(
        "{:?}\n--- stdout\n{}\n--- stderr\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn programs_in(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n != ".flux" && n != "fail")
            {
                programs_in(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "fl") {
            out.push(path);
        }
    }
}

/// Every program in the repository behaves the same on both engines,
/// including the bindings a program without `main` prints.
#[test]
fn every_repository_program_matches_on_both_engines() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in ["programs", "examples"] {
        programs_in(&root.join(dir), &mut files);
    }
    files.sort();
    assert!(files.len() > 80, "only {} programs", files.len());
    let mut failures = Vec::new();
    for file in &files {
        let cwd = file.parent().unwrap();
        let vm = run_program(file, cwd, "--vm");
        let jit = run_program(file, cwd, "--jit");
        if vm != jit {
            failures.push(format!("{}\nVM: {vm}\nJIT: {jit}", file.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
