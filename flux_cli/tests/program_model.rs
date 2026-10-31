//! The CLI program model: an optional `fn main` entry point owns stdout and
//! sets the exit status; `eprint`/`eprintln` write stderr; `args()` reads the
//! arguments after `--`. Programs without `main` keep the reactive dump.

use std::fs;
use std::process::{Command, Output};

fn run(src: &str, engine: &str, extra: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.fl"), src).unwrap();
    let mut args = vec!["run", engine, "main.fl"];
    args.extend_from_slice(extra);
    Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(&args)
        .current_dir(dir.path())
        .output()
        .expect("spawn flux")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const CLI: &str = "\
import io { println, eprintln, args }
import collections { len, get }
import str { to_string }

const GREETING = \"hello\"

fn main() -> Int {
    let a = args()
    println(GREETING + \", \" + to_string(len(a)) + \" args\")
    match get(a, 0) {
        Some(first) => {
            println(\"first: \" + first)
            0
        },
        None => {
            eprintln(\"usage: main.fl <name>\")
            2
        }
    }
}
";

#[test]
fn main_owns_stdout_and_sets_the_exit_status() {
    for engine in ["--vm", "--jit"] {
        let ok = run(CLI, engine, &["--", "alice", "bob"]);
        assert_eq!(ok.status.code(), Some(0), "{engine}: {}", text(&ok.stderr));
        // Only the program's own output: no banner, no binding dump.
        assert_eq!(
            text(&ok.stdout),
            "hello, 2 args\nfirst: alice\n",
            "{engine}"
        );

        let usage = run(CLI, engine, &[]);
        assert_eq!(usage.status.code(), Some(2), "{engine}");
        assert_eq!(text(&usage.stdout), "hello, 0 args\n", "{engine}");
        assert_eq!(text(&usage.stderr), "usage: main.fl <name>\n", "{engine}");
    }
}

#[test]
fn unit_main_exits_zero() {
    let out = run(
        "import io { println }\nfn main() {\n  println(\"done\")\n}\n",
        "--vm",
        &[],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "done\n");
}

#[test]
fn main_failures_exit_one_with_a_message() {
    let big = run("fn main() -> Int { 300 }\n", "--vm", &[]);
    assert_eq!(big.status.code(), Some(1));
    assert!(
        text(&big.stderr).contains("0..=255"),
        "{}",
        text(&big.stderr)
    );

    for engine in ["--vm", "--jit"] {
        let trap = run(
            "fn main() -> Int {\n  let z = 0\n  10 / z\n}\n",
            engine,
            &[],
        );
        assert_eq!(trap.status.code(), Some(1), "{engine}");
        assert!(
            text(&trap.stderr).contains("integer division by zero in `main`"),
            "{engine}: {}",
            text(&trap.stderr)
        );
    }
}

#[test]
fn main_with_parameters_is_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.fl"), "fn main(x: Int) -> Int { x }\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["check", "main.fl"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("fn main()"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn programs_without_main_keep_the_reactive_dump() {
    let out = run(
        "let mut count = 1\nlet doubled = count * 2\n",
        "--vm",
        &["--", "x"],
    );
    assert_eq!(out.status.code(), Some(0));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("count = 1"), "{stdout}");
    assert!(stdout.contains("doubled = 2"), "{stdout}");
}

#[test]
fn const_is_readable_in_a_pure_function_but_let_mut_is_not() {
    let ok = run(
        "import io { println }\nconst LIMIT = 3\nfn main() -> Int {\n  println(LIMIT * 2)\n  0\n}\n",
        "--vm",
        &[],
    );
    assert_eq!(ok.status.code(), Some(0), "{}", text(&ok.stderr));
    assert_eq!(text(&ok.stdout), "6\n");

    let reactive = run(
        "let mut c = 0\nfn f() -> Int { c }\nfn main() -> Int { f() }\n",
        "--vm",
        &[],
    );
    assert_eq!(reactive.status.code(), Some(1));
    assert!(text(&reactive.stderr).contains("cannot read reactive `c`"));
}
