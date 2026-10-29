use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, RuntimeValue};

const COUNTER: &str = r#"
event Click
let mut count = 0
let doubled = count * 2
on Click { count = count + 1 }
"#;

fn bench_vm_counter_emit(c: &mut Criterion) {
    let source = source_from("counter.fl", COUNTER);
    let mut compiled = compile_with_mode(source, ExecutionMode::Vm).expect("compile");
    c.bench_function("exec_vm_counter_emit_1000", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                compiled.executor.emit("Click", RuntimeValue::Unit);
            }
            black_box(compiled.executor.get_number("count"));
        });
    });
}

fn bench_jit_counter_emit(c: &mut Criterion) {
    let source = source_from("counter.fl", COUNTER);
    let mut compiled = compile_with_mode(source, ExecutionMode::Jit).expect("compile");
    c.bench_function("exec_jit_counter_emit_1000", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                compiled.executor.emit("Click", RuntimeValue::Unit);
            }
            black_box(compiled.executor.get_number("count"));
        });
    });
}

fn bench_vm_compile_arithmetic(c: &mut Criterion) {
    let src = include_str!("../../programs/ok/basics/arithmetic.fl");
    c.bench_function("compile_vm_arithmetic", |b| {
        b.iter(|| {
            let source = source_from("arithmetic.fl", black_box(src));
            let compiled = compile_with_mode(source, ExecutionMode::Vm).expect("compile");
            black_box(compiled.executor.get_number("sum"));
        });
    });
}

fn bench_jit_compile_arithmetic(c: &mut Criterion) {
    let src = include_str!("../../programs/ok/basics/arithmetic.fl");
    c.bench_function("compile_jit_arithmetic", |b| {
        b.iter(|| {
            let source = source_from("arithmetic.fl", black_box(src));
            let compiled = compile_with_mode(source, ExecutionMode::Jit).expect("compile");
            black_box(compiled.executor.get_number("sum"));
        });
    });
}

criterion_group!(
    benches,
    bench_vm_counter_emit,
    bench_jit_counter_emit,
    bench_vm_compile_arithmetic,
    bench_jit_compile_arithmetic
);
criterion_main!(benches);
