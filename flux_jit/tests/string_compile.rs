use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_jit::JitEngine;
use flux_runtime::ExecutionMode;
use std::fs;

#[test]
fn string_concat_compiles_in_jit() {
    let contents = fs::read_to_string("../programs/ok/stdlib/string_concat.fl").unwrap();
    let compiled = compile_with_mode(
        source_from("string_concat.fl", contents),
        ExecutionMode::Jit,
    )
    .unwrap();

    let msg = compiled
        .executor
        .program
        .nodes
        .iter()
        .find(|n| n.name == "msg")
        .expect("msg node");
    eprintln!("msg code: {:?}", msg.code);

    let engine = JitEngine::new(&compiled.executor.program);
    let stats = engine.stats();
    eprintln!("engine stats: {stats:?}");
    assert!(
        engine.exprs.contains_key("msg"),
        "msg should compile, stats: {stats:?}"
    );
}
