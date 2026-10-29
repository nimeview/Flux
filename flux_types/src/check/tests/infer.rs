//! End-to-end inference tests via parse → lower → typecheck.

use crate::monomorphize_program;
use crate::{typecheck_program, Type, TypeCheckContext, TypedProgram};
use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_syntax::parse_program;

fn check_source(src: &str) -> (flux_hir::HirProgram, TypedProgram) {
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    assert!(!diags.has_errors(), "{:?}", diags.errors);
    let mut hir = lower_program(&ast, &mut diags).expect("lower");
    assert!(!diags.has_errors(), "{:?}", diags.errors);
    monomorphize_program(&mut hir, SourceId(0), &mut diags);
    let typed = typecheck_program(&hir, SourceId(0), &mut diags).expect("typecheck");
    assert!(!diags.has_errors(), "{:?}", diags.errors);
    (hir, typed)
}

fn binding_type(hir: &flux_hir::HirProgram, typed: &TypedProgram, name: &str) -> Type {
    let id = hir
        .names
        .iter()
        .find(|(_, n)| n.as_str() == name)
        .map(|(id, _)| *id)
        .expect(name);
    typed.types.get(&id).cloned().expect("type")
}

#[test]
fn infers_int_literal_binding() {
    let (hir, typed) = check_source("let x = 42\n");
    let ty = binding_type(&hir, &typed, "x");
    assert!(matches!(ty, Type::Cell(inner) if *inner == Type::Int64));
}

#[test]
fn infers_derived_arithmetic() {
    let (hir, typed) = check_source("let a = 1\nlet b = 2\nlet c = a + b\n");
    let ty = binding_type(&hir, &typed, "c");
    assert!(matches!(ty, Type::Read(inner) if *inner == Type::Int64));
}

#[test]
fn infers_string_concat() {
    let (hir, typed) = check_source(r#"let s = "a" + "b""#);
    let ty = binding_type(&hir, &typed, "s");
    assert!(matches!(
        &ty,
        Type::Cell(inner) | Type::Read(inner) if **inner == Type::String
    ));
}

#[test]
fn infers_bool_comparison() {
    let (hir, typed) = check_source("let a = 1\nlet b = a > 0\n");
    let ty = binding_type(&hir, &typed, "b");
    assert!(matches!(ty, Type::Read(inner) if *inner == Type::Bool));
}

#[test]
fn infers_float_mixed_arithmetic() {
    let (hir, typed) = check_source("let x = 1.5 + 2\n");
    let ty = binding_type(&hir, &typed, "x");
    let inner = match ty {
        Type::Cell(inner) | Type::Read(inner) => *inner,
        other => other,
    };
    assert_eq!(inner, Type::Float64);
}

#[test]
fn struct_type_registered() {
    let src = "struct Point { x: Int, y: Int }\nlet p = Point { x: 1, y: 2 }\n";
    let (hir, typed) = check_source(src);
    let ctx = TypeCheckContext::from_program(&hir);
    let point = ctx.user_type("Point").expect("Point type");
    assert!(matches!(point, Type::Struct { fields, .. } if fields.len() == 2));
    let _ = typed;
}

#[test]
fn enum_type_registered() {
    let src = "enum Color { Red, Green, Blue }\n";
    let (hir, typed) = check_source(src);
    let ctx = TypeCheckContext::from_program(&hir);
    let color = ctx.user_type("Color").expect("Color");
    assert!(matches!(color, Type::Enum { variants, .. } if variants.len() == 3));
    let _ = typed;
}

#[test]
fn trait_impl_indexed() {
    let src = r#"trait Display { fn to_string(x: Int) -> String; }
impl Display for Int { fn to_string(self: Int) -> String { "i" } }
"#;
    let (hir, _typed) = check_source(src);
    let ctx = TypeCheckContext::from_program(&hir);
    assert!(ctx.trait_impl("Int", "to_string").is_some());
}

#[test]
fn function_return_type_inferred() {
    let src = "fn add(a: Int, b: Int) -> Int { a + b }\n";
    let (hir, typed) = check_source(src);
    let id = hir
        .names
        .iter()
        .find(|(_, n)| n.as_str() == "add")
        .map(|(id, _)| *id)
        .unwrap();
    let ty = typed.types.get(&id).unwrap();
    assert!(matches!(ty, Type::Func { ret, .. } if **ret == Type::Int64));
}

#[test]
fn event_type_is_event_unit() {
    let src = "event Tick\n";
    let (hir, typed) = check_source(src);
    let id = hir
        .names
        .iter()
        .find(|(_, n)| n.as_str() == "Tick")
        .map(|(id, _)| *id)
        .unwrap();
    let ty = typed.types.get(&id).unwrap();
    assert!(matches!(ty, Type::Event(inner) if **inner == Type::Unit));
}

#[test]
fn array_literal_inferred() {
    let (hir, typed) = check_source("let nums = [1, 2, 3]\n");
    let ty = binding_type(&hir, &typed, "nums");
    let inner = match ty {
        Type::Cell(inner) | Type::Read(inner) => *inner,
        other => other,
    };
    assert!(matches!(inner, Type::Array(_)));
}

#[test]
fn option_some_payload() {
    let src = "enum Opt { Some(Int), None }\n";
    let (hir, _) = check_source(src);
    let ctx = TypeCheckContext::from_program(&hir);
    let payload = ctx.enum_payload("Opt", "Some").expect("Some payload");
    assert_eq!(*payload, Type::Int64);
}

#[test]
fn rejects_type_mismatch_in_binding() {
    let src = "let x: String = 42\n";
    let mut diags = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
    let hir = lower_program(&ast, &mut diags).expect("lower");
    let result = typecheck_program(&hir, SourceId(0), &mut diags);
    assert!(result.is_none());
    assert!(diags.has_errors());
}

#[test]
fn if_expression_infers_union_numeric() {
    let (hir, typed) = check_source("let a = 1\nlet b = if a > 0 { 1 } else { 2 }\n");
    let ty = binding_type(&hir, &typed, "b");
    assert!(matches!(ty, Type::Read(inner) if *inner == Type::Int64));
}
