use flux_ir::{CompiledProgram, Inst, IrNode, IrValue, NodeKind};
use flux_runtime::Executor;

#[test]
fn rejects_invalid_load_at_executor_init() {
    let program = CompiledProgram {
        nodes: vec![
            IrNode {
                name: "a".into(),
                kind: NodeKind::Source,
                deps: vec![],
                init: Some(IrValue::Int(0)),
                code: vec![],
                effect: None,
            },
            IrNode {
                name: "b".into(),
                kind: NodeKind::Derived,
                deps: vec![],
                init: None,
                code: vec![
                    Inst::Load("missing_binding".into()),
                    Inst::Const(IrValue::Int(1)),
                    Inst::Add,
                ],
                effect: None,
            },
        ],
        init_stmts: vec![],
        functions: vec![],
        watches: vec![],
        on_handlers: vec![],
        events: vec![],
    };
    assert!(
        Executor::new(program).is_err(),
        "invalid bytecode must fail validation"
    );
}
