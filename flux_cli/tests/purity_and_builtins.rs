//! Holes found in the 0.2.0 review: derived values and `const` initializers
//! stay free of side effects and `snap` however they are reached, library
//! functions keep their checked types, and `read_line` reports end of input.
//! Every program runs on both engines, which must agree.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn flux(dir: &Path, args: &[&str], stdin: &str) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(dir)
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
    let output = child.wait_with_output().expect("wait for flux");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn both(src: &str, stdin: &str) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    std::fs::write(dir.path().join("data.txt"), "data").unwrap();
    let vm = flux(
        dir.path(),
        &["run", "--vm", "--no-cache", "--quiet", "main.fl"],
        stdin,
    );
    let jit = flux(
        dir.path(),
        &["run", "--jit", "--no-cache", "--quiet", "main.fl"],
        stdin,
    );
    assert_eq!(vm, jit, "{src}");
    vm
}

fn prints(src: &str, want: &str) {
    let (status, stdout, stderr) = both(src, "");
    assert_eq!(status, Some(0), "{src}\n{stderr}");
    assert_eq!(stdout, want, "{src}");
}

fn rejects(src: &str, needle: &str) {
    let (status, stdout, stderr) = both(src, "");
    assert_eq!(status, Some(1), "{src}");
    assert!(stdout.is_empty(), "nothing may run: {stdout}");
    assert!(stderr.contains(needle), "{src}\n{stderr}");
}

#[test]
fn derived_values_reject_effects_and_snap_however_reached() {
    // A side-effecting library function passed as a value.
    rejects(
        "import io { println }\nderived bad = [\"oops\"].map(println)\nfn main() {\n    let x = snap(bad)\n}\n",
        "side effect `println` is not allowed in a derived expression",
    );
    // A method with a side effect.
    rejects(
        "import io { println }\nstruct P {\n    n: Int\n}\nimpl P {\n    fn shout(self) -> Int {\n        println(\"loud\")\n        self.n\n    }\n}\nconst p = P { n: 1 }\nderived bad = p.shout()\nfn main() {\n    let x = snap(bad)\n}\n",
        "side effect `shout` is not allowed in a derived expression",
    );
    // `snap` inside a function: the derived value would not follow `x`.
    rejects(
        "let mut x = 1\nfn read() -> Int {\n    snap(x)\n}\nderived y = read()\nfn main() {\n    let z = snap(y)\n}\n",
        "`read` reads with `snap()`, so a derived value cannot call it",
    );
    // A pure method and a pure function are fine.
    prints(
        "import io { println }\nstruct P {\n    n: Int\n}\nimpl P {\n    fn twice(self) -> Int {\n        self.n * 2\n    }\n}\nlet mut p = P { n: 2 }\nderived d = p.twice() + [1, 2].reduce(0, |a, b| a + b)\nfn main() {\n    println(\"{snap(d)}\")\n}\n",
        "7\n",
    );
}

#[test]
fn const_initializers_have_no_side_effects() {
    rejects(
        "import io { println, read_file }\nconst C = read_file(\"data.txt\")\nfn main() {\n    println(\"{C}\")\n}\n",
        "side effect `read_file` is not allowed in a `const` initializer",
    );
    prints(
        "import io { println }\nfn square(n: Int) -> Int {\n    n * n\n}\nconst C = square(4) + 1\nfn main() {\n    println(\"{C}\")\n}\n",
        "17\n",
    );
}

#[test]
fn library_functions_keep_their_types() {
    rejects(
        "import collections { reduce }\nfn main() {\n    let x: Float = reduce([1.5, 2.5], 0.0)\n}\n",
        "takes 3 arguments but 2 were given",
    );
    rejects(
        "import math { sqrt }\nfn main() {\n    let f = sqrt\n}\n",
        "`sqrt`, a library function that can only be called",
    );
    // A library function with one type still works as a value.
    prints(
        "import io { println }\nfn main() {\n    let printed = [\"a\", \"b\"].map(println)\n}\n",
        "a\nb\n",
    );
}

#[test]
fn an_integer_with_float32_stays_float32() {
    prints(
        "import io { println }\nfn main() {\n    let x: Float32 = 16777217 + 0.0f32\n    let y: Float = 16777217 + 0.0\n    println(\"{x} {y}\")\n}\n",
        "16777216.0 16777217.0\n",
    );
}

