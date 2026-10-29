use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, RuntimeValue};

#[test]
fn int_add_preserves_int_tag_in_jit() {
    let src = "let c = 1 + 2\n";
    let source = source_from("parity.fl", src.to_string());
    let vm = compile_with_mode(source.clone(), ExecutionMode::Vm).unwrap();
    let jit = compile_with_mode(source, ExecutionMode::Jit).unwrap();
    assert_eq!(vm.executor.get_value("c"), Some(RuntimeValue::int_i64(3)));
    assert_eq!(jit.executor.get_value("c"), Some(RuntimeValue::int_i64(3)));
}
