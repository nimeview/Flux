//! CLI-level regression tests: malformed input must fail with a diagnostic and
//! a nonzero exit code; valid input must still succeed.

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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_fails_with_parse_error(output: &Output, file_name: &str) {
    let err = stderr(output);
    assert!(
        !output.status.success(),
        "expected nonzero exit, got {:?}; stderr={err}",
        output.status
    );
    assert!(err.contains("parse error"), "missing parse error: {err}");
    assert!(
        err.contains(file_name),
        "diagnostic should name `{file_name}`: {err}"
    );
    assert!(
        !stdout(output).contains("ok"),
        "failure must not print success: {}",
        stdout(output)
    );
}

#[test]
fn syntax_error_in_root_file_fails_check_and_run() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bad.fl"), "let x =\n").unwrap();

    for cmd in ["check", "run"] {
        let output = flux(&[cmd, "bad.fl"], dir.path());
        assert_fails_with_parse_error(&output, "bad.fl");
    }
}

#[test]
fn syntax_error_in_imported_module_fails_check_and_run() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.fl"), "import util\nlet y = 1\n").unwrap();
    fs::write(dir.path().join("util.fl"), "pub fn f( -> Int { 1 }\n").unwrap();

    for cmd in ["check", "run"] {
        let output = flux(&[cmd, "main.fl"], dir.path());
        assert_fails_with_parse_error(&output, "util.fl");
        // The snippet must come from the imported file, not the root file.
        assert!(
            stderr(&output).contains("pub fn f("),
            "diagnostic should show the imported source: {}",
            stderr(&output)
        );
    }
}

#[test]
fn missing_file_fails_check_and_run() {
    let dir = tempfile::tempdir().unwrap();
    for cmd in ["check", "run"] {
        let output = flux(&[cmd, "missing.fl"], dir.path());
        assert!(!output.status.success(), "{cmd} should fail");
        assert!(
            stderr(&output).contains("missing.fl"),
            "{cmd}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn valid_program_succeeds_check_and_run() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.fl"),
        "import util\nlet y = util.f() + 1\n",
    )
    .unwrap();
    fs::write(dir.path().join("util.fl"), "pub fn f() -> Int { 1 }\n").unwrap();

    let check = flux(&["check", "main.fl"], dir.path());
    assert!(check.status.success(), "check: {}", stderr(&check));
    assert!(stdout(&check).contains("ok"));

    let run = flux(&["run", "--no-cache", "main.fl"], dir.path());
    assert!(run.status.success(), "run: {}", stderr(&run));
    assert!(stdout(&run).contains("y = 2"), "run: {}", stdout(&run));
}
