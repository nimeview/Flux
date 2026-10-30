//! Diagnostics from imported modules must render against their own file:
//! path, line, column and snippet all come from the module the error is in.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn flux(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn flux binary")
}

fn check(files: &[(&str, &str)]) -> (bool, String) {
    let dir = tempfile::tempdir().unwrap();
    for (name, contents) in files {
        fs::write(dir.path().join(name), contents).unwrap();
    }
    let output = flux(&["check", files[0].0], dir.path());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !stderr.contains("Failed to read contents"),
        "diagnostic rendered against the wrong source: {stderr}"
    );
    (output.status.success(), stderr)
}

/// Asserts a rendered header `<file>:<line>:<col>]` and the snippet text.
fn assert_points_at(stderr: &str, file: &str, line: usize, col: usize, snippet: &str) {
    let header = format!("{file}:{line}:{col}]");
    assert!(stderr.contains(&header), "expected `{header}` in: {stderr}");
    assert!(
        stderr.contains(snippet),
        "expected snippet `{snippet}` in: {stderr}"
    );
}

#[test]
fn undefined_name_in_imported_module() {
    let (ok, err) = check(&[
        ("main.fl", "import util { f }\nlet x = f(1)\nlet y = x\n"),
        (
            "util.fl",
            "\n\npub fn f(a: Int) -> Int {\n  missing + a\n}\n",
        ),
    ]);
    assert!(!ok);
    assert!(err.contains("undefined name `missing`"), "{err}");
    assert_points_at(&err, "util.fl", 4, 3, "missing + a");
}

#[test]
fn errors_from_root_and_import_render_in_one_invocation() {
    let (ok, err) = check(&[
        (
            "main.fl",
            "import util { f }\nlet x = f(1)\nlet y: Bool = 3\nlet z = x\n",
        ),
        (
            "util.fl",
            "\npub fn f(a: Int) -> Int {\n  let s: String = a\n  a\n}\n",
        ),
    ]);
    assert!(!ok);
    assert_points_at(&err, "util.fl", 3, 3, "let s: String = a");
    assert_points_at(&err, "main.fl", 3, 1, "let y: Bool = 3");
}

#[test]
fn duplicate_declaration_in_imported_module() {
    let (ok, err) = check(&[
        ("main.fl", "import util { f }\nlet x = f()\nlet y = x\n"),
        (
            "util.fl",
            "pub fn f() -> Int { 1 }\n\npub fn f() -> Int { 2 }\n",
        ),
    ]);
    assert!(!ok);
    assert!(err.contains("util.fl"), "{err}");
    assert!(err.contains("pub fn f() -> Int { 2 }"), "{err}");
}

#[test]
fn unknown_export_points_at_importing_line() {
    let (ok, err) = check(&[
        ("main.fl", "\nimport util { nope }\nlet y = 1\n"),
        ("util.fl", "pub fn f() -> Int { 1 }\n"),
    ]);
    assert!(!ok);
    assert_points_at(&err, "main.fl", 2, 1, "import util { nope }");
}

#[test]
fn root_warning_renders_against_root_when_imports_exist() {
    let (ok, err) = check(&[
        ("main.fl", "import util { f }\nlet y = f()\n\nlet z = y\n"),
        ("util.fl", "\n\n\npub fn f() -> Int { 1 }\n"),
    ]);
    assert!(ok, "{err}");
    assert!(err.contains("unused binding `z`"), "{err}");
    assert_points_at(&err, "main.fl", 4, 1, "let z = y");
}

#[test]
fn cyclic_import_points_at_closing_import() {
    let (ok, err) = check(&[
        (
            "a.fl",
            "import b { g }\npub fn f() -> Int { 1 }\nlet x = g()\n",
        ),
        ("b.fl", "\nimport a { f }\npub fn g() -> Int { f() }\n"),
    ]);
    assert!(!ok);
    assert!(err.contains("cyclic import"), "{err}");
    assert_points_at(&err, "b.fl", 2, 1, "import a { f }");
}

#[test]
fn missing_module_points_at_import() {
    let (ok, err) = check(&[("main.fl", "let a = 1\nimport nowhere { f }\n")]);
    assert!(!ok);
    assert!(err.contains("module not found"), "{err}");
    assert_points_at(&err, "main.fl", 2, 1, "import nowhere");
}
