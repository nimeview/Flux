//! The library for command-line programs: `env`, `fs`, `path`, `process`, and
//! `io::read_stdin`. Programs run on both engines, which must agree.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn flux(dir: &Path, args: &[&str], stdin: &str) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(dir)
        .env("FLUX_OS_TEST", "set")
        .env_remove("FLUX_OS_UNSET")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flux");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait for flux");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Runs `src` as `main.fl` in a fresh directory on both engines.
fn both(src: &str, stdin: &str) -> (Option<i32>, String, String) {
    let mut results = Vec::new();
    for engine in ["--vm", "--jit"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.fl"), src).unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let (status, out, err) = flux(
            dir.path(),
            &["run", engine, "--no-cache", "--quiet", "main.fl"],
            stdin,
        );
        // Each engine runs in its own directory; messages name it.
        let here = dir.path().to_string_lossy().into_owned();
        results.push((status, out, err.replace(&here, "<dir>")));
    }
    assert_eq!(results[0], results[1], "{src}");
    results.remove(0)
}

#[test]
fn env_fs_path_and_stdin_work_and_report_failures_as_err() {
    let (status, stdout, stderr) = both(
        r#"import io { println, read_stdin }
import env
import fs
import path

fn main() {
    let set = env::env_var("FLUX_OS_TEST")
    let unset = env::env_var("FLUX_OS_UNSET")
    println("{set} {unset}")
    let entries = fs::list_dir(".")
    let sub = fs::is_dir("sub")
    let file = fs::is_dir("a.txt")
    println("{entries} {sub} {file}")
    let made = fs::create_dir("x/y")
    let there = fs::is_dir("x/y")
    let gone = fs::remove_dir("x/y")
    let file_gone = fs::remove_file("a.txt")
    let missing = fs::remove_dir("nope")
    let no_dir = fs::list_dir("nope")
    println("{made} {there} {gone} {file_gone} {missing} {no_dir}")
    let joined = path::path_join("a", "b.txt")
    let name = path::file_name("a/b.txt")
    let ext = path::extension("a/b.txt")
    let up = path::parent("a/b.txt")
    let top = path::parent("b")
    let plain = path::extension("Makefile")
    println("{joined} {name} {ext} {up} {top} {plain}")
    let input = read_stdin()
    println("{input}")
}
"#,
        "one\ntwo",
    );
    assert_eq!(status, Some(0), "{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "Some(\"set\") None");
    assert_eq!(lines[1], "Ok([\"a.txt\", \"main.fl\", \"sub\"]) true false");
    assert!(
        lines[2].starts_with("Ok(()) true Ok(()) Ok(()) Err(\"nope: ")
            && lines[2].contains(") Err(\"nope: "),
        "{}",
        lines[2]
    );
    assert_eq!(
        lines[3],
        "a/b.txt Some(\"b.txt\") Some(\"txt\") Some(\"a\") None None"
    );
    assert_eq!(&lines[4..], ["Ok(\"one", "two\")"]);
}

#[test]
fn run_gives_the_exit_code_and_both_outputs() {
    let (status, stdout, stderr) = both(
        r#"import io { println }
import process { run }

fn main() {
    match run("sh", ["-c", "echo out; echo err >&2; exit 3"]) {
        Ok((code, out, err)) => println("{code} {out.trim()} {err.trim()}"),
        Err(e) => println("failed: {e}"),
    }
    let missing = run("no-such-program-for-flux", [])
    println("{missing.is_err()}")
}
"#,
        "",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "3 out err\ntrue\n");
}

#[test]
fn exit_stops_the_program_with_its_status_from_anywhere() {
    let exit = |src: &str| both(src, "");
    // From `main`, nothing after it runs.
    let (status, stdout, _) = exit(
        "import io { println }\nimport process { exit }\nfn main() {\n    println(\"a\")\n    exit(3)\n    println(\"b\")\n}\n",
    );
    assert_eq!((status, stdout.as_str()), (Some(3), "a\n"));
    // While the module initializes.
    let (status, stdout, _) = exit(
        "import io { println }\nimport process { exit }\nif true {\n    exit(4)\n}\nfn main() {\n    println(\"main\")\n}\n",
    );
    assert_eq!((status, stdout.as_str()), (Some(4), ""));
    // From a handler, in the middle of a batch.
    let (status, stdout, _) = exit(
        "import io { println }\nimport process { exit }\nevent Stop(code: Int)\non Stop(code) {\n    exit(code)\n}\nact fn main() {\n    println(\"before\")\n    emit Stop(5)\n    println(\"after\")\n}\n",
    );
    assert_eq!((status, stdout.as_str()), (Some(5), "before\n"));
    // From a callback: the first call wins, and no later one runs.
    let (status, _, _) = exit(
        "import process { exit }\nimport collections { map }\nfn main() {\n    let xs = map([3, 4, 5], exit)\n}\n",
    );
    assert_eq!(status, Some(3));
    // A status outside 0..=255 is a runtime error, even before a valid one.
    let (status, _, stderr) = exit(
        "import process { exit }\nimport collections { map }\nfn main() {\n    let xs = map([256, 0], exit)\n}\n",
    );
    assert_eq!(status, Some(1));
    assert!(stderr.contains("exit status must be 0..=255"), "{stderr}");
}

#[test]
fn a_failing_callback_stops_map_filter_and_reduce() {
    let (status, stdout, stderr) = both(
        "import io { println }\nimport collections { map }\nfn show(x: Int) -> Int {\n    println(\"{x}\")\n    10 / x\n}\nfn main() {\n    let ys = map([1, 0, 2], show)\n}\n",
        "",
    );
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(
        stdout, "1\n0\n",
        "the call after the failing one must not run"
    );
}

#[test]
fn os_functions_are_side_effects_and_path_functions_are_pure() {
    let (status, _, stderr) = both(
        "import env { env_var }\nderived home = env_var(\"HOME\")\nfn main() {\n    let h = snap(home)\n}\n",
        "",
    );
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("side effect `env_var` is not allowed in a derived expression"),
        "{stderr}"
    );
    let (status, stdout, stderr) = both(
        "import io { println }\nimport fs\nimport path { file_name }\nconst NAME = file_name(\"a/b.txt\")\nfn main() {\n    let list = fs::list_dir\n    let sub = list(\"sub\")\n    println(\"{NAME} {sub}\")\n}\n",
        "",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "Some(\"b.txt\") Ok([])\n");
}

