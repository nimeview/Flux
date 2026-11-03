//! Language rules decided before the surface freeze. Every program runs on both
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

fn rejects(src: &str, needle: &str) {
    let (status, _, stderr) = both(src);
    assert_eq!(status, Some(1), "{src}");
    assert!(stderr.contains(needle), "{src}\n{stderr}");
}

/// `import io { println }` and `fn main() { println("{expr}") }`.
fn value(expr: &str, want: &str) {
    prints(
        &format!("import io {{ println }}\nfn main() {{\n    println(\"{{{expr}}}\")\n}}\n"),
        &format!("{want}\n"),
    );
}

#[test]
fn function_match_arms_may_emit_and_write_sources() {
    prints(
        "import io { println }\nevent T\non T {\n    println(\"got\")\n}\n\
         fn main() {\n    match 0 {\n        _ => {\n            emit T\n            0\n        }\n    }\n    println(\"after\")\n}\n",
        "got\nafter\n",
    );
    // The write propagates at once, as in a function body: not a local binding.
    prints(
        "import io { println }\nlet mut x = 0\nwatch x {\n    println(\"x={x}\")\n}\n\
         fn main() {\n    match 0 {\n        _ => {\n            x = 1\n            println(\"in arm\")\n        }\n    }\n}\n",
        "x=0\nx=1\nin arm\n",
    );
    // Direct and nested forms now obey the same rule.
    prints(
        "import io { println }\nevent T\non T {\n    println(\"got\")\n}\n\
         fn main() {\n    match 0 {\n        _ => {\n            if true {\n                emit T\n            }\n            0\n        }\n    }\n}\n",
        "got\n",
    );
}

#[test]
fn match_arms_keep_their_other_limits() {
    let derived = "let mut x = 0\nlet d = x + 1\nfn main() {\n    match 0 {\n        _ => {\n            d = 1\n        }\n    }\n}\n";
    rejects(derived, "cannot assign to derived binding `d`");
    let nested = "let mut x = 0\nlet d = x + 1\nfn main() {\n    match 0 {\n        _ => {\n            if true {\n                d = 1\n            }\n        }\n    }\n}\n";
    rejects(nested, "cannot assign to derived binding `d`");
    rejects(
        "const C = 1\nfn main() {\n    match 0 {\n        _ => {\n            C = 2\n        }\n    }\n}\n",
        "`C`",
    );
    rejects(
        "event T\nlet mut x = 0\nderived d = match x {\n    _ => {\n        emit T\n        1\n    }\n}\nfn main() { }\n",
        "`emit` is not supported",
    );
    rejects(
        "event T\non T {\n    match 0 {\n        _ => {\n            emit T\n        }\n    }\n}\nfn main() { }\n",
        "`emit` is not supported",
    );
    // An effect hidden in a function's arm still cannot reach a derived value.
    rejects(
        "event T\nlet mut x = 1\nfn noisy() -> Int {\n    match 0 {\n        _ => {\n            emit T\n            1\n        }\n    }\n}\nlet d = x + noisy()\nfn main() { }\n",
        "side effect `noisy`",
    );
}

#[test]
fn true_and_false_cover_bool() {
    prints(
        "import io { println }\nfn bit(b: Bool) -> Int {\n    match b { true => 1, false => 0 }\n}\nfn main() {\n    println(\"{bit(true)} {bit(false)}\")\n}\n",
        "1 0\n",
    );
    rejects(
        "fn f(b: Bool) -> Int {\n    match b { true => 1 }\n}\nfn main() { }\n",
        "catch-all",
    );
    // A guarded arm covers nothing.
    rejects(
        "fn f(b: Bool, c: Bool) -> Int {\n    match b { true if c => 1, false => 0 }\n}\nfn main() { }\n",
        "catch-all",
    );
}

#[test]
fn numeric_literal_forms() {
    for (literal, want) in [
        ("1e3", "1000.0"),
        ("1E3", "1000.0"),
        ("1.5e3", "1500.0"),
        ("2.5e-3", "0.0025"),
        ("1e+2", "100.0"),
        ("1_000", "1000"),
        ("1_000.5", "1000.5"),
        ("0xff", "255"),
        ("0xFF", "255"),
        ("0xff_ff", "65535"),
        // These end in what looks like a float suffix, but it is hex digits.
        ("0xf32", "3890"),
        ("0xabcdef64", "2882400100"),
        ("0x10u8", "16"),
        ("0xffffffffffffffffu64", "18446744073709551615"),
        ("-0x8000000000000000", "-9223372036854775808"),
        ("1e3f32", "1000.0"),
    ] {
        value(literal, want);
    }
    for literal in [
        "1e400",
        "0xffffffffffffffff",
        "0x1ffffffffffffffffffffffffffffffff0",
    ] {
        rejects(
            &format!("fn main() {{\n    let a = {literal}\n}}\n"),
            "outside the supported range",
        );
    }
    // Never a number followed by a separate statement: `e` exists here.
    for literal in ["1__0", "1_", "0x", "1e", "12abc", "0xfg", "1e3x"] {
        rejects(
            &format!("fn main() {{\n    let e = 5\n    let a = {literal}\n}}\n"),
            "parse error",
        );
    }
}

