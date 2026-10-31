//! `flux test`: discovers `fn test_*()` in the project's `.fl` files, runs
//! each in isolation, reports failures with their reason, and exits non-zero
//! when anything fails. Also: assertions are runtime errors, never panics.

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

const TESTS: &str = "\
import math { assert, assert_eq }
import calc { add }
fn test_add() {
  assert_eq(add(2, 2), 4)
}
fn test_bool() -> Bool { add(1, 1) == 2 }
fn test_false() -> Bool { add(1, 1) == 3 }
fn test_fail() {
  assert_eq(add(2, 2), 5)
}
fn test_div() -> Int {
  let z = 0
  1 / z
}
fn main() -> Int { 9 }
";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[
            ("flux.toml", "[package]\nname = \"t\"\n"),
            (
                "src/calc.fl",
                "pub fn add(a: Int, b: Int) -> Int { a + b }\n",
            ),
            ("src/calc_test.fl", TESTS),
            (".hidden/skip.fl", "fn test_hidden() -> Bool { false }\n"),
        ],
    );
    dir
}

#[test]
fn reports_each_test_and_fails_the_run_on_any_failure() {
    let dir = project();
    for engine in ["--vm", "--jit"] {
        let out = flux(&["test", engine], dir.path());
        let stdout = text(&out.stdout);
        assert_eq!(out.status.code(), Some(1), "{engine}: {stdout}");
        for line in [
            "test src/calc_test.fl::test_add ... ok",
            "test src/calc_test.fl::test_bool ... ok",
            "test src/calc_test.fl::test_false ... FAILED: returned false",
            "test src/calc_test.fl::test_fail ... FAILED: runtime error: assertion failed: 4 != 5 in `test_fail`",
            "test src/calc_test.fl::test_div ... FAILED: runtime error: integer division by zero in `test_div`",
            "test result: FAILED. 2 passed; 3 failed",
        ] {
            assert!(stdout.lines().any(|l| l == line), "{engine}: missing `{line}` in\n{stdout}");
        }
        // Hidden directories are skipped; `main` is never run.
        assert!(!stdout.contains("test_hidden"), "{stdout}");
    }
}

#[test]
fn filter_selects_tests_and_a_clean_run_exits_zero() {
    let dir = project();
    let out = flux(&["test", "add"], dir.path());
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("test src/calc_test.fl::test_add ... ok"),
        "{stdout}"
    );
    assert!(
        stdout.contains("test result: ok. 1 passed; 0 failed"),
        "{stdout}"
    );

    let none = flux(&["test", "nothing-matches"], dir.path());
    assert_eq!(none.status.code(), Some(0));
    assert_eq!(text(&none.stdout), "no tests found\n");
}

#[test]
fn a_test_file_that_does_not_compile_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[("broken_test.fl", "fn test_x() -> Int { undefined_name }\n")],
    );
    let out = flux(&["test"], dir.path());
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stdout).contains("broken_test.fl ... FAILED: does not compile"));
    assert!(
        text(&out.stderr).contains("undefined name"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn failed_assertion_in_run_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[(
            "main.fl",
            "import math { assert_eq }\nfn main() -> Int {\n  assert_eq(1 + 1, 3)\n  0\n}\n",
        )],
    );
    for engine in ["--vm", "--jit"] {
        let out = flux(&["run", engine, "main.fl"], dir.path());
        assert_eq!(out.status.code(), Some(1), "{engine}");
        let err = text(&out.stderr);
        assert!(
            err.contains("assertion failed: 2 != 3 in `main`"),
            "{engine}: {err}"
        );
        assert!(!err.contains("panicked"), "{engine}: {err}");
    }
}
