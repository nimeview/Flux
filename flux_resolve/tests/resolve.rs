use std::fs;
use std::path::PathBuf;

use flux_diagnostics::{DiagnosticBag, FluxError};
use flux_resolve::{
    link_modules, resolve_file, resolve_file_with_root, try_resolve_file_with_root,
};

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("flux_resolve_{name}_{}", std::process::id()))
}

#[test]
fn resolve_file_with_root_uses_buffer_contents() {
    let root = temp_root("with_root");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("math.fl"),
        "pub fn double(x: Int) -> Int { x * 2 }\n",
    )
    .unwrap();
    fs::write(
        root.join("main.fl"),
        "import math as m\n\nlet x = m.double(4)\n",
    )
    .unwrap();

    let mut diag = DiagnosticBag::new();
    let resolved = resolve_file_with_root(
        &root.join("main.fl"),
        Some("import math as m\n\nlet x = m.double(99)\n"),
        &mut diag,
    )
    .expect("resolve with root override");
    assert_eq!(resolved.modules.len(), 2, "diag: {:?}", diag.errors);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cyclic_import_reports_e013() {
    let root = temp_root("cyclic");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.fl"), "import b\n").unwrap();
    fs::write(root.join("b.fl"), "import a\n").unwrap();
    fs::write(root.join("main.fl"), "import a\n").unwrap();

    let mut diag = DiagnosticBag::new();
    assert!(resolve_file(&root.join("main.fl"), &mut diag).is_none());
    assert!(diag
        .errors
        .iter()
        .any(|e| matches!(e, FluxError::CyclicImport { .. })));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn unknown_export_reports_e014() {
    let root = temp_root("unknown_export");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("math.fl"),
        "pub fn double(x: Int) -> Int { x * 2 }\n",
    )
    .unwrap();
    fs::write(
        root.join("main.fl"),
        "import math { triple }\n\nlet x = 1\n",
    )
    .unwrap();

    let mut diag = DiagnosticBag::new();
    let resolved = resolve_file(&root.join("main.fl"), &mut diag).expect("resolve");
    assert!(link_modules(&resolved, &mut diag).is_none());
    assert!(diag
        .errors
        .iter()
        .any(|e| matches!(e, FluxError::UnknownExport { .. })));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn selective_import_symbols_are_validated() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs/ok/modules/import_math.fl");
    let mut diag = DiagnosticBag::new();
    let resolved = resolve_file(&path, &mut diag).expect("resolve");
    let linked = link_modules(&resolved, &mut diag).expect("link");
    assert_eq!(linked.imports[0].symbols, Vec::<String>::new());
}

#[test]
fn syntax_error_in_root_is_reported() {
    let root = temp_root("root_syntax");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.fl"), "let x =\n").unwrap();

    let mut diag = DiagnosticBag::new();
    let failure = try_resolve_file_with_root(&root.join("main.fl"), None, &mut diag)
        .expect_err("malformed root must fail");
    let err = diag.errors.first().expect("parse diagnostic");
    assert!(matches!(err, FluxError::Parse { .. }), "{err:?}");
    let source = failure.source(err.source_id()).expect("failing source");
    assert!(source.path.ends_with("main.fl"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn syntax_error_in_import_is_reported_against_imported_source() {
    let root = temp_root("import_syntax");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.fl"), "import util\nlet y = 1\n").unwrap();
    fs::write(root.join("util.fl"), "pub fn f( -> Int { 1 }\n").unwrap();

    let mut diag = DiagnosticBag::new();
    let failure = try_resolve_file_with_root(&root.join("main.fl"), None, &mut diag)
        .expect_err("malformed import must fail");
    let err = diag.errors.first().expect("parse diagnostic");
    assert!(matches!(err, FluxError::Parse { .. }), "{err:?}");
    let source = failure.source(err.source_id()).expect("failing source");
    assert!(source.path.ends_with("util.fl"), "{}", source.path);

    let _ = fs::remove_dir_all(&root);
}