#[test]
fn abs_keeps_an_integer_type() {
    for (expr, want) in [
        ("abs(-3)", "3"),
        ("abs(3)", "3"),
        ("abs(0)", "0"),
        ("abs(-9223372036854775807)", "9223372036854775807"),
        ("abs(-5 as Int32)", "5"),
        ("abs(200u8)", "200"),
        ("abs(18446744073709551615u64)", "18446744073709551615"),
        ("abs(-1.5)", "1.5"),
        ("abs(-3) / 2", "1"),
    ] {
        prints(
            &format!("import io {{ println }}\nimport math {{ abs }}\nfn main() {{\n    println(\"{{{expr}}}\")\n}}\n"),
            &format!("{want}\n"),
        );
    }
    prints(
        "import io { println }\nimport math { abs }\nfn dist(a: Int, b: Int) -> Int {\n    abs(a - b)\n}\nfn main() {\n    println(\"{dist(2, 9)}\")\n}\n",
        "7\n",
    );
    let (status, _, stderr) =
        both("import math { abs }\nfn main() {\n    let _a = abs(-9223372036854775807 - 1)\n}\n");
    assert_eq!(status, Some(1));
    assert!(stderr.contains("integer overflow"), "{stderr}");
    rejects(
        "import math { abs }\nfn main() {\n    let _a: Float = abs(-3)\n}\n",
        "E005",
    );
}

#[test]
fn strings_order_by_code_point() {
    for (expr, want) in [
        (r#""a" < "b""#, "true"),
        (r#""b" <= "a""#, "false"),
        (r#""abc" > "abd""#, "false"),
        (r#""Z" < "a""#, "true"),
        (r#""я" > "z""#, "true"),
        (r#""" >= """#, "true"),
        (r#""ab" > "a""#, "true"),
    ] {
        value(expr, want);
    }
    // At run time too, so that compiled code takes its slow path.
    prints(
        "import io { println }\nfn less(a: String, b: String) -> Bool {\n    a < b\n}\nfn main() {\n    println(\"{less(\"a\", \"b\")} {less(\"b\", \"a\")}\")\n}\n",
        "true false\n",
    );
    rejects(
        "fn main() {\n    let a = \"a\" < 1\n}\n",
        "ordered comparison",
    );
}

#[test]
fn ok_err_some_none_need_no_import() {
    prints(
        "import io { println }\nfn f(n: Int) -> Result[Int, String] {\n    if n > 0 {\n        Ok(n)\n    } else {\n        Err(\"neg\")\n    }\n}\n\
         fn g(x: Option[Int]) -> Int {\n    match x { Some(v) => v, None => 0 }\n}\n\
         fn main() {\n    println(\"{g(Some(1))} {g(None)}\")\n    match f(1) { Ok(v) => println(\"{v}\"), Err(e) => println(e) }\n}\n",
        "1 0\n1\n",
    );
}

#[test]
fn a_programs_own_name_wins_over_the_builtin_one() {
    prints(
        "import io { println }\nenum Opt {\n    Some(Int),\n    None\n}\nfn g(x: Opt) -> Int {\n    match x { Some(v) => v, None => 0 }\n}\nfn main() {\n    println(\"{g(Some(1))} {g(None)}\")\n}\n",
        "1 0\n",
    );
    prints(
        "import io { println }\nfn Ok(n: Int) -> Int {\n    n + 1\n}\nfn main() {\n    println(\"{Ok(1)}\")\n}\n",
        "2\n",
    );
    prints(
        "import io { println }\nfn main() {\n    let Some = 5\n    println(\"{Some}\")\n}\n",
        "5\n",
    );
    // Earlier imports stay valid.
    prints(
        "import io { println }\nimport result { Ok, Err }\nimport option\nfn main() {\n    let r: Result[Int, String] = Ok(1)\n    match r { Ok(v) => println(\"{v}\"), Err(e) => println(e) }\n}\n",
        "1\n",
    );
}

#[test]
fn a_module_gets_the_names_and_its_own_win_only_there() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.fl"),
        "pub fn wrap(n: Int) -> Option[Int] {\n    if n > 0 {\n        Some(n)\n    } else {\n        None\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "import lib { wrap }\nimport io { println }\nenum My {\n    Some(Int),\n    None\n}\nfn own(m: My) -> Int {\n    match m { Some(v) => v, None => 0 }\n}\n\
         fn main() {\n    match wrap(1) { Some(v) => println(\"{v} {own(None)}\"), None => println(\"none\") }\n}\n",
    )
    .unwrap();
    let vm = run(dir.path(), "--vm");
    assert_eq!(vm.0, Some(0), "{}", vm.2);
    assert_eq!(vm.1, "1 0\n");
    assert_eq!(vm, run(dir.path(), "--jit"));
}
