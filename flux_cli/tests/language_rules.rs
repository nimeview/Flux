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
    // An arm in an `act fn` writes the source, not a local binding; the
    // batch settles when the `act fn` returns.
    prints(
        "import io { println }\nlet mut x = 0\nwatch x {\n    println(\"x={x}\")\n}\n\
         act fn main() {\n    match 0 {\n        _ => {\n            x = 1\n            println(\"in arm\")\n        }\n    }\n}\n",
        "x=0\nin arm\nx=1\n",
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
        "`emit` is not allowed in a derived value",
    );
    // 0.5: a handler's arms may act, as its body does.
    prints(
        "import io { println }\nevent T\nevent U\non T {\n    match 0 {\n        _ => {\n            emit U\n        }\n    }\n}\non U {\n    println(\"u\")\n}\nfn main() {\n    emit T\n}\n",
        "u\n",
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

#[test]
fn an_event_checks_the_number_and_types_of_its_values() {
    let head = "event Move(x: Int, y: Int)\n";
    rejects(
        &format!("{head}fn main() {{\n    emit Move(1)\n}}\n"),
        "expected `2 values`, found `1 value`",
    );
    rejects(
        &format!("{head}fn main() {{\n    emit Move(1, \"a\")\n}}\n"),
        "type mismatch",
    );
    rejects(
        &format!("{head}on Move(a) {{\n}}\nfn main() {{\n}}\n"),
        "expected `2 values`, found `1 name`",
    );
    rejects(
        &format!("{head}let mut s = \"\"\non Move(a, b) {{\n    s = a\n}}\nfn main() {{\n}}\n"),
        "type mismatch",
    );
    rejects(
        "event Move(x: Int, x: Int)\nfn main() {\n}\n",
        "duplicate definition of `x`",
    );
    rejects(
        "event T(n: Int)\non T(a) { b ->\n}\nfn main() {\n}\n",
        "name the event's values once",
    );
}

#[test]
fn migrate_leaves_current_code_alone() {
    let dir = tempfile::tempdir().unwrap();
    let source =
        "import io { println }\n\n// rfn in a comment\nfn main() {\n    println(\"rfn\")\n}\n";
    std::fs::write(dir.path().join("main.fl"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["migrate", "--write", "main.fl"])
        .current_dir(dir.path())
        .output()
        .expect("spawn flux");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("main.fl")).unwrap(),
        source
    );
}

#[test]
fn trailing_commas_semicolons_and_raw_strings() {
    prints(
        "import io { println, }\n\
         struct P {\n    x: Int,\n    y: Int,\n}\n\
         enum E {\n    A,\n    B(Int,),\n}\n\
         fn add(a: Int, b: Int,) -> Int {\n    a + b\n}\n\
         fn pick(e: E) -> Int {\n    match e {\n        A => 0,\n        B(n) => n,\n    }\n}\n\
         fn main() {\n    let a = 1; let b = 2;\n    let p = P { x: a, y: b, }\n\
         \x20   let f = |u: Int, v: Int,| u + v\n\
         \x20   println(\"{add(a, b,)} {[1, 2,]} {p.y} {pick(B(7))} {f(1, 2)}\"); println(\"two\")\n}\n",
        "3 [1, 2] 2 7 3\ntwo\n",
    );
    prints(
        "import io { println }\nfn main() {\n    println(r\"no {x} \\n\")\n    println(r#\"{\"k\": \"v\"}\"#)\n    println(r##\"a \"# b\"##)\n}\n",
        "no {x} \\n\n{\"k\": \"v\"}\na \"# b\n",
    );
    prints(
        "import io { println }\nfn main() {\n    let s = r\"x\"\n    println(match s { r\"x\" => \"raw\", _ => \"other\" })\n}\n",
        "raw\n",
    );
    // A semicolon is only a separator: the block keeps its value.
    prints(
        "import io { println }\nfn f() -> Int {\n    let a = 1; a + 1;\n}\nfn main() {\n    println(\"{f()}\")\n}\n",
        "2\n",
    );
    rejects("fn main() {\n    let s = r\"open\n}\n", "parse error");
    rejects("fn main() {\n    let a = [,]\n}\n", "parse error");
}

