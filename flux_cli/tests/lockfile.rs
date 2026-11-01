//! `flux.lock`: written for projects with path dependencies, pinning every
//! package of the transitive tree by content; `--locked` refuses to run
//! when the lock is missing or stale.

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

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[
            ("app/flux.toml", "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n"),
            (
                "app/main.fl",
                "import io { println }\nimport lib { twice }\nfn main() -> Int {\n  println(twice(5))\n  0\n}\n",
            ),
            ("lib/flux.toml", "[package]\nname = \"lib\"\n\n[dependencies]\nbase = { path = \"../base\" }\n"),
            ("lib/lib.fl", "import base { one }\npub fn twice(x: Int) -> Int { x * 2 + one() }\n"),
            ("base/flux.toml", "[package]\nname = \"base\"\n"),
            ("base/lib.fl", "pub fn one() -> Int { 1 }\n"),
        ],
    );
    dir
}

#[test]
fn run_writes_a_deterministic_lock_of_the_transitive_tree() {
    let dir = workspace();
    let app = dir.path().join("app");
    let out = flux(&["run"], &app);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let lock = fs::read_to_string(app.join("flux.lock")).expect("lock written");
    assert!(
        lock.contains("name = \"base\"\npath = \"../base\"\nchecksum = \"sha256:"),
        "{lock}"
    );
    assert!(
        lock.contains("name = \"lib\"\npath = \"../lib\"\nchecksum = \"sha256:"),
        "{lock}"
    );
    assert!(
        lock.find("\"base\"") < lock.find("\"lib\""),
        "sorted by path: {lock}"
    );

    // Re-running leaves it byte-for-byte unchanged and `--locked` accepts it.
    assert!(flux(&["check"], &app).status.success());
    assert_eq!(fs::read_to_string(app.join("flux.lock")).unwrap(), lock);
    for cmd in [["check", "--locked"], ["run", "--locked"]] {
        let out = flux(&cmd, &app);
        assert_eq!(out.status.code(), Some(0), "{cmd:?}: {}", text(&out.stderr));
    }
}

#[test]
fn locked_refuses_a_missing_or_stale_lock() {
    let dir = workspace();
    let app = dir.path().join("app");

    let missing = flux(&["run", "--locked"], &app);
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        text(&missing.stderr).contains("flux.lock is missing"),
        "{}",
        text(&missing.stderr)
    );
    assert!(text(&missing.stdout).is_empty(), "nothing may run");
    assert!(!app.join("flux.lock").exists(), "--locked never writes");

    assert!(flux(&["check"], &app).status.success());
    fs::write(
        dir.path().join("base/lib.fl"),
        "pub fn one() -> Int { 2 }\n",
    )
    .unwrap();
    for cmd in ["check", "run", "test"] {
        let out = flux(&[cmd, "--locked"], &app);
        let err = text(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{cmd}: {err}");
        assert!(
            err.contains("flux.lock is out of date (`base` changed)"),
            "{cmd}: {err}"
        );
    }

    // Without --locked the lock is refreshed and the new source runs.
    let out = flux(&["run"], &app);
    assert_eq!(text(&out.stdout), "12\n", "{}", text(&out.stderr));
    assert!(flux(&["run", "--locked"], &app).status.success());
}

#[test]
fn removing_a_dependency_is_reported_as_drift() {
    let dir = workspace();
    let app = dir.path().join("app");
    assert!(flux(&["check"], &app).status.success());
    fs::write(
        dir.path().join("lib/flux.toml"),
        "[package]\nname = \"lib\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("lib/lib.fl"),
        "pub fn twice(x: Int) -> Int { x * 2 }\n",
    )
    .unwrap();
    let out = flux(&["check", "--locked"], &app);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("`base` removed"), "{err}");
    assert!(err.contains("`lib` changed"), "{err}");
}

#[test]
fn projects_without_dependencies_get_no_lock() {
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[
            ("flux.toml", "[package]\nname = \"solo\"\n"),
            ("main.fl", "let x = 1\n"),
        ],
    );
    assert!(flux(&["check", "--locked"], dir.path()).status.success());
    assert!(!dir.path().join("flux.lock").exists());
}
