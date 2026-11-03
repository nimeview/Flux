use flux_diagnostics::{DiagnosticBag, FluxError, SourceId};
use flux_hir::{lower_program, HirItem};
use flux_syntax::parse_program;
use flux_types::{monomorphize_program, typecheck_program};
use miette::Diagnostic;

fn check(source: &str) -> DiagnosticBag {
    let mut diagnostics = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), source, &mut diagnostics).expect("parse");
    let mut hir = lower_program(&ast, &mut diagnostics).expect("lower");
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.errors);
    monomorphize_program(&mut hir, SourceId(0), &mut diagnostics);
    typecheck_program(&hir, SourceId(0), &mut diagnostics);
    diagnostics
}

#[test]
fn fallback_field_error_explains_the_parameter_default_once() {
    let diagnostics = check("struct Total { total: Int }\nlet f = |t| t.total\n");
    assert_eq!(diagnostics.errors.len(), 1, "{:?}", diagnostics.errors);
    let error = &diagnostics.errors[0];
    assert_eq!(error.code(), "E005");
    assert_eq!(
        error.to_string(),
        "type mismatch: expected `field `total``, found `Int64`"
    );
    assert_eq!(error.help().unwrap().to_string(),
        "closure parameter `t` defaulted to `Int` because it had no type annotation or call-site hint; add an explicit annotation, e.g. `|t: YourType| ...`");
}

#[test]
fn explicit_and_inferred_int_parameters_get_no_fallback_help() {
    for source in [
        "let f = |t: Int| t.total\n",
        "fn apply(f: fn(Int) -> Int) -> Int { f(1) }\nlet n = apply(|t| t.total)\n",
        "import collections { map }\nlet ns = map([1, 2], |t| t.total)\n",
        "fn f(t: Int) -> Int { t.total }\n",
        "let t = 1\nlet x = t.total\n",
        // Same spelling, different symbols: only the first closure defaulted.
        "let f = |t| t + 1\nlet g = |t: Int| t.total\n",
    ] {
        let diagnostics = check(source);
        assert!(!diagnostics.errors.is_empty(), "{source}");
        assert!(
            diagnostics.errors.iter().all(|e| e.help().is_none()),
            "{source}: {:?}",
            diagnostics.errors
        );
        assert!(
            diagnostics
                .errors
                .iter()
                .any(|e| matches!(e, FluxError::TypeMismatch { .. })),
            "{source}"
        );
    }
}

#[test]
fn annotation_and_valid_integer_fallback_keep_working() {
    for source in [
        "struct Total { total: Int }\nlet f = |t: Total| t.total\nlet n = f(Total { total: 2 })\n",
        "let f = |t| t + 1\nlet n = f(2)\n",
    ] {
        let diagnostics = check(source);
        assert!(!diagnostics.has_errors(), "{:?}", diagnostics.errors);
    }
}

#[test]
fn captured_and_generic_closures_preserve_fallback_provenance() {
    for source in [
        "fn make(k: Int) { let f = |t| t.total + k }\n",
        "fn make[T](k: T) { let f = |t| t.total + k }\nlet n = make(1)\n",
        "let f = |t| (|u: Int| t.total + u)(1)\n",
    ] {
        let diagnostics = check(source);
        let hinted: Vec<_> = diagnostics
            .errors
            .iter()
            .filter(|e| e.help().is_some())
            .collect();
        assert_eq!(hinted.len(), 1, "{source}: {:?}", diagnostics.errors);
        assert!(hinted[0]
            .help()
            .unwrap()
            .to_string()
            .contains("parameter `t` defaulted"));
    }
}

#[test]
fn lowering_marks_only_parameters_without_annotations_or_hints() {
    let source = "fn apply(f: fn(Int) -> Int) -> Int { f(1) }\nlet a = |t| t\nlet b = |t: Int| t\nlet c = apply(|t| t)\nlet d = |x, y: Int| x + y\n";
    let mut diagnostics = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), source, &mut diagnostics).expect("parse");
    let mut hir = lower_program(&ast, &mut diagnostics).expect("lower");
    monomorphize_program(&mut hir, SourceId(0), &mut diagnostics);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.errors);
    let flags: Vec<_> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|name| {
            let binding = hir
                .items
                .iter()
                .find_map(|item| match item {
                    HirItem::Binding(b) if hir.names[&b.name] == name => Some(b),
                    _ => None,
                })
                .expect("binding");
            let value = match &binding.value {
                flux_hir::HirExpr::Call { args, .. } => &args[0],
                value => value,
            };
            let flux_hir::HirExpr::Closure { fn_symbol, .. } = value else {
                panic!("expected closure, got {value:?}");
            };
            hir.items
                .iter()
                .find_map(|item| match item {
                    HirItem::Fn(f) if f.name == *fn_symbol => Some(
                        f.params
                            .iter()
                            .map(|p| p.defaulted_to_int)
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .expect("closure function")
        })
        .collect();
    assert_eq!(
        flags,
        vec![vec![true], vec![false], vec![false], vec![true, false]]
    );
}
