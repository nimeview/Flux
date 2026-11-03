//! Compile-and-run regressions for monomorphization tree traversal.

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, RuntimeValue};

fn assert_result(body: &str, expected: i64) {
    let source = format!("fn id[T](x: T) -> T {{ x }}\n{body}");
    let compiled = compile_with_mode(source_from("<mono-walkers>", &source), ExecutionMode::Vm)
        .unwrap_or_else(|failure| panic!("{:?}\n{source}", failure.diagnostics.errors));
    assert_eq!(
        compiled.executor.get_value("result"),
        Some(RuntimeValue::int_i64(expected))
    );
}

#[test]
fn generic_calls_in_full_else_if_chains_compile_and_run() {
    // Both discovery of a new specialization and rewriting an existing one.
    for warmup in ["", "let warmup = id(99)\n"] {
        assert_result(
            &format!(
                r#"
{warmup}
fn f() -> Int {{
    if false {{ return 0 }}
    else if false {{ return 1 }}
    else if false {{ return 2 }}
    else {{ return id(42) }}
}}
let result = f()
"#
            ),
            42,
        );
        assert_result(
            &format!(
                r#"
{warmup}
let mut result = 0
if false {{ result = 1 }}
else if false {{ result = 2 }}
else if false {{ result = 3 }}
else {{ result = id(42) }}
"#
            ),
            42,
        );
    }
}

#[test]
fn generic_arguments_use_locals_in_nested_match_bodies() {
    assert_result(
        r#"
fn f() -> Int {
    match 0 {
        _ => {
            let x = 1
            id(x)
        }
    }
}
let result = f()
"#,
        1,
    );
    assert_result(
        r#"
fn f() -> Int {
    let a = [match 0 { _ => {
        let x = 4
        match x { _ => { let y = x + 3
            id(y) } }
    } }]
    a[0]
}
let result = f()
"#,
        7,
    );
}

#[test]
fn generic_arguments_use_locals_in_init_and_else_if_tails() {
    assert_result(
        r#"
let mut result = 0
if false { result = 1 }
else if false { result = 2 }
else { let x = 42
    result = id(x) }
"#,
        42,
    );
    assert_result(
        r#"
fn f() -> Int {
    if false { return 0 }
    else if false { return 1 }
    else { let x = 42
        return id(x) }
}
let result = f()
"#,
        42,
    );
}

#[test]
fn generic_arguments_use_match_pattern_bindings() {
    for (declarations, scrutinee, arms) in [
        ("", "42", "n => id(n)"),
        (
            "import option { Some, None }",
            "Some(42)",
            "Some(n) => id(n), None => 0",
        ),
        (
            "import result { Ok, Err }",
            "Ok(42)",
            "Ok(n) => id(n), Err(_) => 0",
        ),
        (
            "import result { Ok, Err }",
            "Err(42)",
            "Ok(_) => 0, Err(n) => id(n)",
        ),
        (
            "enum E { Value(Int), Empty }",
            "Value(42)",
            "Value(n) => id(n), Empty => 0",
        ),
        (
            "import option { Some, None }\nenum E { Value(Int), Empty }",
            "Some(Value(42))",
            "Some(Value(n)) => id(n), Some(Empty) => 0, None => 0",
        ),
    ] {
        assert_result(
            &format!("{declarations}\nlet result = match {scrutinee} {{ {arms} }}\n"),
            42,
        );
    }
}

#[test]
fn generic_arguments_use_struct_variant_field_types() {
    // The typed parameter exercises field binding without requiring a
    // struct-variant constructor in expression syntax.
    assert_result(
        r#"
enum E { Value { n: Int }, Empty }
fn read(e: E) -> Int {
    match e { Value { n } => id(n), Empty => 42 }
}
let result = read(Empty)
"#,
        42,
    );
}
