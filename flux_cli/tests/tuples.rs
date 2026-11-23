//! Tuples: literals, types, `t.0`, destructuring in `let` and `for`, tuple
//! patterns in `match` with exhaustiveness, and map iteration. Every program
//! runs on both engines, which must agree.

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
    assert!(stderr.is_empty(), "{stderr}");
}

fn rejects(src: &str, needle: &str) {
    let (status, _, stderr) = both(src);
    assert_eq!(status, Some(1), "{src}");
    assert!(stderr.contains(needle), "{src}\n{stderr}");
}

#[test]
fn tuples_are_built_read_and_taken_apart() {
    prints(
        "import io { println }\nfn divmod(a: Int, b: Int) -> (Int, Int) {\n    (a / b, a % b)\n}\nfn main() {\n    let t = (1, \"a\", true)\n    let (q, r) = divmod(17, 5)\n    let (x, (y, _)) = (1, (2, 3))\n    println(\"{t} {t.0} {t.1} {q} {r} {x} {y}\")\n    let mut p = (1, 2)\n    p.0 += 10\n    println(\"{p} {p == (11, 2)} {p != (1, 2)}\")\n    let pairs: [(Int, String)] = [(1, \"one\"), (2, \"two\")]\n    for (n, name) in pairs {\n        println(\"{n} {name}\")\n    }\n}\n",
        "(1, \"a\", true) 1 a 3 2 1 2\n(11, 2) true true\n1 one\n2 two\n",
    );
}

#[test]
fn a_map_loop_gives_entries_in_key_order() {
    prints(
        "import io { println }\nimport collections { map_new }\nfn main() {\n    let mut m: Map[String, Int] = map_new()\n    m[\"b\"] = 2\n    m[\"a\"] = 1\n    for (k, v) in m {\n        println(\"{k}={v}\")\n    }\n    for e in m {\n        println(\"{e}\")\n    }\n}\n",
        "a=1\nb=2\n(\"a\", 1)\n(\"b\", 2)\n",
    );
}

#[test]
fn tuple_patterns_match_position_by_position() {
    prints(
        "import io { println }\nenum Light {\n    Red,\n    Green,\n    Blink(Int)\n}\nfn describe(t: (Int, String)) -> String {\n    match t {\n        (0, name) => \"zero {name}\",\n        (n, \"x\") if n > 5 => \"big x\",\n        (n, _) => \"other {n}\"\n    }\n}\nfn both(a: Bool, b: Bool) -> Int {\n    match (a, b) {\n        (true, true) => 3,\n        (true, false) => 2,\n        (false, _) => 0\n    }\n}\nfn lights(p: (Light, Option[Int])) -> Int {\n    match p {\n        (Red, _) => 1,\n        (Green, Some(n)) => n,\n        (Green, None) => 0,\n        (Blink(k), Some(_)) => k,\n        (Blink(_), None) => 9\n    }\n}\nfn first(o: Option[(Int, Int)]) -> Int {\n    match o {\n        Some((a, 0)) => a,\n        Some((_, b)) => b,\n        None => -1\n    }\n}\nfn leave(t: (Int, Int)) -> Int {\n    match t {\n        (0, _) => return 100,\n        (a, b) => a + b\n    }\n}\nfn main() {\n    println(\"{describe((0, \"q\"))} {describe((9, \"x\"))} {describe((2, \"x\"))}\")\n    println(\"{both(true, true)} {both(true, false)} {both(false, true)}\")\n    println(\"{lights((Red, None))} {lights((Green, Some(4)))} {lights((Blink(7), Some(1)))} {lights((Blink(7), None))}\")\n    println(\"{first(Some((5, 0)))} {first(Some((5, 6)))} {first(None)}\")\n    println(\"{leave((0, 1))} {leave((2, 3))}\")\n}\n",
        "zero q big x other 2\n3 2 0\n1 4 7 9\n5 6 -1\n100 5\n",
    );
}

#[test]
fn a_match_on_tuples_must_cover_every_case() {
    rejects(
        "fn f(a: Bool, b: Bool) -> Int {\n    match (a, b) {\n        (true, true) => 1,\n        (false, _) => 0\n    }\n}\nfn main() {\n}\n",
        "patterns that cover every tuple",
    );
    rejects(
        "fn f(t: (Int, String)) -> Int {\n    match t {\n        (0, _) => 1\n    }\n}\nfn main() {\n}\n",
        "patterns that cover every tuple",
    );
}

#[test]
fn tuple_mistakes_are_reported() {
    rejects(
        "fn main() {\n    let x = (1,)\n}\n",
        "a tuple has two or more values",
    );
    rejects(
        "fn main() {\n    let (a, b, c) = (1, 2)\n}\n",
        "expected `a tuple of 3 values`, found `(Int64, Int64)`",
    );
    rejects(
        "fn main() {\n    let t = (1, 2)\n    let u = t.5\n}\n",
        "`.5` on `(Int64, Int64)`",
    );
    rejects(
        "fn main() {\n    let t: (Int, String) = (1, 2)\n}\n",
        "expected `(Int64, String)`, found `(Int64, Int64)`",
    );
}

