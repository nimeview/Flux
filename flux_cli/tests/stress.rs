//! Stress tests — large graphs, rapid updates, deep chains.

use flux_cli::pipeline::{compile, source_from};
use flux_runtime::RuntimeValue;

fn compile_src(name: &str, src: &str) -> flux_cli::pipeline::Compiled {
    compile(source_from(name, src))
        .unwrap_or_else(|e| panic!("{name} should compile: {:?}", e.diagnostics.errors))
}

#[test]
fn stress_deep_derived_chain_100() {
    let n = 100;
    let mut src = String::from("let a0 = 1\n");
    for i in 1..n {
        src.push_str(&format!("let a{i} = a{} + 1\n", i - 1));
    }
    let compiled = compile_src("chain.fl", &src);
    assert_eq!(
        compiled.executor.get_number("a99"),
        Some(100.0),
        "chain tip should equal length"
    );
}

#[test]
fn stress_wide_fanout_200() {
    let mut src = String::from("let mut root = 10\n");
    for i in 0..200 {
        src.push_str(&format!("let d{i} = root + {i}\n"));
    }
    let mut compiled = compile_src("fanout.fl", &src);
    assert_eq!(compiled.executor.get_number("d0"), Some(10.0));
    assert_eq!(compiled.executor.get_number("d199"), Some(209.0));

    compiled
        .executor
        .set_source("root", RuntimeValue::int_i64(0));
    assert_eq!(compiled.executor.get_number("d199"), Some(199.0));
}

#[test]
fn stress_diamond_dependency() {
    let src = r#"
let mut base = 1
let left = base + 10
let right = base + 20
let top = left + right
"#;
    let mut compiled = compile_src("diamond.fl", src);
    assert_eq!(compiled.executor.get_number("top"), Some(32.0));
    compiled
        .executor
        .set_source("base", RuntimeValue::int_i64(5));
    assert_eq!(compiled.executor.get_number("top"), Some(40.0));
}

#[test]
fn stress_rapid_source_updates_1000() {
    let mut compiled = compile_src("pulse.fl", "let mut x = 0\nlet y = x * 2\nlet z = y + x\n");
    for i in 1..=1000 {
        compiled.executor.set_source("x", RuntimeValue::int_i64(i));
    }
    assert_eq!(compiled.executor.get_number("x"), Some(1000.0));
    assert_eq!(compiled.executor.get_number("y"), Some(2000.0));
    assert_eq!(compiled.executor.get_number("z"), Some(3000.0));
}

#[test]
fn stress_many_watches_50() {
    let mut src = String::from("let mut seed = 1\n");
    for i in 0..50 {
        src.push_str(&format!("let mut side{i} = 0\n"));
        src.push_str(&format!("watch seed {{\n  side{i} = seed + {i}\n}}\n"));
    }
    let mut compiled = compile_src("watches.fl", &src);
    assert_eq!(compiled.executor.get_number("side0"), Some(1.0));
    assert_eq!(compiled.executor.get_number("side49"), Some(50.0));

    compiled
        .executor
        .set_source("seed", RuntimeValue::int_i64(10));
    assert_eq!(compiled.executor.get_number("side0"), Some(10.0));
    assert_eq!(compiled.executor.get_number("side49"), Some(59.0));
}

#[test]
fn stress_large_for_loop_in_watch() {
    let src = r#"
import collections
let mut total = 0

watch {
    total = 0
    for i in range(2000) {
        total = total + 1
    }
}
"#;
    let compiled = compile_src("loop.fl", src);
    assert_eq!(compiled.executor.get_number("total"), Some(2000.0));
}

#[test]
fn stress_nested_loops_and_conditionals() {
    let src = r#"
import collections
let mut acc = 0

watch {
    acc = 0
    for outer in range(20) {
        let mut row = 0
        for inner in range(10) {
            row = row + 1
        }
        if row == 10 {
            acc = acc + outer
        }
    }
}
"#;
    let compiled = compile_src("nested.fl", src);
    // sum of outer for outer in 0..19 where inner loop runs 10 times = 0+1+...+19 = 190
    assert_eq!(compiled.executor.get_number("acc"), Some(190.0));
}

#[test]
fn stress_many_functions_and_calls() {
    let mut src = String::from("let input = 1\n");
    for i in 0..30 {
        src.push_str(&format!("fn f{i}(x: Int) -> Int {{ x + {i} }}\n"));
    }
    src.push_str("let step0 = input\n");
    for i in 0..30 {
        src.push_str(&format!("let step{} = f{i}(step{i})\n", i + 1));
    }
    let compiled = compile_src("fn_chain.fl", &src);
    // 1 + 0 + 1 + 2 + ... + 29 = 1 + 29*30/2 = 436
    assert_eq!(compiled.executor.get_number("step30"), Some(436.0));
}

#[test]
fn stress_event_burst_500() {
    let src = r#"
event Tick
let mut n = 0
on Tick { n = n + 1 }
let derived = n * 2
"#;
    let mut compiled = compile_src("burst.fl", src);
    for _ in 0..500 {
        compiled.executor.emit("Tick", RuntimeValue::Unit);
    }
    assert_eq!(compiled.executor.get_number("n"), Some(500.0));
    assert_eq!(compiled.executor.get_number("derived"), Some(1000.0));
}

#[test]
fn stress_boolean_expression_chain() {
    let src = r#"
let a = 1
let b = 2
let c = 3
let ok = a < b && b < c && !(a == c) || false && (10 / 0 > 1)
let score = if ok { 42 } else { 0 }
"#;
    let compiled = compile_src("bool.fl", src);
    assert_eq!(
        compiled.executor.get_value("ok"),
        Some(RuntimeValue::Bool(true))
    );
    assert_eq!(compiled.executor.get_number("score"), Some(42.0));
}

#[test]
fn stress_many_bindings_compile_time() {
    let mut src = String::new();
    for i in 0..300 {
        src.push_str(&format!("let v{i} = {i}\n"));
    }
    src.push_str("let sum = v0 + v1 + v2 + v3 + v4\n");
    let compiled = compile_src("many_bindings.fl", &src);
    assert_eq!(compiled.executor.get_number("v299"), Some(299.0));
    assert_eq!(compiled.executor.get_number("sum"), Some(10.0));
}

#[test]
fn stress_rfn_reactive_web_20_nodes() {
    let mut src = String::from("let mut hub = 1\n");
    for i in 0..20 {
        src.push_str(&format!(
            "rfn scale{i}(x: Int) -> Int {{ x + hub + {i} }}\n"
        ));
        src.push_str(&format!("let out{i} = scale{i}(hub)\n"));
    }
    let mut compiled = compile_src("rfn_web.fl", &src);
    assert_eq!(compiled.executor.get_number("out0"), Some(2.0));
    assert_eq!(compiled.executor.get_number("out19"), Some(21.0));

    compiled
        .executor
        .set_source("hub", RuntimeValue::int_i64(10));
    assert_eq!(compiled.executor.get_number("out0"), Some(20.0));
    assert_eq!(compiled.executor.get_number("out19"), Some(39.0));
}

#[test]
fn stress_repeated_full_pipeline() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../programs/demo/dashboard.fl"),
    )
    .expect("dashboard.fl");

    for round in 0..50 {
        let name = format!("round{round}.fl");
        compile_src(&name, &src).executor.get_number("health");
    }
}
