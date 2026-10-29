use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_ir::lower_to_ir;
use flux_syntax::parse_program;

#[test]
fn lowers_fn_call_in_derived() {
    let src = "fn double(x: Int) -> Int {\n  x * 2\n}\nlet a = 3\nlet b = double(a)\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();

    assert_eq!(ir.functions.len(), 1);
    assert_eq!(ir.functions[0].name, "double");
    assert!(!ir.functions[0].body.is_empty());

    let b = ir.nodes.iter().find(|n| n.name == "b").expect("b node");
    assert!(matches!(b.kind, flux_ir::NodeKind::Derived));
    assert!(ir
        .nodes
        .iter()
        .find(|n| n.name == "b")
        .unwrap()
        .code
        .iter()
        .any(|i| matches!(i, flux_ir::Inst::Call { .. })));
}

#[test]
fn lowers_short_circuit_and() {
    let src = "let ok = if false && (10 / a > 1) { 0 } else { 1 }\nlet mut a = 0\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();
    let ok = ir.nodes.iter().find(|n| n.name == "ok").expect("ok node");
    assert!(
        ok.code
            .iter()
            .any(|i| matches!(i, flux_ir::Inst::ShortAnd(_))),
        "expected ShortAnd in ok binding, got {ok:?}"
    );
}

#[test]
fn folds_const_binding_to_source_in_ir() {
    let src = "let two = 1 + 1\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();
    let two = ir.nodes.iter().find(|n| n.name == "two").expect("two node");
    assert!(matches!(two.kind, flux_ir::NodeKind::Source));
    assert!(matches!(two.init, Some(flux_ir::IrValue::Int(2))));
    assert!(two.code.is_empty());
}

#[test]
fn rejects_cyclic_dependencies() {
    let src = "let a = b + 1\nlet b = a + 1\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    assert!(lower_to_ir(&hir, SourceId(0), &mut diags).is_none());
    assert!(diags
        .errors
        .iter()
        .any(|e| matches!(e, flux_diagnostics::FluxError::CyclicDependency { .. })));
}

#[test]
fn lowers_event_and_effect_graph_nodes() {
    let src = "event Tick\nlet mut n = 0\non Tick {\n  n = n + 1\n}\nwatch n {\n  n = n\n}\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();

    let tick = ir.event_node("Tick").expect("Tick event node");
    assert!(matches!(tick.kind, flux_ir::NodeKind::Event));
    let tick_id = ir
        .nodes
        .iter()
        .position(|n| std::ptr::eq(n, tick))
        .map(|i| flux_ir::NodeId(i as u32))
        .expect("tick id");

    let effects: Vec<_> = ir.effect_nodes().collect();
    assert_eq!(effects.len(), 2);
    assert!(effects.iter().any(|n| {
        n.effect.as_ref().is_some_and(|m| {
            m.handler_kind == flux_ir::HandlerKind::On
                && matches!(&m.trigger, flux_ir::EffectTrigger::OnEvent(e) if e == "Tick")
        })
    }));
    assert!(effects.iter().any(|n| {
        n.effect.as_ref().is_some_and(|m| {
            m.handler_kind == flux_ir::HandlerKind::Watch
                && matches!(&m.trigger, flux_ir::EffectTrigger::Watch { target: Some(t) } if t == "n")
        })
    }));

    let on_effect = effects
        .iter()
        .find(|n| n.name.starts_with("__effect_on_"))
        .expect("on effect");
    assert_eq!(on_effect.deps, vec![tick_id]);
}

fn node_id_by_name(ir: &flux_ir::CompiledProgram, name: &str) -> flux_ir::NodeId {
    ir.nodes
        .iter()
        .position(|n| n.name == name)
        .map(|i| flux_ir::NodeId(i as u32))
        .unwrap_or_else(|| panic!("node `{name}` not found"))
}

fn dep_names(ir: &flux_ir::CompiledProgram, node_name: &str) -> Vec<String> {
    let id = node_id_by_name(ir, node_name);
    ir.nodes[id.0 as usize]
        .deps
        .iter()
        .map(|dep| ir.nodes[dep.0 as usize].name.clone())
        .collect()
}

#[test]
fn struct_field_access_adds_derived_dependency() {
    let src =
        "struct Point { x: Int, y: Int }\nlet pt = Point { x: 1, y: 2 }\nlet sum = pt.x + pt.y\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();
    let deps = dep_names(&ir, "sum");
    assert!(
        deps.contains(&"pt".to_string()),
        "sum should depend on pt, got {deps:?}"
    );
}

#[test]
fn match_guard_adds_derived_dependency() {
    let src = "let a = 1\nlet x = 5\nlet out = match x { n if n > a => n, _ => 0 }\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();
    let deps = dep_names(&ir, "out");
    assert!(
        deps.contains(&"a".to_string()),
        "out should depend on a via match guard, got {deps:?}"
    );
}

#[test]
fn lowers_trait_method_call_in_derived() {
    let src = r#"
trait Display {
    fn to_string(x: Int) -> String;
}
impl Display for Int {
    fn to_string(self: Int) -> String {
        "int"
    }
}
let msg = (42).to_string()
"#;
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();
    let msg = ir.nodes.iter().find(|n| n.name == "msg").expect("msg node");
    assert!(
        msg.code
            .iter()
            .any(|i| matches!(i, flux_ir::Inst::Call { .. })),
        "expected trait dispatch Call in msg derived code: {:?}",
        msg.code
    );
}

#[test]
fn lowers_module_if_into_init_stmts() {
    let src =
        "let use_true = true\nlet mut a = 0\nif use_true {\n    a = 10\n} else {\n    a = 5\n}\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    let ir = lower_to_ir(&hir, SourceId(0), &mut diags).unwrap();
    assert!(
        !ir.init_stmts.is_empty(),
        "expected init_stmts for module-level if"
    );
    assert!(
        ir.init_stmts
            .iter()
            .any(|s| matches!(s, flux_ir::IrStmt::If { .. })),
        "expected If in init_stmts, got {:?}",
        ir.init_stmts
    );
}
