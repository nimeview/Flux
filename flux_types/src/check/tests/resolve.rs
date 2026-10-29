//! Tests for HIR type annotation lowering.

use crate::types::Type;
use flux_diagnostics::Span;
use flux_hir::HirType;

use crate::check::resolve::resolve_hir_type_simple;

fn dummy_span() -> Span {
    Span::new(flux_diagnostics::SourceId(0), 0, 0)
}

#[test]
fn resolve_named_int() {
    let ty = resolve_hir_type_simple(&HirType::Named {
        name: "Int".into(),
        span: dummy_span(),
    });
    assert_eq!(ty, Type::Int64);
}

#[test]
fn resolve_named_string() {
    let ty = resolve_hir_type_simple(&HirType::Named {
        name: "String".into(),
        span: dummy_span(),
    });
    assert_eq!(ty, Type::String);
}

#[test]
fn resolve_array_of_int() {
    let ty = resolve_hir_type_simple(&HirType::Array {
        inner: Box::new(HirType::Named {
            name: "Int".into(),
            span: dummy_span(),
        }),
        span: dummy_span(),
    });
    assert_eq!(ty, Type::Array(Box::new(Type::Int64)));
}

#[test]
fn resolve_option_int() {
    let ty = resolve_hir_type_simple(&HirType::Option {
        inner: Box::new(HirType::Named {
            name: "Int".into(),
            span: dummy_span(),
        }),
        span: dummy_span(),
    });
    assert_eq!(ty, Type::Option(Box::new(Type::Int64)));
}

#[test]
fn resolve_unknown_named_becomes_enum_stub() {
    let ty = resolve_hir_type_simple(&HirType::Named {
        name: "MyType".into(),
        span: dummy_span(),
    });
    assert!(matches!(ty, Type::Enum { name, .. } if name == "MyType"));
}

#[test]
fn resolve_type_param_var() {
    let ty = resolve_hir_type_simple(&HirType::TypeParam {
        name: "T".into(),
        span: dummy_span(),
    });
    assert_eq!(ty, Type::Error);
}
