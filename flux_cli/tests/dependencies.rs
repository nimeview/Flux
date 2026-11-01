//! `flux.toml` dependencies: local path dependencies resolve; anything that
//! would otherwise be dropped silently (a registry/version dependency, a
//! missing path, invalid TOML) is a diagnostic pointing into the manifest.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

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

/// `app/` depends on `lib/` (manifest written by the caller); `lib` exports
/// `twice` and keeps `hidden` private.
fn workspace(app_manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let (app, lib) = (dir.path().join("app"), dir.path().join("lib"));
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("flux.toml"), "[package]\nname = \"lib\"\n").unwrap();
    fs::write(
        lib.join("lib.fl"),
        "pub fn twice(x: Int) -> Int { x * 2 }\nfn hidden() -> Int { 9 }\n",
    )
    .unwrap();
    fs::write(app.join("flux.toml"), app_manifest).unwrap();
    fs::write(
        app.join("main.fl"),
        "import io { println }\nimport lib { twice }\nfn main() -> Int {\n  println(twice(21))\n  0\n}\n",
    )
    .unwrap();
    dir
}

const PATH_DEP: &str = "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n";

#[test]
fn path_dependency_resolves_and_runs() {
    let dir = workspace(PATH_DEP);
    let out = flux(&["run"], &dir.path().join("app"));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "42\n");
}

#[test]
fn private_items_of_a_dependency_cannot_be_imported() {
    let dir = workspace(PATH_DEP);
    let app = dir.path().join("app");
    fs::write(
        app.join("main.fl"),
        "import lib { hidden }\nlet a = hidden()\n",
    )
    .unwrap();
    let out = flux(&["check"], &app);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("`hidden` in module `lib` is private"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn version_dependency_is_an_error_not_silently_ignored() {
    let dir = workspace(&format!("{PATH_DEP}remote = \"1.2.0\"\n"));
    for cmd in ["check", "run"] {
        let out = flux(&[cmd], &dir.path().join("app"));
        let err = text(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{cmd}: {err}");
        assert!(
            err.contains("registry dependencies are not supported yet"),
            "{cmd}: {err}"
        );
        // Rendered against the manifest line that declares it.
        assert!(err.contains("flux.toml:6:1]"), "{cmd}: {err}");
        assert!(err.contains("remote = \"1.2.0\""), "{cmd}: {err}");
        assert!(text(&out.stdout).is_empty(), "{cmd} must not run");
    }
}

#[test]
fn missing_path_dependency_is_an_error() {
    let dir =
        workspace("[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../nope\" }\n");
    let out = flux(&["check"], &dir.path().join("app"));
    let err = text(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("path dependency `lib` not found"), "{err}");
    assert!(err.contains("flux.toml:5:1]"), "{err}");
}

#[test]
fn undeclared_package_is_not_importable() {
    let dir = workspace("[package]\nname = \"app\"\n");
    let out = flux(&["check"], &dir.path().join("app"));
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("module not found: lib"),
        "{}",
        text(&out.stderr)
    );
}
