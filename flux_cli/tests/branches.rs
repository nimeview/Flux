//! One `if` and one `match` everywhere (0.5): a value `if` takes `else if`
//! and statements in its branches; a block arm needs no comma after it; an
//! arm may be one statement (`=> emit E()`, `=> n += 1`); `on` handlers and
//! watches may act in their arms, as in their bodies. Derived values stay
//! pure. Every program runs on both engines, which must agree.

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

fn both(src: &str) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let vm = flux(
        dir.path(),
        &["run", "--vm", "--no-cache", "--quiet", "main.fl"],
    );
    let jit = flux(
        dir.path(),
        &["run", "--jit", "--no-cache", "--quiet", "main.fl"],
    );
    assert_eq!(vm, jit, "{src}");
    vm
}

fn prints(src: &str, want: &str) {
    let (status, stdout, stderr) = both(src);
    assert_eq!(status, Some(0), "{src}\n{stderr}");
    assert_eq!(stdout, want, "{src}");
}

fn rejects(src: &str, needles: &[&str]) {
    let (status, _, stderr) = both(src);
    assert_eq!(status, Some(1), "{src}");
    for needle in needles {
        assert!(
            stderr.contains(needle),
            "{src}\nmissing {needle:?} in\n{stderr}"
        );
    }
}

#[test]
fn a_value_if_takes_else_if_and_statements() {
    prints(
        r#"import io { println }

fn grade(n: Int) -> String {
    let g = if n > 90 { "A" } else if n > 50 { "B" } else { "C" }
    g
}

fn describe(n: Int) -> String {
    let text = if n < 0 {
        let m = -n
        "minus {m}"
    } else if n == 0 {
        "zero"
    } else {
        let twice = n * 2
        let more = twice + 1
        "plus {more}"
    }
    text
}

fn main() {
    println("{grade(95)} {grade(60)} {grade(1)}")
    println("{describe(-4)} {describe(0)} {describe(3)}")
    println("{1 + if true { let a = 2; a } else { 0 }}")
}
"#,
        "A B C\nminus 4 zero plus 7\n3\n",
    );
}

#[test]
fn a_derived_value_if_recomputes_with_its_statements() {
    prints(
        r#"import io { println }

event Set(v: Int)

let mut t = 5

derived mode = if t < 0 {
    "cold"
} else if t < 20 {
    let mid = t * 2
    "mild {mid}"
} else {
    "hot"
}

on Set(v) {
    t = v
}

watch mode {
    println(mode)
}

fn main() {
    emit Set(-3)
    emit Set(7)
    emit Set(30)
}
"#,
        "mild 10\ncold\nmild 14\nhot\n",
    );
}

#[test]
fn a_branch_may_leave_the_function() {
    prints(
        "import io { println }\n\nfn f(x: Int) -> Int {\n    let y = if x > 0 { x } else { return 0 }\n    y * 10\n}\n\nfn main() {\n    println(\"{f(3)} {f(-1)}\")\n}\n",
        "30 0\n",
    );
}

#[test]
fn handlers_and_watches_act_in_match_arms() {
    prints(
        r#"import io { println }

enum K {
    A,
    B,
    C,
}

event Hit(k: K)
event Boom

let mut booms = 0
let mut seen = 0
let mut last = 0

on Hit(k) {
    match k {
        K::A => emit Boom(),
        K::B => seen += 1,
        K::C => {
            if seen > 0 {
                emit Boom()
            }
            seen = 0
        }
    }
}

on Boom {
    booms += 1
}

watch booms {
    match booms {
        0 => {}
        _ => last = booms,
    }
}

fn main() {
    emit Hit(K::A)
    emit Hit(K::B)
    emit Hit(K::C)
    println("{snap(booms)} {snap(seen)} {snap(last)}")
}
"#,
        "2 0 2\n",
    );
}

#[test]
fn block_arms_need_no_comma() {
    prints(
        "import io { println }\n\nfn f(x: Int) -> Int {\n    match x {\n        0 => {\n            1\n        }\n        1 => {\n            let y = 5\n            y\n        }\n        _ => 9,\n    }\n}\n\nfn main() {\n    println(\"{f(0)} {f(1)} {f(2)}\")\n}\n",
        "1 5 9\n",
    );
}

#[test]
fn a_derived_branch_cannot_act() {
    rejects(
        "event Boom\n\nlet mut t = 1\n\nderived d = if t > 0 {\n    emit Boom()\n    1\n} else {\n    2\n}\n\nfn main() {\n}\n",
        &["`emit` is not allowed in a derived value", "do it in an `on` handler or an `act fn`"],
    );
    rejects(
        "import io { println }\n\nlet mut t = 1\n\nderived d = if t > 0 {\n    println(\"side effect\")\n    1\n} else {\n    2\n}\n\nfn main() {\n}\n",
        &["E017"],
    );
}

/// A value `if` or `match` whose branches mix Int and Float is a Float, and
/// the Int branch is converted when it runs (0.5: it stayed an Int at run
/// time, printed `3` for a `Float`).
#[test]
fn mixed_number_branches_convert_when_they_run() {
    prints(
        "import io { println }\n\nlet mut c = true\n\nderived x = if c { 3 } else { 2.5 }\n\nfn f(n: Int) -> Float {\n    let a = if n > 1 { 3 } else { 2.5 }\n    a\n}\n\nfn g(n: Int) -> Float {\n    match n { 0 => 3, _ => 2.5 }\n}\n\nfn h(n: Int) -> Float {\n    let y = if n > 0 {\n        let t = n * 2\n        t\n    } else {\n        0.5\n    }\n    y\n}\n\nfn main() {\n    println(\"{snap(x)} {f(3)} {f(0)} {g(0)} {g(1)} {h(4)} {f(3) / 2}\")\n}\n",
        "3.0 3.0 2.5 3.0 2.5 8.0 1.5\n",
    );
}
