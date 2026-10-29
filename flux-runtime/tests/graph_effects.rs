use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_ir::{Codegen, IrCodegen};
use flux_runtime::{Executor, HandlerKind, NodeKind, RuntimeValue};
use flux_syntax::parse_program;
use flux_types::typecheck_program;

fn compile(src: &str) -> Executor {
    let mut diags = DiagnosticBag::new();
    let source_id = SourceId(0);
    let ast = parse_program(source_id, src, &mut diags).expect("parse");
    let hir = lower_program(&ast, &mut diags).expect("hir");
    typecheck_program(&hir, source_id, &mut diags).expect("types");
    let ir = IrCodegen.emit(&hir, source_id, &mut diags).expect("ir");
    Executor::new(ir).expect("valid program")
}

#[test]
fn event_node_links_on_effect_in_graph() {
    let ex = compile("event Ping\non Ping { }\n");
    let tick = ex.graph.name_to_id.get("Ping").expect("Ping in graph");
    assert!(matches!(
        ex.graph.nodes[tick.0 as usize].kind,
        NodeKind::Event
    ));
    assert_eq!(ex.graph.effects_for_event(*tick).len(), 1);
    let effect_id = ex.graph.effects_for_event(*tick)[0];
    assert!(matches!(
        ex.graph.nodes[effect_id.0 as usize].kind,
        NodeKind::Effect { .. }
    ));
}

#[test]
fn emit_runs_on_effects_from_graph() {
    let mut ex = compile("event Inc\nlet mut n = 0\non Inc { n = n + 1 }\n");
    ex.emit("Inc", RuntimeValue::Unit);
    assert_eq!(ex.get_number("n"), Some(1.0));
}

#[test]
fn watch_effect_runs_from_graph_node() {
    let ex = compile("let a = 1\nwatch a { }\n");
    assert!(ex.graph.nodes.iter().any(
        |n| matches!(&n.kind, NodeKind::Effect { meta } if meta.handler_kind == HandlerKind::Watch)
    ));
}
