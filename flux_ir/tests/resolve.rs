use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_ir::{lower_to_ir, resolve_locals, CompiledProgram, Inst, IrStmt};
use flux_syntax::parse_program;

fn lower(src: &str) -> CompiledProgram {
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).unwrap();
    let hir = lower_program(&ast, &mut diags).unwrap();
    lower_to_ir(&hir, SourceId(0), &mut diags).unwrap()
}

fn function<'a>(ir: &'a CompiledProgram, name: &str) -> &'a [IrStmt] {
    &ir.functions.iter().find(|f| f.name == name).unwrap().body
}

#[test]
fn params_and_lets_get_slots_and_globals_stay_by_name() {
    let ir = lower("const k = 2\nfn f(a: Int, b: Int) -> Int {\n    let c = a + b\n    c * k\n}\n");
    let body = function(&ir, "f");
    let IrStmt::Let { slot, expr, .. } = &body[0] else {
        panic!("{body:?}")
    };
    assert_eq!(*slot, 2, "params take 0 and 1");
    assert!(
        matches!(&expr[0], Inst::LoadLocal { slot: 0, .. }),
        "{expr:?}"
    );
    assert!(
        matches!(&expr[1], Inst::LoadLocal { slot: 1, .. }),
        "{expr:?}"
    );
    let code = match &body[1] {
        IrStmt::Expr { code } | IrStmt::Return { expr: Some(code) } => code,
        other => panic!("{other:?}"),
    };
    assert!(matches!(&code[0], Inst::LoadLocal { slot: 2, .. }));
    assert!(
        code.iter()
            .any(|i| matches!(i, Inst::Load(name) if name == "k")),
        "{code:?}"
    );
}

#[test]
fn assignment_to_a_source_has_no_slot() {
    let ir = lower(
        "event Tick(n: Int)\nlet mut total = 0\non Tick { n ->\n    let mut step = n\n    step = step + 1\n    total = total + step\n}\n",
    );
    let body = &ir.on_handlers[0].body;
    let slots: Vec<_> = body
        .iter()
        .filter_map(|s| match s {
            IrStmt::Assign { target, slot, .. } => Some((target == "total", *slot)),
            _ => None,
        })
        .collect();
    // `n` is slot 0 (the handler parameter), `step` slot 1; the source
    // `total` is assigned by name.
    assert_eq!(slots, [(false, Some(1)), (true, None)]);
}

#[test]
fn resolving_twice_changes_nothing() {
    let mut ir = lower(
        "fn f(xs: [Int]) -> Int {\n    let mut s = 0\n    for x in xs {\n        s = s + x\n    }\n    s\n}\n",
    );
    let before = format!("{:?}", ir.functions);
    resolve_locals(&mut ir);
    assert_eq!(format!("{:?}", ir.functions), before);
}