#[test]
fn tuples_work_with_generics_and_closures() {
    prints(
        "import io { println }\nimport collections { map }\nfn swap[A, B](p: (A, B)) -> (B, A) {\n    (p.1, p.0)\n}\nfn main() {\n    let k = 10\n    let xs = map([1, 2], |x| (x, x * k))\n    println(\"{swap((1, \"a\"))} {xs}\")\n}\n",
        "(\"a\", 1) [(1, 10), (2, 20)]\n",
    );
}

#[test]
fn tuple_forms_survive_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let src = "fn f(p:(Int,Int))->(Int,Int) {\n    let (a,b)=p\n    match p {\n        (0,_) => (b,a),\n        _ => p\n    }\n}\n\nfn main() {\n    for (x , y) in [(1,2)] {\n        let t = f((x,y))\n    }\n}\n";
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(
        stdout.contains("fn f(p: (Int, Int)) -> (Int, Int) {"),
        "{stdout}"
    );
    assert!(stdout.contains("    let (a, b) = p\n"), "{stdout}");
    assert!(
        stdout.contains("    match p { (0, _) => (b, a), _ => p }\n"),
        "{stdout}"
    );
    assert!(stdout.contains("    for (x, y) in [(1, 2)] {"), "{stdout}");
    std::fs::write(dir.path().join("main.fl"), &stdout).unwrap();
    let (_, again, _) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(again, stdout);
}

#[test]
fn tuples_inside_arrays_and_options_reach_compiled_code_safely() {
    prints(
        "import io { println }\nimport collections { len }\nfn count(xs: [(Int, Int)]) -> Int {\n    len(xs) * 2\n}\nfn pick(o: Option[(Int, Int)]) -> Int {\n    match o {\n        Some(p) => p.0,\n        None => 0\n    }\n}\nfn main() {\n    let xs = [(1, 2), (3, 4)]\n    println(\"{count(xs)} {pick(Some((5, 6)))} {xs == [(1, 2), (3, 4)]}\")\n}\n",
        "4 5 true\n",
    );
}

#[test]
fn nested_tuple_patterns_keep_each_level_apart() {
    prints(
        "import io { println }\nfn main() {\n    let n = match ((true, false), true) {\n        ((a, b), c) => c\n    }\n    let m = match ((1, 2), 3) {\n        ((a, b), c) => a + b * 10 + c * 100\n    }\n    println(\"{n} {m}\")\n}\n",
        "true 321\n",
    );
}

#[test]
fn a_leaving_tuple_arm_is_still_type_checked() {
    rejects(
        "import io { println }\nfn f(t: (Int, Int)) -> Int {\n    match t {\n        (0, _) => return \"oops\",\n        (a, b) => a + b\n    }\n}\nfn main() {\n    println(\"{f((0, 1))}\")\n}\n",
        "expected `Int64`, found `String`",
    );
}

#[test]
fn exhaustiveness_handles_recursive_and_wide_tuples() {
    let (status, _, stderr) = both(
        "enum L {\n    Nil,\n    Next(L)\n}\nfn f(t: (L, Bool)) -> Int {\n    match t {\n        (_, true) => 1,\n        (_, false) => 0\n    }\n}\nfn main() {\n}\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    let wide = |last: &str| {
        let mut parts = vec!["_"; 20];
        parts.push(last);
        format!("({})", parts.join(", "))
    };
    let ty = format!("({})", vec!["Bool"; 21].join(", "));
    let src = format!(
        "fn f(t: {ty}) -> Int {{\n    match t {{\n        {} => 1,\n        {} => 0\n    }}\n}}\nfn main() {{\n}}\n",
        wide("true"),
        wide("false")
    );
    let started = std::time::Instant::now();
    let (status, _, stderr) = both(&src);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(started.elapsed().as_secs() < 10);
}

#[test]
fn destructuring_checks_the_whole_shape() {
    let (status, _, stderr) = both(
        "fn main() {\n    let (_, _) = 42\n    let (a, b, _) = (1, 2)\n    let (c, d) = (1, 2, 3)\n}\n",
    );
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("expected `a tuple of 2 values`, found `Int64`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("expected `a tuple of 3 values`, found `(Int64, Int64)`"),
        "{stderr}"
    );
    assert!(stderr.contains("found `(Int64, Int64, Int64)`"), "{stderr}");
    rejects(
        "import io { println }\nlet (a, b): (String, String) = (1, 2)\nfn main() {\n    println(\"{snap(a)} {snap(b)}\")\n}\n",
        "expected `String`, found `Int64`",
    );
    rejects(
        "import io { println }\nfn main() {\n    let t = (1, 2)\n    println(\"{t.00}\")\n}\n",
        "`.00` on `(Int64, Int64)`",
    );
}

#[test]
fn generic_calls_see_tuple_and_map_entry_types() {
    prints(
        "import io { println }\nimport collections { map_new }\nfn id[T](x: T) -> T {\n    x\n}\nfn f(p: (Int, String)) -> Int {\n    id(p.0)\n}\nfn main() {\n    let mut m: Map[String, Int] = map_new()\n    m[\"a\"] = 1\n    for (k, v) in m {\n        println(\"{id(k)} {id(v)} {f((2, k))}\")\n    }\n}\n",
        "a 1 2\n",
    );
}