#[test]
fn flux_test_reports_exit_without_leaking_it_into_other_tests() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("flux.toml"), "[package]\nname = \"t\"\n").unwrap();
    std::fs::write(
        dir.path().join("a_test.fl"),
        "import process { exit }\nfn test_exits() {\n    exit(9)\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b_test.fl"),
        "import math { assert }\nfn test_fails() {\n    assert(false)\n}\n",
    )
    .unwrap();
    let (status, stdout, _) = flux(dir.path(), &["test"], "");
    assert_eq!(status, Some(1));
    assert!(
        stdout.contains("test a_test.fl::test_exits ... FAILED: called exit(9)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("test b_test.fl::test_fails ... FAILED: runtime error: assertion failed"),
        "{stdout}"
    );
}

#[test]
fn exit_in_the_repl_ends_it_with_that_status() {
    let dir = tempfile::tempdir().unwrap();
    let (status, _, _) = flux(
        dir.path(),
        &["repl"],
        "import process { exit }\nlet mut x = 0\nwatch x {\n    if x > 0 {\n        exit(7)\n    }\n}\nevent Go\non Go {\n    x = 1\n}\n:emit Go\n",
    );
    assert_eq!(status, Some(7));
}

/// 0.4: `Bytes`: built from numbers or text, indexed as `UInt8`, sliced,
/// read back as UTF-8, and written to and read from a file.
#[test]
fn bytes_are_built_indexed_and_round_trip_through_a_file() {
    let (status, stdout, stderr) = both(
        "import io { println, read_bytes, write_bytes }\nimport collections { bytes }\n\nfn main() {\n    let missing = \"missing.bin\"\n    let b = bytes([72, 105, 0, 255])\n    println(\"{b} {b.len()} {b[1]} {b[3] >> 4} {b.slice(0, 2).to_text()}\")\n    let t = \"héllo\".to_bytes()\n    println(\"{t.len()} {t.to_text()} {b.to_text()}\")\n    match write_bytes(\"out.bin\", b) {\n        Ok(_) => println(\"written\"),\n        Err(e) => println(e)\n    }\n    match read_bytes(\"out.bin\") {\n        Ok(r) => println(\"{r == b} {r.len()}\"),\n        Err(e) => println(e)\n    }\n    let gone = read_bytes(missing).is_err()\n    println(\"{gone}\")\n}\n",
        "",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "Bytes[72, 105, 0, 255] 4 105 15 Ok(\"Hi\")\n6 Ok(\"héllo\") Err(\"not UTF-8 text at byte 3\")\nwritten\ntrue 4\ntrue\n"
    );
    let (status, _, stderr) = both(
        "import collections { bytes }\nfn main() {\n    let b = bytes([300])\n}\n",
        "",
    );
    assert_eq!(status, Some(1));
    assert!(stderr.contains("numeric cast out of range"), "{stderr}");
}
