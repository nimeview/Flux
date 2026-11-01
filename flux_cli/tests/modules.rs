//! Module contract: every module has its own namespace (all of its items plus
//! its own imports, stdlib included); only `pub` items are exported; a
//! module's imports resolve from its directory and its own package's
//! dependencies; a module imported twice is one instance; a module never
//! sees the root's names. Type names are still program-global.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write_tree(root: &Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

fn flux(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn flux")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs `dir/<cwd>/main.fl` on both engines and returns stdout (must succeed).
fn run_ok(files: &[(&str, &str)], cwd: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    write_tree(dir.path(), files);
    let mut outputs = Vec::new();
    for engine in ["--vm", "--jit"] {
        let out = flux(&["run", engine, "main.fl"], &dir.path().join(cwd));
        assert_eq!(
            out.status.code(),
            Some(0),
            "{engine}: stdout={} stderr={}",
            text(&out.stdout),
            text(&out.stderr)
        );
        outputs.push(text(&out.stdout));
    }
    assert_eq!(outputs[0], outputs[1], "VM and JIT differ");
    outputs.remove(0)
}

/// `flux check` must fail with `needle` in stderr.
fn check_fails(files: &[(&str, &str)], cwd: &str, needle: &str) {
    let dir = tempfile::tempdir().unwrap();
    write_tree(dir.path(), files);
    let out = flux(&["check", "main.fl"], &dir.path().join(cwd));
    let err = text(&out.stderr);
    assert!(!out.status.success(), "expected failure: {err}");
    assert!(err.contains(needle), "missing `{needle}` in:\n{err}");
}

const MAIN_TWICE: &str = "import io { println }\nimport lib { twice }\nfn main() -> Int {\n  println(twice(5))\n  0\n}\n";

#[test]
fn pub_fn_can_use_private_items_of_its_module() {
    let out = run_ok(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "fn helper(x: Int) -> Int { x + 1 }\nconst BASE = 100\npub fn twice(x: Int) -> Int { helper(x) * 2 + BASE }\n",
            ),
        ],
        ".",
    );
    assert_eq!(out, "112\n");
}

#[test]
fn a_module_uses_its_own_imports_selected_aliased_and_stdlib() {
    let out = run_ok(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "import helpers { inc }\nimport helpers as h\nimport str { to_string }\npub fn twice(x: Int) -> Int { inc(x) + h.inc(x) }\npub fn show(x: Int) -> String { to_string(x) }\n",
            ),
            ("helpers.fl", "pub fn inc(x: Int) -> Int { x + 1 }\n"),
        ],
        ".",
    );
    assert_eq!(out, "12\n");
}

#[test]
fn same_private_name_in_two_modules_stays_separate() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { fa }\nimport b { fb }\nfn main() -> Int {\n  println(fa() * 10 + fb())\n  0\n}\n",
            ),
            ("a.fl", "fn helper() -> Int { 1 }\npub fn fa() -> Int { helper() }\n"),
            ("b.fl", "fn helper() -> Int { 2 }\npub fn fb() -> Int { helper() }\n"),
        ],
        ".",
    );
    assert_eq!(out, "12\n");
}

#[test]
fn diamond_import_is_one_module() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { fa }\nimport b { fb }\nfn main() -> Int {\n  println(fa() + fb())\n  0\n}\n",
            ),
            ("a.fl", "import shared { base }\npub fn fa() -> Int { base() + 1 }\n"),
            ("b.fl", "import shared { base }\npub fn fb() -> Int { base() + 2 }\n"),
            ("shared.fl", "pub fn base() -> Int { 10 }\n"),
        ],
        ".",
    );
    assert_eq!(out, "23\n");
}

