//! Cross-stage contracts: symbol resolution and type checking must agree with VM execution.

use flux_cli::pipeline::{compile, source_from};
use flux_diagnostics::FluxError;
use flux_driver::check_file;
use flux_runtime::RuntimeValue;

fn run_value(source: &str, name: &str) -> RuntimeValue {
    let compiled = compile(source_from("<core-contract>", source))
        .unwrap_or_else(|failure| panic!("compilation failed: {:?}", failure.diagnostics.errors));
    compiled.executor.get_value(name).expect("binding value")
}

#[test]
fn user_function_can_have_builtin_name() {
    let value = run_value("fn abs(x: Int) -> Int { x + 1 }\nlet n = abs(1)\n", "n");
    assert_eq!(value, RuntimeValue::int_i64(2));
}

#[test]
fn user_function_named_print_remains_pure() {
    let value = run_value(
        "fn print(x: Int) -> Int { x + 1 }\nlet mut input = 1\nlet result = print(input)\n",
        "result",
    );
    assert_eq!(value, RuntimeValue::int_i64(2));
}

#[test]
fn generic_specialization_cannot_capture_user_function() {
    let value = run_value(
        "fn id[T](x: T) -> T { x }\nfn id_Int(x: Int) -> Int { x + 1 }\nlet n = id(1)\n",
        "n",
    );
    assert_eq!(value, RuntimeValue::int_i64(1));
}

#[test]
fn cast_to_unrelated_type_is_rejected() {
    let failure = match compile(source_from("<invalid-cast>", "let x = true as String\n")) {
        Ok(_) => panic!("invalid cast compiled"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|error| matches!(error, FluxError::TypeMismatch { .. })));
}

#[test]
fn builtin_rejects_wrong_argument_type_before_execution() {
    let directory = tempfile::tempdir().expect("temporary project");
    let source = directory.path().join("main.fl");
    std::fs::write(&source, "import math { abs }\nlet n = abs(\"hello\")\n").expect("write source");
    let failure = match check_file(&source) {
        Ok(_) => panic!("invalid builtin call passed type checking"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|error| matches!(error, FluxError::TypeMismatch { .. })));
}

#[test]
fn derived_binding_rejects_transitive_io_effect() {
    let directory = tempfile::tempdir().expect("temporary project");
    let source = directory.path().join("main.fl");
    std::fs::write(
        &source,
        "import io { println }\nlet mut input = 1\nfn side() -> Int { println(\"effect\")\n  2 }\nfn indirect() -> Int { side() }\nlet result = input + indirect()\n",
    )
    .expect("write source");
    let failure = match check_file(&source) {
        Ok(_) => panic!("side effect in derived value passed type checking"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|error| matches!(error, FluxError::SideEffectInDerived { .. })));
}

#[test]
fn forward_binding_uses_its_inferred_type() {
    let value = run_value("let a = b + \"x\"\nlet b = \"y\"\n", "a");
    assert_eq!(value, RuntimeValue::String("yx".into()));
}

#[test]
fn empty_string_source_is_readable_in_handlers() {
    // `""` used to parse as an interpolation with no parts, which left the
    // source untyped inside `watch`/`on` bodies ("undefined name symbol #…").
    let src = "event Tick\nlet mut s = \"\"\non Tick {\n  s = s + \"x\"\n}\n";
    let mut compiled = compile(source_from("<empty-string>", src))
        .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
    compiled.executor.emit("Tick", RuntimeValue::Unit);
    compiled.executor.emit("Tick", RuntimeValue::Unit);
    assert_eq!(
        compiled.executor.get_value("s"),
        Some(RuntimeValue::String("xx".into()))
    );
}

#[test]
fn continue_skips_iteration_on_vm_and_jit() {
    // JIT may fall back to the VM here; either way the value must match.
    let src = "import collections\nfn f(n: Int) -> Int {\n  let mut acc = 0\n  for i in range(n) {\n    if i % 2 == 0 { continue }\n    acc = acc + i\n  }\n  acc\n}\nlet mut a = 10\nlet r = f(a)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<continue>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("r"),
            Some(RuntimeValue::int_i64(25)),
            "{mode:?}"
        );
    }
}
