#[cfg(test)]
mod tests {
    use flux_diagnostics::{DiagnosticBag, SourceId};
    use flux_syntax::parse_program;

    use crate::lower_program;

    #[test]
    fn infers_derived_binding() {
        let src = "let a = 5\nlet b = 2\nlet c = a * b\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        assert!(!diags.has_errors());

        let hir = lower_program(&ast, &mut diags).expect("lower");
        assert!(!diags.has_errors());

        let binding = hir
            .items
            .iter()
            .find_map(|i| match i {
                crate::HirItem::Binding(b) if hir.names.get(&b.name) == Some(&"c".to_string()) => {
                    Some(b)
                }
                _ => None,
            })
            .expect("binding c");

        assert!(matches!(binding.kind, crate::BindingKind::Derived));
    }

    #[test]
    fn rejects_reactive_read_in_pure_fn() {
        let src = "let a = 5\nfn foo() { let x = a + 1 }\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags);
        assert!(hir.is_none());
        assert!(diags.has_errors());
        assert!(diags
            .errors
            .iter()
            .any(|e| matches!(e, flux_diagnostics::FluxError::ReactiveInPureContext { .. })));
    }

    #[test]
    fn allows_snap_in_pure_fn() {
        let src = "let a = 5\nfn foo() { let x = snap(a) + 1 }\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        assert!(!diags.has_errors());
        assert_eq!(hir.items.len(), 2);
    }

    #[test]
    fn module_let_lowers_like_binding() {
        let src = "let a = 5\nlet b = a + 1\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        assert!(!diags.has_errors());
        assert_eq!(hir.items.len(), 2);
        assert!(matches!(hir.items[0], crate::HirItem::Binding(_)));
    }

    #[test]
    fn folds_const_expr_to_source_binding() {
        let src = "let two = 1 + 1\nlet label = \"a\" + \"b\"\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");

        let two = binding_named(&hir, "two");
        assert!(matches!(two.kind, crate::BindingKind::Source));
        assert!(matches!(two.value, crate::HirExpr::Int { value: 2, .. }));

        let label = binding_named(&hir, "label");
        assert!(matches!(label.kind, crate::BindingKind::Source));
        assert!(matches!(
            label.value,
            crate::HirExpr::String { ref value, .. } if value == "ab"
        ));
    }

    #[test]
    fn const_if_folds_to_source() {
        let src = "let picked = if true { 10 } else { 20 }\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");

        let picked = binding_named(&hir, "picked");
        assert!(matches!(picked.kind, crate::BindingKind::Source));
        assert!(matches!(
            picked.value,
            crate::HirExpr::Int { value: 10, .. }
        ));
    }

    #[test]
    fn parses_module_destructure_let() {
        let src = "struct Point { x: Int, y: Int }\nlet pt = Point { x: 3, y: 4 }\nlet Point { x, y } = pt\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        assert_eq!(ast.items.len(), 3);
        assert!(matches!(ast.items[2], flux_syntax::Item::Let(_)));
    }

    #[test]
    fn lowers_module_destructure_without_sum() {
        let src = "struct Point { x: Int, y: Int }\nlet pt = Point { x: 3, y: 4 }\nlet Point { x, y } = pt\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).unwrap_or_else(|| {
            panic!("lower failed: {:?}", diags.errors);
        });
        let names: Vec<_> = hir
            .items
            .iter()
            .filter_map(|i| match i {
                crate::HirItem::Binding(b) => hir.names.get(&b.name).cloned(),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"x".to_string()), "bindings: {names:?}");
    }

    #[test]
    fn module_destructure_let_creates_field_bindings() {
        let src = "struct Point { x: Int, y: Int }\nlet pt = Point { x: 3, y: 4 }\nlet Point { x, y } = pt\nlet sum = x + y\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).unwrap_or_else(|| {
            panic!("lower failed: {:?}", diags.errors);
        });
        let names: Vec<_> = hir
            .items
            .iter()
            .filter_map(|i| match i {
                crate::HirItem::Binding(b) => hir.names.get(&b.name).cloned(),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"x".to_string()), "bindings: {names:?}");
        assert!(names.contains(&"y".to_string()), "bindings: {names:?}");
    }

    #[test]
    fn lowers_read_line_call() {
        let src = "import io\nlet x = read_line()\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        assert!(!diags.has_errors(), "{:?}", diags.errors);
        assert!(!hir.items.is_empty());
    }

    #[test]
    fn builtins_include_io() {
        assert!(flux_std::BUILTIN_FNS.contains(&"read_line"));
        assert!(flux_std::BUILTIN_FNS.contains(&"write_file"));
    }

    #[test]
    fn lookup_by_id_survives_scope_pop() {
        use crate::{
            symbol::{SymbolKind, SymbolTable},
            ScopeKind,
        };
        use flux_diagnostics::Span;

        let mut table = SymbolTable::new();
        let span = Span {
            source: SourceId(0),
            start: 0,
            end: 1,
        };
        table.push(ScopeKind::Block);
        let id = table
            .declare("tmp", SymbolKind::Local { mutable: false }, span)
            .unwrap();
        table.pop();
        let sym = table.lookup_by_id(id).expect("symbol after pop");
        assert_eq!(sym.name, "tmp");
    }

    #[test]
    fn lower_linked_program_resolves_qualified_import() {
        use std::path::PathBuf;

        use flux_resolve::resolve_file;

        use crate::lower_linked_program;

        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs/ok/modules/import_math.fl");
        let mut diags = DiagnosticBag::new();
        let resolved = resolve_file(&path, &mut diags).expect("resolve");
        let hir = lower_linked_program(&resolved, &mut diags).expect("lower linked");
        assert!(
            hir.qualified_symbols
                .contains_key(&("m".to_string(), "double".to_string())),
            "expected m.double in qualified_symbols"
        );
        assert!(
            hir.items
                .iter()
                .any(|i| matches!(i, crate::HirItem::Binding(_))),
            "expected imported binding items"
        );
    }

    #[test]
    fn unused_binding_warning_emitted_during_lower() {
        let src = "let live = 1\nlet ghost = 2\nlet out = live\n";
        let mut diags = DiagnosticBag::new();
        let ast = parse_program(SourceId(0), src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        assert!(hir.items.len() >= 3);
        assert!(diags.warnings.iter().any(|w| {
            matches!(
                w,
                flux_diagnostics::FluxWarning::UnusedBinding { name, .. } if name == "ghost"
            )
        }));
    }

    fn binding_named<'a>(hir: &'a crate::HirProgram, name: &str) -> &'a crate::HirBinding {
        hir.items
            .iter()
            .find_map(|item| match item {
                crate::HirItem::Binding(b) if hir.names.get(&b.name) == Some(&name.to_string()) => {
                    Some(b)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("binding `{name}` not found"))
    }
}
