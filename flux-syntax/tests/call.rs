use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_syntax::{parse_program, Expr, Item};

#[test]
fn parses_zero_arg_call_in_binding() {
    let mut diags = DiagnosticBag::new();
    let program = parse_program(SourceId(0), "let x = f()\n", &mut diags).expect("parse");
    let Item::Let(b) = &program.items[0] else {
        panic!("let binding")
    };
    assert!(
        matches!(b.value, Expr::Call { .. }),
        "expected Call, got {:?}",
        std::mem::discriminant(&b.value)
    );
}