#[test]
fn read_line_reports_the_end_of_input() {
    let (status, stdout, stderr) = both(
        "import io { println, read_line }\nfn main() {\n    let mut n = 0\n    let mut going = true\n    while going {\n        match read_line() {\n            Ok(line) => {\n                n += 1\n            },\n            Err(e) => {\n                println(\"{n} lines, then {e}\")\n                going = false\n            },\n        }\n    }\n}\n",
        "a\n\nb\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "3 lines, then end of input\n");
}

#[test]
fn a_trait_method_without_a_semicolon_says_so() {
    rejects(
        "trait Area {\n    fn area(self) -> Float\n}\nfn main() {}\n",
        "a trait method is only a signature and ends with `;`",
    );
}

#[test]
fn functions_are_followed_through_what_holds_them() {
    // A binding, a tuple, and a later assignment holding a function with
    // effects; calling it in a derived value or a `const` is an error.
    for (src, needle) in [
        (
            "import io { println }\nlet output = println\nderived y = output(\"LEAK\")\nfn main() {}\n",
            "side effect `output`",
        ),
        (
            "import io { println }\nfn loud(s: String) {\n    println(s)\n}\nlet pair = (loud, 1)\nderived y = (pair.0)(\"LEAK\")\nfn main() {}\n",
            "side effect `pair`",
        ),
        (
            "import io { println }\nlet output = println\nfn call(s: String) {\n    snap(output)(s)\n}\nconst C = call(\"LEAK\")\nfn main() {}\n",
            "side effect `call` is not allowed in a `const` initializer",
        ),
        (
            "import io { println }\nfn quiet(s: String) -> Int {\n    1\n}\nfn loud(s: String) -> Int {\n    println(s)\n    1\n}\nlet mut cb = quiet\nderived y = cb(\"LEAK\")\nact fn change() {\n    cb = loud\n}\nfn main() {\n    change()\n}\n",
            "side effect `cb`",
        ),
        (
            "import io { println }\nfn loud(s: String) -> Int {\n    println(s)\n    1\n}\nfn answer() -> Int {\n    let f = loud\n    f(\"x\")\n}\nderived y = answer()\nfn main() {}\n",
            "side effect `answer`",
        ),
    ] {
        rejects(src, needle);
    }
}

#[test]
fn holding_a_function_or_an_unrelated_method_is_not_running_it() {
    // `len` on an array is not `P.len`; a function only stored is not run;
    // a `const` may hold a function with effects.
    prints(
        "import io { println }\nstruct P {\n    n: Int\n}\nimpl P {\n    fn len(self) -> Int {\n        println(\"unused\")\n        self.n\n    }\n}\nconst N = [1, 2].len()\nlet mut x = 1\nfn read() -> Int {\n    snap(x)\n}\nfn answer() -> Int {\n    let held = read\n    let wrapped = |n: Int| read() + n\n    42\n}\nderived y = answer()\nconst output = println\nfn main() {\n    output(\"{N} {snap(y)}\")\n}\n",
        "2 42\n",
    );
}

#[test]
fn functions_are_followed_through_closures_returns_arms_and_events() {
    let io = "import io { println }\n";
    for (code, needle) in [
        // A closure capturing a local that holds `println`.
        (
            "fn run() -> Int {\n    let f = println\n    let g = |s: String| f(s)\n    g(\"LEAK\")\n    1\n}\nderived y = run()\nfn main() {}\n",
            "side effect `run`",
        ),
        // A function returned by a call.
        (
            "fn factory() -> fn(String) {\n    println\n}\nlet output = factory()\nderived y = output(\"LEAK\")\nfn main() {}\n",
            "side effect `output`",
        ),
        // A `let` in a `match` arm block.
        (
            "const f = println\nconst C = match 1 {\n    _ => {\n        let g = f\n        g(\"LEAK\")\n    }\n}\nfn main() {}\n",
            "side effect `g` is not allowed in a `const` initializer",
        ),
        // A function passed through an event to a handler that stores it.
        (
            "fn quiet(s: String) {}\nevent Set(f: fn(String))\nlet mut cb = quiet\nderived y = cb(\"LEAK\")\non Set(f) {\n    cb = f\n}\nfn main() {\n    emit Set(println)\n}\n",
            "side effect `cb`",
        ),
    ] {
        rejects(&format!("{io}{code}"), needle);
    }
    // Making a function value runs nothing.
    prints(
        "import io { println }\nfn factory() -> fn(String) {\n    println\n}\nconst output = factory()\nfn main() {\n    output(\"ok\")\n}\n",
        "ok\n",
    );
}

#[test]
fn calling_a_function_in_a_field_never_runs_a_method_of_another_type() {
    let (status, stdout, stderr) = both(
        "import io { println }\nstruct P {\n    n: Int\n}\nimpl P {\n    fn f(self, x: Int) -> Int {\n        println(\"wrong\")\n        x\n    }\n}\nstruct Op {\n    f: fn(Int) -> Int\n}\nfn quiet(x: Int) -> Int {\n    x + 100\n}\nfn answer() -> Int {\n    let op = Op { f: quiet }\n    op.f(1)\n}\nderived y = answer()\nfn main() {\n    println(\"{snap(y)} {(Op { f: quiet }.f)(2)}\")\n}\n",
        "",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "101 102\n");
    assert!(
        stderr.contains("E026"),
        "the old form still warns: {stderr}"
    );
}

/// 0.4: a derived value, a `const`, or a `track fn` may not run an `act fn`,
/// however reached: a derived value did not follow what the `act fn` read
/// and went stale (`2 1`).
#[test]
fn pure_contexts_do_not_run_act_fns() {
    let read = "let mut x = 1\nact fn read() -> Int {\n    x\n}\n";
    for (body, needle) in [
        (
            "derived y = read()\n",
            "a derived value cannot call the `act fn` `read`",
        ),
        (
            "const C = read()\n",
            "a `const` initializer cannot call the `act fn` `read`",
        ),
        (
            "fn apply(f: fn() -> Int) -> Int {\n    f()\n}\nderived y = apply(read)\n",
            "a derived value cannot call the `act fn` `read`",
        ),
        (
            "track fn t() -> Int {\n    read()\n}\n",
            "the `track fn` `t` cannot call the `act fn` `read`",
        ),
    ] {
        rejects(&format!("{read}{body}fn main() {{\n}}\n"), needle);
    }
    // What the error suggests: a `track fn` is followed.
    prints(
        "import io { println }\nlet mut x = 1\ntrack fn read() -> Int {\n    x\n}\nderived y = read()\nact fn main() {\n    x = 2\n    println(\"{x} {y}\")\n}\n",
        "2 2\n",
    );
}
