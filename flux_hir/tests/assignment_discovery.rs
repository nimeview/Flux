use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_syntax::parse_program;

fn errors(source: &str) -> DiagnosticBag {
    let mut diagnostics = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), source, &mut diagnostics).expect("parse");
    lower_program(&ast, &mut diagnostics);
    diagnostics
}

#[test]
fn nested_if_assignment_in_match_rejects_immutable_module_let() {
    for body in [
        "if true { x = 1 }",
        "match 0 { _ => { if true { x = 1 } 0 } }",
        "let y = match 0 { _ => { if true { x = 1 } 0 } }",
    ] {
        let source = format!("let x = 0\nfn main() {{ {body} }}\n");
        let diagnostics = errors(&source);
        assert!(
            diagnostics.errors.iter().any(|e| e.code() == "E016"),
            "{source}: {:?}",
            diagnostics.errors
        );
    }
}

#[test]
fn nested_assignment_with_mutable_module_let_still_lowers() {
    let diagnostics =
        errors("let mut x = 0\nfn main() { match 0 { _ => { if true { x = 1 } 0 } } }\n");
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.errors);
}

#[test]
fn local_and_pattern_shadowing_do_not_mark_module_let_assigned() {
    for body in [
        "let mut x = 0\nmatch 0 { _ => { if true { x = 1 } 0 } }",
        "match 0 { x => { if true { x = 1 } 0 } }",
        "match Some(0) { Some(x) => { if true { x = 1 } 0 }, _ => 0 }",
        "match Some(Some(0)) { Some(Some(x)) => { if true { x = 1 } 0 }, _ => 0 }",
    ] {
        let source = format!("let x = 0\nfn main() {{ {body} }}\n");
        let diagnostics = errors(&source);
        assert!(
            diagnostics.errors.iter().all(|e| e.code() != "E016"),
            "{source}: {:?}",
            diagnostics.errors
        );
    }
}