#[test]
fn an_if_chain_ending_a_block_arm_is_the_arms_value() {
    // The issue's program: `else if` chain as a block arm's tail.
    prints(
        "import io { println }\nenum T { A, B(Int) }\nfn f(t: T, x: Int) -> T {\n    match t {\n        A => B(x),\n        B(v) => {\n            if x < v {\n                B(1)\n            } else if x > v {\n                B(2)\n            } else {\n                B(3)\n            }\n        }\n    }\n}\nfn show(t: T) -> Int {\n    match t {\n        A => 0,\n        B(n) => n\n    }\n}\nfn main() {\n    println(\"{show(f(B(5), 1))} {show(f(B(5), 9))} {show(f(B(5), 5))} {show(f(A, 7))}\")\n}\n",
        "1 2 3 7\n",
    );
    // Branches may run statements before their value, nest, and leave.
    prints(
        "import io { println }\nfn g(n: Int) -> String {\n    let s = match n {\n        0 => \"zero\",\n        _ => {\n            let half = n / 2\n            if half > 10 {\n                let big = \"big\"\n                println(\"seen {n}\")\n                big\n            } else if n < 0 {\n                return \"negative\"\n            } else {\n                if n % 2 == 0 { \"even\" } else { \"odd\" }\n            }\n        }\n    }\n    \"{s}!\"\n}\nfn main() {\n    println(\"{g(0)} {g(50)} {g(-3)} {g(4)} {g(7)}\")\n}\n",
        "seen 50\nzero! big! negative even! odd!\n",
    );
    // An `if` without `else` still leaves the arm `()`.
    rejects(
        "fn f(n: Int) -> Int {\n    match n {\n        0 => 1,\n        _ => {\n            if n > 1 {\n                2\n            }\n        }\n    }\n}\nfn main() {\n    f(1)\n}\n",
        "Unit",
    );
    // Branches must agree, as in a function body's tail.
    rejects(
        "fn f(n: Int) -> Int {\n    match n {\n        0 => 1,\n        _ => {\n            if n > 1 {\n                2\n            } else {\n                \"two\"\n            }\n        }\n    }\n}\nfn main() {\n    f(1)\n}\n",
        "mismatch",
    );
    // No numeric widening (the runtime inserts no conversion), and the
    // condition must be a `Bool`, as for a function body's `if` tail.
    rejects(
        "fn f(n: Int) -> Float {\n    match n {\n        0 => 1.5,\n        _ => {\n            if n > 1 { 2 } else { 2.5 }\n        }\n    }\n}\nfn main() {\n    f(1)\n}\n",
        "expected `Int`, found `Float`",
    );
    rejects(
        "fn f(n: Int) -> Int {\n    match n {\n        0 => 1,\n        _ => {\n            if n { 2 } else { 3 }\n        }\n    }\n}\nfn main() {\n    f(1)\n}\n",
        "expected `Bool`, found `Int`",
    );
}

/// 0.3.1: `break` and `continue` belong to a loop; a function cannot leave
/// with one (a `-> Int` function returned `()`).
#[test]
fn break_and_continue_outside_a_loop_are_errors() {
    rejects(
        "fn answer() -> Int {\n    break\n    42\n}\nfn main() {\n    let a = answer()\n}\n",
        "`break` outside a loop",
    );
    rejects(
        "act fn main() {\n    continue\n}\n",
        "`continue` outside a loop",
    );
    prints(
        "import io { println }\nfn f(xs: [Int]) -> Int {\n    let mut n = 0\n    for x in xs {\n        match x {\n            0 => continue,\n            _ => {\n                if x > 5 {\n                    break\n                }\n            }\n        }\n        n += x\n    }\n    n\n}\nfn main() {\n    println(\"{f([1, 0, 2, 9, 3])}\")\n}\n",
        "3\n",
    );
}

