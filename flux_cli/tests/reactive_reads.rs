//! Reading a derived value gives its current value, also in the middle of a
//! handler that has just changed one of its sources. Every program runs on both
//! engines, which must agree.

use std::path::Path;
use std::process::Command;

fn run(dir: &Path, engine: &str) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["run", engine, "--no-cache", "--quiet", "main.fl"])
        .current_dir(dir)
        .output()
        .expect("spawn flux");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn both(src: &str) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let vm = run(dir.path(), "--vm");
    assert_eq!(vm, run(dir.path(), "--jit"), "{src}");
    vm
}

fn prints(src: &str, want: &str) {
    let (status, stdout, stderr) = both(src);
    assert_eq!(status, Some(0), "{src}\n{stderr}");
    assert_eq!(stdout, want, "{src}");
}

#[test]
fn a_handler_reads_the_new_derived_value() {
    prints(
        "import io { println }\nevent T\nlet mut a = 1\nlet d = a * 10\non T {\n    a = 2\n    println(\"d={d}\")\n}\nfn main() {\n    emit T\n}\n",
        "d=20\n",
    );
}

#[test]
fn a_chain_of_derived_values_is_current() {
    prints(
        "import io { println }\nevent T\nlet mut a = 1\nlet d = a * 10\nlet e = d + 1\nlet f = e * 2\non T {\n    a = 2\n    println(\"{f} {e} {d}\")\n    a = 3\n    println(\"{f}\")\n}\nfn main() {\n    emit T\n}\n",
        "42 21 20\n62\n",
    );
}

#[test]
fn a_nested_handler_sees_the_outer_writes() {
    prints(
        "import io { println }\nevent T\nevent U\nlet mut a = 1\nlet d = a * 10\non T {\n    a = 2\n    emit U\n    println(\"T after U: d={d}\")\n}\non U {\n    println(\"U sees a={a} d={d}\")\n}\nfn main() {\n    emit T\n}\n",
        "U sees a=2 d=20\nT after U: d=20\n",
    );
}

#[test]
fn a_dynamic_branch_reads_what_it_takes_now() {
    prints(
        "import io { println }\nevent T\nlet mut c = true\nlet mut a = 1\nlet mut b = 2\nlet d = if c { a } else { b }\non T {\n    c = false\n    b = 5\n    println(\"d={d}\")\n    c = true\n    a = 7\n    println(\"d={d}\")\n}\nfn main() {\n    emit T\n}\n",
        "d=5\nd=7\n",
    );
}

#[test]
fn a_tracked_function_reads_current_values() {
    prints(
        "import io { println }\nevent T\nlet mut a = 1\nlet d = a * 10\nrfn twice() -> Int {\n    d * 2\n}\non T {\n    a = 3\n    println(\"{twice()}\")\n}\nfn main() {\n    emit T\n}\n",
        "60\n",
    );
}

#[test]
fn a_derived_value_nobody_reads_is_not_computed_mid_handler() {
    // `10 / a` with `a = 0` would fail, but nothing reads it before `a` changes again.
    prints(
        "import io { println }\nevent T\nlet mut a = 1\nlet d = 10 / a\nwatch d {\n    println(\"d={d}\")\n}\non T {\n    a = 0\n    a = 5\n}\nfn main() {\n    emit T\n}\n",
        "d=10\nd=2\n",
    );
}

#[test]
fn reading_a_failing_derived_value_fails_at_the_read() {
    let (status, stdout, stderr) = both(
        "import io { println }\nevent T\nlet mut a = 1\nlet d = 10 / a\non T {\n    a = 0\n    println(\"before\")\n    println(\"{d}\")\n    println(\"after\")\n}\nfn main() {\n    emit T\n}\n",
    );
    assert_eq!(status, Some(1));
    assert_eq!(stdout, "before\n");
    assert!(stderr.contains("integer division by zero"), "{stderr}");
}

#[test]
fn watches_still_run_once_after_the_handler() {
    prints(
        "import io { println }\nevent T\nlet mut a = 1\nlet mut b = 1\nlet sum = a + b\nwatch sum {\n    println(\"sum={sum}\")\n}\non T {\n    a = 2\n    println(\"read {sum}\")\n    b = 3\n}\nfn main() {\n    emit T\n}\n",
        "sum=2\nread 3\nsum=5\n",
    );
}

#[test]
fn equal_values_by_the_language_are_not_a_change() {
    // `0.0` and `-0.0` are equal under `==`, so the watch does not run again.
    prints(
        "import io { println }\nevent T\nlet mut x = 0.0\nlet y = x * 1.0\nwatch y {\n    println(\"y={y}\")\n}\non T {\n    x = -0.0\n}\nfn main() {\n    emit T\n    println(\"end\")\n}\n",
        "y=0.0\nend\n",
    );
}

#[test]
fn a_compiled_handler_reads_the_new_derived_value() {
    // No output in the handler, so the JIT may compile it.
    prints(
        "import io { println }\nevent T\nlet mut a = 1\nlet mut b = 0\nlet d = a * 10\non T {\n    a = 2\n    b = d + 1\n}\nwatch b {\n    println(\"b={b}\")\n}\nfn main() {\n    emit T\n}\n",
        "b=0\nb=21\n",
    );
}
