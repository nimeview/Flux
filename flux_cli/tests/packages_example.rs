//! `examples/packages`: a library package (`geometry`) consumed by an app
//! (`shapes_app`) through a path dependency, with a committed `flux.lock`.
//! Everything runs with `--locked`, so the tests never write to the repo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn packages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/packages")
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

#[test]
fn app_runs_against_its_locked_dependency() {
    let app = packages().join("shapes_app");
    for engine in ["--vm", "--jit"] {
        let out = flux(&["run", "--locked", engine], &app);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{engine}: the committed flux.lock must match examples/packages/geometry: {}",
            text(&out.stderr)
        );
        assert_eq!(
            text(&out.stdout),
            "distance: 5.0\nsquare perimeter: 8.0\n",
            "{engine}"
        );
    }
}

#[test]
fn library_tests_pass() {
    let lib = packages().join("geometry");
    for engine in ["--vm", "--jit"] {
        let out = flux(&["test", "--locked", engine], &lib);
        let stdout = text(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{engine}: {stdout}{}",
            text(&out.stderr)
        );
        assert!(
            stdout.contains("test result: ok. 3 passed; 0 failed"),
            "{engine}: {stdout}"
        );
    }
}

#[test]
fn example_sources_are_formatted() {
    for file in [
        "shapes_app/main.fl",
        "geometry/lib.fl",
        "geometry/geometry_test.fl",
    ] {
        let out = flux(&["fmt", "--check", file], &packages());
        assert!(
            out.status.success(),
            "{file} is not fmt-clean: {}",
            text(&out.stderr)
        );
    }
}
