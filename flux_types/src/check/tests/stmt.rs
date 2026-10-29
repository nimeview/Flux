//! Statement-level type checking tests.

use flux_diagnostics::{DiagnosticBag, FluxError, SourceId};
use flux_hir::lower_program;
use flux_syntax::parse_program;

use crate::typecheck_program;

#[test]
fn on_non_event_binding_reports_unknown_event() {
    let src = "event Inc\nlet mut count = 0\non count { count = count + 1 }\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let hir = lower_program(&ast, &mut diags).expect("lower");
    assert!(typecheck_program(&hir, SourceId(0), &mut diags).is_none());
    assert!(diags
        .errors
        .iter()
        .any(|e| matches!(e, FluxError::UnknownEvent { .. })));
}

#[test]
fn assign_to_derived_reports_e003() {
    let src = "let a = 1\nlet b = a + 1\nwatch { b = 2 }\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let hir = lower_program(&ast, &mut diags).expect("lower");
    assert!(typecheck_program(&hir, SourceId(0), &mut diags).is_none());
    assert!(diags
        .errors
        .iter()
        .any(|e| matches!(e, FluxError::AssignToDerived { .. })));
}
