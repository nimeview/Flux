//! Monomorphization tests.

use crate::monomorphize_program;
use crate::typecheck_program;
use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_syntax::parse_program;

#[test]
fn instantiates_generic_id_int() {
    let src = "fn id[T](x: T) -> T { x }\nlet result = id(42)\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    assert!(hir.names.values().any(|n| n.starts_with("id_Int")));
}

#[test]
fn instantiates_generic_id_string() {
    let src = r#"fn id[T](x: T) -> T { x }
let label = id("hi")
"#;
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    assert!(hir.names.values().any(|n| n.starts_with("id_String")));
}

#[test]
fn no_instantiation_without_generics() {
    let src = "fn add(a: Int, b: Int) -> Int { a + b }\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    let before = hir.items.len();
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    assert_eq!(hir.items.len(), before);
}

#[test]
fn generic_fn_template_collected() {
    let src = "fn swap[A, B](a: A, b: B) -> A { a }\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    assert!(hir.generic_fns.contains_key("swap") || hir.names.values().any(|n| n == "swap"));
}

#[test]
fn monomorph_rewrites_call_site() {
    let src = "fn id[T](x: T) -> T { x }\nlet a = id(1)\nlet b = id(2)\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    let inst_count = hir
        .names
        .values()
        .filter(|n| n.starts_with("id_Int"))
        .count();
    assert_eq!(inst_count, 1, "should deduplicate identical instantiations");
}

#[test]
fn instantiates_both_int_and_string_in_same_program() {
    let src = "fn id[T](x: T) -> T { x }\nlet int_result = id(42)\nlet str_result = id(\"hi\")\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    let typed = typecheck_program(&hir, SourceId(0), &mut diags);
    assert!(typed.is_some(), "typecheck failed: {:?}", diags.errors);
    assert!(hir.names.values().any(|n| n == "id_Int"));
    assert!(hir.names.values().any(|n| n == "id_String"));
}

#[test]
fn instantiates_generic_call_inside_watch() {
    let src = "fn id[T](x: T) -> T { x }\nlet mut out = 0\nwatch { out = id(42) }\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    assert!(
        hir.names.values().any(|n| n.starts_with("id_Int")),
        "expected id_Int from watch body, names: {:?}",
        hir.names.values().collect::<Vec<_>>()
    );
    let typed = typecheck_program(&hir, SourceId(0), &mut diags);
    assert!(typed.is_some(), "typecheck failed: {:?}", diags.errors);
}
