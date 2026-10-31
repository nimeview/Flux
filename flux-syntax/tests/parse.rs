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

fn string_value(src: &str) -> String {
    match &parse(src).items[0] {
        Item::Let(l) => match &l.value {
            flux_syntax::Expr::String { value, .. } => value.clone(),
            other => panic!("expected a plain string literal, got {other:?}"),
        },
        other => panic!("expected let, got {other:?}"),
    }
}

#[test]
fn string_escapes_decode() {
    assert_eq!(
        string_value(r#"let s = "a\nb\tc\rd\0e\\f\"g\{h\}""#),
        "a\nb\tc\rd\0e\\f\"g{h}"
    );
}

#[test]
fn unknown_string_escape_is_a_parse_error() {
    let mut diags = DiagnosticBag::new();
    let src = "let s = \"bad \\q\"\n";
    assert!(parse_program(SourceId(0), src, &mut diags).is_none());
    let err = diags.errors.first().expect("diagnostic");
    assert!(matches!(err, FluxError::Parse { message, .. } if message.contains("`\\q`")));
    let span = err.label_span();
    assert_eq!(&src[span.start..span.end], "\\q");
}

#[test]
fn if_statement_keeps_else_and_else_if_bodies() {
    let program = parse("fn f(c: Int) -> Int {\n  let mut r = 0\n  if c > 10 {\n    r = 1\n  } else if c == 0 {\n    r = 5\n  } else {\n    r = 9\n  }\n  r\n}\n");
    let Item::Fn(f) = &program.items[0] else {
        panic!("expected fn");
    };
    let flux_syntax::Stmt::If(first) = &f.body.stmts[1] else {
        panic!("expected if");
    };
    let Some(flux_syntax::ElseBranch::If(second)) = &first.else_branch else {
        panic!("expected else-if, got {:?}", first.else_branch);
    };
    assert_eq!(
        second.then_branch.stmts.len(),
        1,
        "else-if body was dropped"
    );
    let Some(flux_syntax::ElseBranch::Block(last)) = &second.else_branch else {
        panic!("expected final else, got {:?}", second.else_branch);
    };
    assert_eq!(last.stmts.len(), 1, "else body was dropped");
}

#[test]
fn block_match_arm_splits_statements_and_value() {
    let program = parse(
        "let a = match n {\n  Some(v) => {\n    let w = v + 1\n    w\n  },\n  None => 0\n}\n",
    );
    let Item::Let(l) = &program.items[0] else {
        panic!("expected let");
    };
    let flux_syntax::Expr::Match { arms, .. } = &l.value else {
        panic!("expected match");
    };
    assert_eq!(arms[0].stmts.len(), 1);
    assert!(matches!(&arms[0].expr, flux_syntax::Expr::Ident { name, .. } if name == "w"));
    assert!(arms[1].stmts.is_empty());
}

#[test]
fn block_match_arm_must_end_with_an_expression() {
    let mut diags = DiagnosticBag::new();
    let src = "fn f(n: Int) -> Int {\n  match n {\n    _ => {\n      let x = 1\n    }\n  }\n}\n";
    assert!(parse_program(SourceId(0), src, &mut diags).is_none());
    assert!(diags.errors.iter().any(|e| matches!(
        e,
        FluxError::Parse { message, .. } if message.contains("must end with an expression")
    )));
}
