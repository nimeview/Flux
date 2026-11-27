//! Defects fixed before 0.4.0: a file beside the importer could take a
//! library module's place; `-128i8` failed at run time; `fmt` rewrote
//! suffixed literals as casts.

use std::path::Path;
use std::process::Command;

fn flux(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn flux");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// `import collections` means the library, whatever files lie beside the
/// importer; a file of that name gets a warning (not for the file itself).
#[test]
fn a_library_module_name_means_the_library() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("collections.fl"),
        "import collections { len }\nimport io { println }\n\nfn main() {\n    println(\"{len([1, 2, 3])}\")\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "import collections { len }\nimport io { println }\n\nfn main() {\n    println(\"{len([1, 2])}\")\n}\n",
    )
    .unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["run", "--quiet", "collections.fl"]);
    assert_eq!((status, stdout.as_str()), (Some(0), "3\n"), "{stderr}");
    assert!(!stderr.contains("E035"), "{stderr}");
    let (status, stdout, stderr) = flux(dir.path(), &["run", "--quiet", "main.fl"]);
    assert_eq!((status, stdout.as_str()), (Some(0), "2\n"), "{stderr}");
    assert!(
        stderr.contains("E035") && stderr.contains("`collections.fl` is not imported"),
        "{stderr}"
    );
}

const LITERALS: &str = "import io { println }\n\nfn main() {\n    let a = 120i8\n    let b = -128i8\n    let c = 2.5f32\n    let d = 7 as Int8\n    let e = -5i64 - 3\n    let f = -(3i8)\n    println(\"{a} {b} {c} {d} {e} {f} {-1_000i32}\")\n}\n";

/// `-128i8` is the literal -128 of type `Int8`, on both engines.
#[test]
fn negative_suffixed_literals() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), LITERALS).unwrap();
    for engine in ["--vm", "--jit"] {
        let (status, stdout, stderr) = flux(
            dir.path(),
            &["run", engine, "--no-cache", "--quiet", "main.fl"],
        );
        assert_eq!(status, Some(0), "{engine}: {stderr}");
        assert_eq!(stdout, "120 -128 2.5 7 -8 -3 -1000\n", "{engine}");
    }
}

/// `fmt` keeps a suffixed literal as written, and a cast as a cast.
#[test]
fn fmt_keeps_suffixed_literals() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), LITERALS).unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, LITERALS);
}