/// 0.3.1: a value holding a function, at any depth, has no `==` and no text
/// form (`[id].contains(id)` compared functions; 17 levels of arrays passed).
#[test]
fn functions_inside_values_are_neither_compared_nor_shown() {
    let id = "fn id(x: Int) -> Int {\n    x\n}\n";
    rejects(
        &format!("import io {{ println }}\n{id}fn main() {{\n    println(\"{{[id].contains(id)}}\")\n}}\n"),
        "elements that `==` can compare",
    );
    rejects(
        &format!("{id}fn main() {{\n    let s = [id].to_string()\n}}\n"),
        "a value with a text form",
    );
    let deep = format!("{}id{}", "[".repeat(18), "]".repeat(18));
    rejects(
        &format!("{id}fn main() {{\n    let a = {deep}\n    let same = a == a\n}}\n"),
        "compatible equality types",
    );
    rejects(
        &format!("import io {{ println }}\n{id}fn main() {{\n    let a = {deep}\n    println(\"{{a}}\")\n}}\n"),
        "a value with a text form",
    );
}

/// 0.3.1: a function ending in a `match` whose arms all `return` has the
/// returned values' type, as one ending in `if … else` does.
#[test]
fn a_match_whose_arms_all_return_ends_a_function() {
    prints(
        "import io { println }\nfn f(x: Int) -> Int {\n    match x {\n        0 => return 1,\n        _ => return 2,\n    }\n}\nfn g(x: Int) {\n    match x {\n        0 => return 10,\n        _ => return 20,\n    }\n}\nfn main() {\n    println(\"{f(0)} {f(5)} {g(0)}\")\n}\n",
        "1 2 10\n",
    );
    rejects(
        "fn h(x: Int) -> Int {\n    match x {\n        0 => return \"s\",\n        _ => return 2,\n    }\n}\nfn main() {\n    let a = h(1)\n}\n",
        "expected `Int`, found `String`",
    );
}

/// 0.4: `checked_` operations give `None` where the plain operation stops
/// the program, in the receiver's integer type.
#[test]
fn checked_integer_operations_return_none_instead_of_stopping() {
    prints(
        "import io { println }\nfn main() {\n    let big = 9223372036854775807\n    let small = 120i8\n    let u = 3u8\n    let neg = -7\n    let lowest = -127i8 - 1i8\n    println(\"{big.checked_add(1)} {big.checked_sub(1)} {small.checked_add(7)} {small.checked_add(8)}\")\n    println(\"{u.checked_sub(4)} {u.checked_mul(85)} {u.checked_mul(86)} {small.checked_add(300)}\")\n    println(\"{7.checked_div(0)} {7.checked_rem(0)} {7.checked_div(2)} {neg.checked_rem(3)}\")\n    println(\"{2.checked_pow(62)} {2.checked_pow(63)} {3.checked_pow(0)} {2.checked_pow(-1)}\")\n    println(\"{lowest.checked_neg()} {small.checked_neg()} {lowest.checked_div(-1)}\")\n    println(\"{big.checked_add(5).unwrap_or(0)}\")\n}\n",
        "None Some(9223372036854775806) Some(127) None\n\
         None Some(255) None None\n\
         None None Some(3) Some(-1)\n\
         Some(4611686018427387904) None Some(1) None\n\
         None Some(-120) None\n\
         0\n",
    );
    rejects(
        "fn main() {\n    let f = 1.5\n    let a = f.checked_add(1)\n}\n",
        "`checked_` operations are for integer types",
    );
    rejects(
        "fn main() {\n    let b = 3.checked_add(1.5)\n}\n",
        "expected `an integer`, found `Float`",
    );
}