#[test]
fn transitive_path_dependencies_resolve_per_package() {
    let out = run_ok(
        &[
            (
                "app/flux.toml",
                "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n",
            ),
            ("app/main.fl", MAIN_TWICE),
            (
                "lib/flux.toml",
                "[package]\nname = \"lib\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
            ),
            (
                "lib/lib.fl",
                "import base { one }\npub fn twice(x: Int) -> Int { x * 2 + one() }\n",
            ),
            ("base/flux.toml", "[package]\nname = \"base\"\n"),
            ("base/lib.fl", "pub fn one() -> Int { 1 }\n"),
        ],
        "app",
    );
    assert_eq!(out, "11\n");
}

#[test]
fn a_package_cannot_import_its_dependencys_dependencies() {
    check_fails(
        &[
            (
                "app/flux.toml",
                "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n",
            ),
            ("app/main.fl", "import base { one }\nlet a = one()\n"),
            (
                "lib/flux.toml",
                "[package]\nname = \"lib\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
            ),
            ("lib/lib.fl", "pub fn twice(x: Int) -> Int { x * 2 }\n"),
            ("base/flux.toml", "[package]\nname = \"base\"\n"),
            ("base/lib.fl", "pub fn one() -> Int { 1 }\n"),
        ],
        "app",
        "module not found: base",
    );
}

#[test]
fn a_module_does_not_see_the_roots_names() {
    check_fails(
        &[
            (
                "main.fl",
                "import lib { f }\nconst SECRET = 1\nlet a = f()\n",
            ),
            ("lib.fl", "pub fn f() -> Int { SECRET }\n"),
        ],
        ".",
        "undefined name `SECRET`",
    );
}

#[test]
fn private_items_of_a_nested_import_stay_private() {
    check_fails(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "import helpers { hidden }\npub fn twice(x: Int) -> Int { hidden(x) }\n",
            ),
            ("helpers.fl", "fn hidden(x: Int) -> Int { x }\n"),
        ],
        ".",
        "`hidden` in module `helpers` is private",
    );
}

#[test]
fn watch_in_an_imported_module_is_rejected() {
    check_fails(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "pub let mut n = 0\nwatch n {\n  n = 1\n}\npub fn twice(x: Int) -> Int { x }\n",
            ),
        ],
        ".",
        "not supported in an imported module",
    );
}

#[test]
fn imported_reactive_binding_drives_root_derived_values() {
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[
            ("main.fl", "import counter { count }\nevent Tick\nlet doubled = count * 2\non Tick {\n  count = count + 1\n}\n"),
            ("counter.fl", "pub let mut count = 1\n"),
        ],
    );
    let compiled = flux_cli::pipeline::compile_file(
        &dir.path().join("main.fl"),
        flux_runtime::ExecutionMode::Vm,
    )
    .unwrap_or_else(|f| panic!("{:?}", f.diagnostics.errors));
    let mut ex = compiled.executor;
    assert_eq!(ex.get_number("doubled"), Some(2.0));
    ex.emit("Tick", flux_runtime::RuntimeValue::Unit);
    assert_eq!(ex.get_number("doubled"), Some(4.0));
}

#[test]
fn imported_struct_type_is_usable_and_type_names_are_global() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { mk, Pt }\nfn main() -> Int {\n  let p = mk()\n  println(p.x)\n  0\n}\n",
            ),
            ("a.fl", "pub struct Pt { x: Int }\npub fn mk() -> Pt { Pt { x: 7 } }\n"),
        ],
        ".",
    );
    assert_eq!(out, "7\n");
    check_fails(
        &[
            (
                "main.fl",
                "import a { mk }\nimport b { mk2 }\nlet p = mk()\nlet q = mk2()\n",
            ),
            (
                "a.fl",
                "pub struct Pt { x: Int }\npub fn mk() -> Pt { Pt { x: 1 } }\n",
            ),
            (
                "b.fl",
                "pub struct Pt { y: Int }\npub fn mk2() -> Int { 2 }\n",
            ),
        ],
        ".",
        "duplicate definition of `Pt`",
    );
}
