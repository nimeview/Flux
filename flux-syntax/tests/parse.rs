use flux_diagnostics::{DiagnosticBag, FluxError, SourceId};
use flux_syntax::{parse_program, Item};

fn parse(src: &str) -> flux_syntax::Program {
    let mut diags = DiagnosticBag::new();
    parse_program(SourceId(0), src, &mut diags).expect("parse failed")
}

#[test]
fn parse_error_on_invalid_syntax() {
    let mut diags = DiagnosticBag::new();
    let result = parse_program(SourceId(0), "fn fn fn\n", &mut diags);
    assert!(result.is_none());
    assert!(diags.has_errors());
}

#[test]
fn parse_error_has_precise_span() {
    let input = "fn fn fn\n";
    let mut diags = DiagnosticBag::new();
    assert!(parse_program(SourceId(0), input, &mut diags).is_none());
    let FluxError::Parse { span, .. } = &diags.errors[0] else {
        panic!("expected parse error");
    };
    // pest points at the unexpected third `fn` (byte offset 6)
    assert_eq!(span.offset(), 6);
    assert!(!span.is_empty());
}

#[test]
fn parses_let_mut_at_module_level() {
    let program = parse("let mut a = 5\n");
    assert_eq!(program.items.len(), 1);
    let Item::Let(l) = &program.items[0] else {
        panic!("let")
    };
    assert!(l.mutable);
}

#[test]
fn parses_derived_keyword() {
    let program = parse("derived c = a * b\n");
    assert!(matches!(program.items[0], Item::Derived(_)));
}

#[test]
fn parses_derived_chain() {
    let src = "let a = 5\nlet b = 2\nlet c = a * b\n";
    let program = parse(src);
    assert_eq!(program.items.len(), 3);
}

#[test]
fn parses_fn_with_let_body() {
    let src = "fn foo() {\n  let x = 1\n}\n";
    let program = parse(src);
    assert!(matches!(program.items[0], Item::Fn(_)));
}

#[test]
fn parses_fn_with_params_and_return_type() {
    let src = "fn double(x: Int) -> Int {\n  x * 2\n}\n";
    let program = parse(src);
    let Item::Fn(f) = &program.items[0] else {
        panic!("expected fn");
    };
    assert_eq!(f.params.len(), 1);
    assert!(f.ret.is_some());
}

#[test]
fn parses_bool_precedence() {
    let src = "let x = a && b || c\n";
    let program = parse(src);
    assert_eq!(program.items.len(), 1);
}
