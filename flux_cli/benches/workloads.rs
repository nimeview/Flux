//! Phase 10 baseline: representative workloads on the VM (reference) and the
//! JIT, measured before any optimization. Each workload is a Flux program in
//! `benches/workloads/`; pure ones define `fn run() -> Int`, which is called
//! on an already-initialized executor so only execution is timed.
//!
//! Run: `cargo bench -p flux_cli --bench workloads`. Criterion writes
//! `target/criterion/<group>/<engine>/` for comparisons between runs.

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flux_cli::pipeline::{compile_with_options, source_from, CompileOptions};
use flux_runtime::{ExecutionMode, Executor, RuntimeValue};

const PURE: &[(&str, &str)] = &[
    ("compute_loop", include_str!("workloads/compute_loop.fl")),
    ("recursion", include_str!("workloads/recursion.fl")),
    ("strings", include_str!("workloads/strings.fl")),
    ("collections", include_str!("workloads/collections.fl")),
    ("structs_match", include_str!("workloads/structs_match.fl")),
    ("enum_match", include_str!("workloads/enum_match.fl")),
];
const REACTIVE: &str = include_str!("workloads/reactive_events.fl");
const COMPONENTS: &str = include_str!("workloads/components.fl");
const TIMERS: &str = include_str!("workloads/timers.fl");
const COMPILE_INPUT: &str = include_str!("../../examples/general/inventory_totals.fl");

const ENGINES: [(&str, ExecutionMode); 2] =
    [("vm", ExecutionMode::Vm), ("jit", ExecutionMode::Jit)];

fn executor(name: &str, src: &str, mode: ExecutionMode) -> Executor {
    let options = CompileOptions::run(mode).without_cache();
    compile_with_options(source_from(format!("{name}.fl"), src), options)
        .unwrap_or_else(|f| panic!("{name}: {:?}", f.diagnostics.errors))
        .executor
}

fn pure_workloads(c: &mut Criterion) {
    for (name, src) in PURE {
        let mut group = c.benchmark_group(*name);
        for (engine, mode) in ENGINES {
            let mut ex = executor(name, src, mode);
            let expected = ex.call("run", &[]).expect("workload runs");
            group.bench_function(engine, |b| {
                b.iter(|| {
                    let value = ex.call("run", &[]).expect("workload runs");
                    debug_assert_eq!(value, expected);
                    black_box(value)
                })
            });
        }
        group.finish();
    }
}

fn reactive_events(c: &mut Criterion) {
    let mut group = c.benchmark_group("reactive_events_1000");
    for (engine, mode) in ENGINES {
        let mut ex = executor("reactive_events", REACTIVE, mode);
        group.bench_function(engine, |b| {
            b.iter(|| {
                for n in 0..1000 {
                    ex.emit("Tick", RuntimeValue::int_i64(n % 7));
                }
                black_box(ex.get_value("d5"))
            })
        });
    }
    group.finish();
}

/// 1,000 component instances: making them, then 10 global events that each
/// change every instance and the sum over them.
fn components(c: &mut Criterion) {
    let mut group = c.benchmark_group("components_1000");
    for (engine, mode) in ENGINES {
        group.bench_function(format!("make/{engine}"), |b| {
            b.iter_batched(
                || executor("components", COMPONENTS, mode),
                |mut ex| {
                    ex.emit("Make", RuntimeValue::int_i64(1000));
                    black_box(ex.get_value("total"))
                },
                criterion::BatchSize::LargeInput,
            )
        });
        let mut ex = executor("components", COMPONENTS, mode);
        ex.emit("Make", RuntimeValue::int_i64(1000));
        group.bench_function(format!("tick/{engine}"), |b| {
            b.iter(|| {
                for _ in 0..10 {
                    ex.emit("Tick", RuntimeValue::Unit);
                }
                black_box(ex.get_value("total"))
            })
        });
    }
    group.finish();
}

/// A graph of 300 derived values, each reading two earlier ones (or the
/// source), updated by 100 events.
fn derived_graph(c: &mut Criterion) {
    let mut src = String::from("event Set(v: Int)\n\nlet mut base = 0\n\nlet d0 = base + 1\n");
    for i in 1..300 {
        src.push_str(&format!("let d{i} = d{} + d{} % 7\n", i - 1, i / 2));
    }
    src.push_str("\non Set(v) {\n    base = v\n}\n");
    let mut group = c.benchmark_group("derived_graph_300");
    for (engine, mode) in ENGINES {
        let mut ex = executor("derived_graph", &src, mode);
        group.bench_function(engine, |b| {
            b.iter(|| {
                for n in 0..100 {
                    ex.emit("Set", RuntimeValue::int_i64(n));
                }
                black_box(ex.get_value("d299"))
            })
        });
    }
    group.finish();
}

/// 1,000 timer firings through the event loop, each setting the next.
fn timers(c: &mut Criterion) {
    let mut group = c.benchmark_group("timers_1000");
    for (engine, mode) in ENGINES {
        let mut ex = executor("timers", TIMERS, mode);
        group.bench_function(engine, |b| {
            b.iter(|| {
                ex.emit("Start", RuntimeValue::Unit);
                ex.run_event_loop().expect("runs");
                black_box(ex.get_value("n"))
            })
        });
    }
    group.finish();
}

fn compile_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile_inventory_totals");
    for (engine, mode) in ENGINES {
        group.bench_function(engine, |b| {
            b.iter(|| black_box(executor("inventory_totals", COMPILE_INPUT, mode)))
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(3));
    targets = pure_workloads, reactive_events, components, derived_graph, timers, compile_pipeline
}
criterion_main!(benches);
