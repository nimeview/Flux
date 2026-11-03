use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_syntax::parse_program;
use flux_types::{monomorphize_program, typecheck_program};

fn errors(source: &str) -> DiagnosticBag {
    let mut diagnostics = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), source, &mut diagnostics).expect("parse");
    let mut hir = lower_program(&ast, &mut diagnostics).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diagnostics);
    typecheck_program(&hir, SourceId(0), &mut diagnostics);
    diagnostics
}

#[test]
fn reduce_requires_exact_accumulator_result_type_at_the_callback() {
    for (initial, callback, expected, found) in [
        ("Acc { v: 0 }", "|a, b| a.v + b", "Acc", "Int64"),
        ("0", "|a, b| \"s\"", "Int64", "String"),
        ("0", "|a, b| 1.0", "Int64", "Float64"),
        ("0", "|a, b| 1i8", "Int64", "Int8"),
    ] {
        let source = format!("import collections {{ reduce }}\nstruct Acc {{ v: Int }}\nlet n = reduce([1, 2], {initial}, {callback})\n");
        let diagnostics = errors(&source);
        assert_eq!(diagnostics.errors.len(), 1, "{:?}", diagnostics.errors);
        let error = &diagnostics.errors[0];
        assert_eq!(error.code(), "E005");
        assert_eq!(
            error.to_string(),
            format!("type mismatch: expected `{expected}`, found `{found}`")
        );
        let span = error.label_span();
        assert_eq!(&source[span.start..span.end], callback);
    }
}

#[test]
fn filter_requires_bool_and_all_hofs_require_callable_callbacks() {
    for (call, expected, found) in [
        ("filter([1], |i| i + 1)", "Bool", "Int64"),
        ("map([1], 3)", "function callback", "Int64"),
        ("filter([1], 3)", "function callback", "Int64"),
        ("reduce([1], 0, 3)", "function callback", "Int64"),
    ] {
        let diagnostics = errors(&format!(
            "import collections {{ map, filter, reduce }}\nlet n = {call}\n"
        ));
        assert_eq!(diagnostics.errors.len(), 1, "{:?}", diagnostics.errors);
        assert_eq!(
            diagnostics.errors[0].to_string(),
            format!("type mismatch: expected `{expected}`, found `{found}`")
        );
    }
}

#[test]
fn unused_generic_closure_checks_known_types_without_assuming_unknown_types() {
    for body in ["map([1], |i| i + true)", "map(xs, |i| 1 + true)"] {
        let diagnostics = errors(&format!(
            "import collections {{ map }}\nfn unused[T](xs: [T]) {{ {body} }}\n"
        ));
        assert_eq!(diagnostics.errors.len(), 1, "{:?}", diagnostics.errors);
        assert_eq!(
            diagnostics.errors[0].to_string(),
            "type mismatch: expected `numeric or String type`, found `Int64 and Bool`"
        );
    }
    let diagnostics =
        errors("import collections { map }\nfn unused[T](xs: [T]) { map(xs, |i| i + true) }\n");
    assert_eq!(diagnostics.errors.len(), 1, "{:?}", diagnostics.errors);
    assert_eq!(
        diagnostics.errors[0].to_string(),
        "type mismatch: expected `numeric or String type`, found `Bool`"
    );
    for body in [
        "map(xs, |i| i.v)",
        "map(xs, |i| i.value())",
        "map(xs, |i| i + 1)",
        "map(xs, |i| len(i))",
        "map(xs, |i| to_string(i))",
    ] {
        let diagnostics = errors(&format!("import collections {{ map, len }}\nimport str {{ to_string }}\nfn unused[T](xs: [T]) {{ {body} }}\n"));
        assert!(
            !diagnostics.has_errors(),
            "{body}: {:?}",
            diagnostics.errors
        );
    }
}

#[test]
fn generic_callback_result_is_inferred_after_its_input() {
    let diagnostics = errors(
        "fn apply[T, U](x: T, f: fn(T) -> U) -> U { f(x) }\nlet result = apply(\"a\", |i| i)\n",
    );
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.errors);
}

#[test]
fn instantiated_generic_body_does_not_also_report_template_errors() {
    let diagnostics = errors("import collections { map }\nfn bad[T](xs: [T]) { map(xs, |i| i + true) }\nfn main() { bad([1])\n0 }\n");
    assert_eq!(diagnostics.errors.len(), 1, "{:?}", diagnostics.errors);
    assert_eq!(
        diagnostics.errors[0].to_string(),
        "type mismatch: expected `numeric or String type`, found `Int64 and Bool`"
    );
}
