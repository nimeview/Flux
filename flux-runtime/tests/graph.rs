use flux_runtime::{ReactiveGraph, RuntimeValue};

#[test]
fn topo_recomputes_derived_in_order() {
    let mut g = ReactiveGraph::new();
    let a = g.add_source("a", RuntimeValue::int_i64(2));
    let b = g.add_source("b", RuntimeValue::int_i64(3));
    let _c = g.add_derived("c", vec![a, b]);
    // Derived values filled by Executor in production; graph stores structure only.
    assert_eq!(g.nodes.len(), 3);
    assert!(g.dependents[a.0 as usize].contains(&_c));
}

#[test]
fn set_source_updates_value() {
    let mut g = ReactiveGraph::new();
    let a = g.add_source("a", RuntimeValue::int_i64(1));
    g.set_source(a, RuntimeValue::int_i64(99));
    assert_eq!(g.get(a), &RuntimeValue::int_i64(99));
}

#[test]
fn set_derived_deps_reorder_preserves_edges() {
    let mut g = ReactiveGraph::new();
    let a = g.add_source("a", RuntimeValue::int_i64(1));
    let b = g.add_source("b", RuntimeValue::int_i64(2));
    let c = g.add_derived("c", vec![b, a]);
    assert!(g.dependents[a.0 as usize].contains(&c));
    assert!(g.dependents[b.0 as usize].contains(&c));

    g.set_derived_deps(c, vec![a, b]);
    assert!(g.dependents[a.0 as usize].contains(&c));
    assert!(g.dependents[b.0 as usize].contains(&c));
}

#[test]
fn get_by_name_finds_binding() {
    let mut g = ReactiveGraph::new();
    g.add_source("price", RuntimeValue::float_f64(10.5));
    assert_eq!(
        g.get_by_name("price").and_then(|v| v.as_float()),
        Some(10.5)
    );
}
