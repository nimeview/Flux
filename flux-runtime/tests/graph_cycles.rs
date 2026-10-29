use flux_runtime::{ReactiveGraph, RuntimeValue};

#[test]
fn detects_dependency_cycle() {
    let mut g = ReactiveGraph::new();
    let a = g.add_source("a", RuntimeValue::int_i64(0));
    let b = g.add_derived("b", vec![a]);
    let c = g.add_derived("c", vec![b]);
    assert!(g.is_acyclic());

    g.set_derived_deps(b, vec![c]);
    assert!(!g.is_acyclic());
}