/// 0.4: bit operations on integers, with Rust's precedence: `|` < `^` <
/// `&` < shifts < `+`, all below comparisons. `!` on an integer flips its
/// bits; `||`, `&&`, and closures `|x|` keep their meaning.
#[test]
fn bit_operations_work_on_integers() {
    prints(
        "import io { println }\nconst READ = 1\nconst WRITE = 2\nfn main() {\n    let flags = READ | WRITE\n    let word = 0x1234\n    let b = 0xf0u8\n    let neg = -8\n    println(\"{flags} {flags & WRITE} {flags ^ READ} {word >> 8} {word & 0xff} {1 << 10}\")\n    println(\"{!0u8} {!b} {!0} {b << 4} {neg >> 1} {200u8 >> 3}\")\n    println(\"{1 + 2 << 3} {6 & 3 == 2} {true || false && !false}\")\n    println(\"{[1, 2, 3].map(|x| x | 8)}\")\n}\n",
        "3 2 2 18 52 1024\n255 15 -1 0 -4 25\n24 true true\n[9, 10, 11]\n",
    );
    rejects(
        "fn main() {\n    let a = 1.5 & 2\n}\n",
        "integer types for a bit operation",
    );
    rejects(
        "fn main() {\n    let b = true | false\n}\n",
        "integer types for a bit operation",
    );
    let (status, _, stderr) =
        both("import io { println }\nfn main() {\n    let n = 70\n    println(\"{1 << n}\")\n}\n");
    assert_eq!(status, Some(1));
    assert!(stderr.contains("integer overflow"), "{stderr}");
}

/// 0.4: `mut` parameters: the function's changes go back to the caller's
/// variable, field, or element; a function that declares a result returns
/// it as well.
#[test]
fn mut_parameters_write_back_to_the_callers_place() {
    prints(
        "import io { println }\nimport collections { range }\nstruct Cart {\n    items: [Int],\n    total: Int\n}\nfn add_tax(mut prices: [Int], pct: Int) {\n    for i in range(prices.len()) {\n        prices[i] += prices[i] * pct / 100\n    }\n}\nfn take_first(mut xs: [Int]) -> Option[Int] {\n    match xs.first() {\n        Some(v) => {\n            xs = xs.slice(1, xs.len())\n            return Some(v)\n        },\n        None => None\n    }\n}\nfn swap(mut a: Int, mut b: Int) {\n    let t = a\n    a = b\n    b = t\n}\nfn bump(mut n: Int) -> Int {\n    n += 1\n    if n > 5 {\n        n * 10\n    } else {\n        n\n    }\n}\nfn main() {\n    let mut list = [100, 200]\n    add_tax(mut list, 10)\n    let mut x = 1\n    let mut y = 2\n    swap(mut x, mut y)\n    let first = take_first(mut list)\n    let mut cart = Cart { items: [5, 6], total: 0 }\n    add_tax(mut cart.items, 50)\n    let mut grid = [[1, 2], [3, 4]]\n    let mut k = 0\n    add_tax(mut grid[k + 1], 100)\n    let mut c = 5\n    let r = bump(mut c)\n    println(\"{list} {x} {y} {first} {cart.items} {grid} {c} {r}\")\n}\n",
        "[220] 2 1 Some(110) [7, 9] [[1, 2], [6, 8]] 6 60\n",
    );
    let grow = "fn grow(mut xs: [Int], n: Int) {\n    xs = xs + [n]\n}\nfn plain(xs: [Int]) -> Int {\n    xs.len()\n}\n";
    for (body, needle) in [
        (
            "    let mut a = [1]\n    grow(a, 2)\n",
            "this function has `mut` parameters",
        ),
        (
            "    let g = grow\n",
            "`grow` has `mut` parameters and cannot be used as a value",
        ),
        (
            "    let mut a = [1]\n    let n = plain(mut a)\n",
            "`mut` arguments go to a declared function with `mut` parameters",
        ),
        (
            "    let mut a = [1]\n    let n = 1 + plain([0]) + grow(mut a, 2).len()\n",
            "a call with `mut` arguments must be a statement, or the value of a `let`",
        ),
        (
            "    let mut a = [1]\n    let r = grow(mut a, 2)\n",
            "declares no result, so it has no value to bind",
        ),
        (
            "    grow(mut [1, 2], 3)\n",
            "a `mut` argument is a variable, or a field or element of one",
        ),
        (
            "    let a = [1]\n    grow(mut a, 2)\n",
            "cannot assign to immutable local `a`",
        ),
    ] {
        rejects(&format!("{grow}fn main() {{\n{body}}}\n"), needle);
    }
}
