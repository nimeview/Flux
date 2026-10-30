//! The README quick-start, end to end through the real `flux` binary.

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

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn init_check_run_fmt_from_root_and_subdirectory() {
    let dir = tempfile::tempdir().unwrap();
    let init = flux(&["init", "app"], dir.path());
    assert!(init.status.success(), "{}", text(&init.stderr));
    let app = dir.path().join("app");

    // A fresh project checks cleanly: no warnings in the template.
    let check = flux(&["check"], &app);
    assert!(check.status.success(), "{}", text(&check.stderr));
    assert_eq!(text(&check.stderr), "", "template should not warn");

    let run = flux(&["run", "--quiet"], &app);
    assert!(run.status.success(), "{}", text(&run.stderr));
    let root_stdout = text(&run.stdout);
    assert!(root_stdout.contains("count = 0"), "{root_stdout}");
    // `println` writes the string itself, not its quoted form.
    assert!(root_stdout.contains("doubled changed\n"), "{root_stdout}");
    assert!(
        !root_stdout.contains("\"doubled changed\""),
        "{root_stdout}"
    );

    // The entry point is found from a nested directory too.
    let nested = app.join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();
    let nested_run = flux(&["run", "--quiet"], &nested);
    assert!(nested_run.status.success(), "{}", text(&nested_run.stderr));
    assert_eq!(text(&nested_run.stdout), root_stdout);

    // Formatting the template is a no-op.
    let main = fs::read_to_string(app.join("main.fl")).unwrap();
    let fmt = flux(&["fmt", "main.fl"], &app);
    assert!(fmt.status.success(), "{}", text(&fmt.stderr));
    assert_eq!(text(&fmt.stdout), main);
}

#[test]
fn wasm_stub_fails_instead_of_reporting_success() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.fl"), "let x = 1\nlet y = x + 1\n").unwrap();
    let output = flux(&["wasm", "main.fl"], dir.path());
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("experimental stub"),
        "{}",
        text(&output.stderr)
    );
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "wasm wrote files"
    );
}

#[test]
fn run_without_manifest_explains_the_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let output = flux(&["run"], dir.path());
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("no flux.toml found in this directory or any parent"),
        "{}",
        text(&output.stderr)
    );
}
