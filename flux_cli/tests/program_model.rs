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

/// Runs `src` on both engines and checks they print exactly `expected`.
fn assert_stdout_on_both(src: &str, expected: &str) {
    for engine in ["--vm", "--jit"] {
        let output = run(src, engine, &[]);
        assert!(output.status.success(), "{engine}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{engine}"
        );
    }
}

#[test]
fn a_write_in_a_watch_updates_derived_values_and_their_watches_in_the_same_event() {
    // The write used to wait for the next event, so `d` lagged one event behind `b`.
    let src = "import io { println }\n\
        event T(n: Int)\n\
        let mut a = 0\n\
        let mut b = 0\n\
        let d = b * 10\n\
        watch a {\n    b = a\n}\n\
        watch d {\n    println(\"b={b} d={d}\")\n}\n\
        on T { n ->\n    a = n\n}\n\
        fn main() {\n    emit T(1)\n    println(\"-\")\n    emit T(2)\n    println(\"-\")\n    emit T(3)\n}\n";
    assert_stdout_on_both(src, "b=0 d=0\nb=1 d=10\n-\nb=2 d=20\n-\nb=3 d=30\n");
}

#[test]
fn a_watch_writes_as_a_batch_and_later_watches_see_the_result() {
    // Inside the body the derived value keeps its old value; the body's writes
    // apply together when it ends, as in `on`.
    let src = "import io { println }\n\
        event T(n: Int)\n\
        let mut a = 0\n\
        let mut b = 0\n\
        let d = b * 10\n\
        watch a {\n    b = a\n    b = b + 1\n    println(\"in d={d}\")\n}\n\
        watch b {\n    println(\"after b={b} d={d}\")\n}\n\
        on T { n ->\n    a = n\n}\n\
        fn main() {\n    emit T(5)\n}\n";
    assert_stdout_on_both(src, "in d=0\nafter b=1 d=10\nin d=10\nafter b=6 d=60\n");
}

#[test]
fn a_watch_runs_at_most_once_per_event_so_cycles_end() {
    let src = "import io { println }\n\
        event T(n: Int)\n\
        let mut a = 0\n\
        let mut b = 0\n\
        watch a {\n    b = a + 1\n    println(\"wa {a} {b}\")\n}\n\
        watch b {\n    a = b + 1\n    println(\"wb {a} {b}\")\n}\n\
        on T { n ->\n    a = n\n}\n\
        fn main() {\n    emit T(10)\n    println(\"end\")\n}\n";
    let vm = run(src, "--vm", &[]);
    let jit = run(src, "--jit", &[]);
    assert!(vm.status.success(), "{vm:?}");
    assert_eq!(vm.stdout, jit.stdout);
    assert_eq!(vm.status.code(), jit.status.code());
    assert!(String::from_utf8_lossy(&vm.stdout).ends_with("end\n"));
}

#[test]
fn an_emit_from_a_watch_runs_its_handler_and_settles() {
    let src = "import io { println }\n\
        event T(n: Int)\n\
        event U(n: Int)\n\
        let mut a = 0\n\
        let mut c = 0\n\
        let d = c * 2\n\
        watch a {\n    emit U(a)\n}\n\
        watch d {\n    println(\"d={d}\")\n}\n\
        on T { n ->\n    a = n\n}\n\
        on U { n ->\n    c = n\n}\n\
        fn main() {\n    emit T(4)\n    println(\"end\")\n}\n";
    assert_stdout_on_both(src, "d=0\nd=8\nend\n");
}

#[test]
fn a_nested_emit_keeps_the_outer_bodys_writes_pending() {
    let src = "import io { println }\n\
        event T(n: Int)\n\
        event U(n: Int)\n\
        let mut x = 0\n\
        let mut y = 0\n\
        let d = x * 2\n\
        watch d {\n    println(\"d={d}\")\n}\n\
        on T { n ->\n    x = 1\n    emit U(0)\n    println(\"in {d}\")\n}\n\
        on U { n ->\n    y = 1\n}\n\
        fn main() {\n    emit T(0)\n}\n";
    assert_stdout_on_both(src, "d=0\nin 0\nd=2\n");
}

#[test]
fn writes_keep_statement_order_across_a_nested_emit() {
    // A compiled handler used to run the nested handler first and apply its
    // own write afterwards, leaving 1 here where the VM leaves 9.
    let src = "import io { println }\n\
        event T(n: Int)\n\
        event U(n: Int)\n\
        let mut x = 0\n\
        watch x {\n    println(\"x={x}\")\n}\n\
        on T { n ->\n    x = 1\n    emit U(0)\n}\n\
        on U { n ->\n    x = 9\n}\n\
        fn main() {\n    emit T(0)\n}\n";
    assert_stdout_on_both(src, "x=0\nx=9\nx=9\n");
}

const EMIT_CHAIN: &str = "import io { println }\n\
    event T(n: Int)\n\
    let mut c = 0\n\
    watch c {\n}\n\
    on T { n ->\n    c = c + 1\n    if n > 0 {\n        emit T(n - 1)\n    }\n}\n";

#[test]
fn a_long_finite_emit_chain_runs_on_both_engines() {
    // 10,000 nested handlers used to overflow the native stack on the VM.
    let src = format!("{EMIT_CHAIN}fn main() {{\n    emit T(9000)\n    println(\"done\")\n}}\n");
    assert_stdout_on_both(&src, "done\n");
}

#[test]
fn an_unbounded_emit_chain_is_a_runtime_error_not_a_crash() {
    for count in ["1000000", "100000000"] {
        let src =
            format!("{EMIT_CHAIN}fn main() {{\n    emit T({count})\n    println(\"no\")\n}}\n");
        for engine in ["--vm", "--jit"] {
            let output = run(&src, engine, &[]);
            assert_eq!(output.status.code(), Some(1), "{engine}: {output:?}");
            assert_eq!(text(&output.stdout), "", "{engine}");
            assert!(
                text(&output.stderr).contains("call depth limit exceeded"),
                "{engine}: {}",
                text(&output.stderr)
            );
        }
    }
}

const TREE: &str = "import io { println }\n\
    enum Tree {\n    Leaf,\n    Wrap(Tree)\n}\n\
    fn depth(t: Tree) -> Int {\n    match t {\n        Wrap(inner) => depth(inner) + 1,\n        Leaf => 0\n    }\n}\n";

#[test]
fn a_recursive_enum_builds_and_matches_at_any_depth() {
    let src = format!(
        "{TREE}fn main() {{\n    println(\"{{depth(Wrap(Wrap(Wrap(Wrap(Wrap(Wrap(Wrap(Leaf))))))))}}\")\n    let mut t = Leaf\n    let mut i = 0\n    while i < 100 {{\n        t = Wrap(t)\n        i = i + 1\n    }}\n    println(\"{{depth(t)}}\")\n}}\n"
    );
    assert_stdout_on_both(&src, "7\n100\n");
}

const LIST: &str = "import io { println }\n\
    enum List {\n    Nil,\n    Cons(Node)\n}\n\
    struct Node {\n    value: Int,\n    next: List\n}\n\
    fn build(n: Int) -> List {\n    let mut l = Nil\n    let mut i = 0\n    while i < n {\n        l = Cons(Node { value: i, next: l })\n        i = i + 1\n    }\n    l\n}\n\
    fn sum(l: List) -> Int {\n    let mut total = 0\n    let mut at = l\n    let mut going = true\n    while going {\n        match at {\n            Cons(n) => {\n                total = total + n.value\n                at = n.next\n            },\n            Nil => {\n                going = false\n            }\n        }\n    }\n    total\n}\n";

#[test]
fn mutually_recursive_types_work_through_fields_and_patterns() {
    let src = format!(
        "{LIST}fn second(l: List) -> Int {{\n    match l {{\n        Cons(a) => match a.next {{\n            Cons(b) => b.value,\n            Nil => 0\n        }},\n        Nil => 0\n    }}\n}}\nfn main() {{\n    println(\"{{sum(build(10))}} {{second(build(3))}} {{build(2) == build(2)}} {{build(2) == build(3)}}\")\n    println(\"{{build(1)}}\")\n}}\n"
    );
    assert_stdout_on_both(
        &src,
        "45 1 true false\nList::Cons(Node { next: List::Nil, value: 0 })\n",
    );
}

#[test]
fn a_struct_can_hold_itself_through_option_and_array() {
    let src = "import io { println }\n\
        import option { Some, None }\n\
        struct Chain {\n    v: Int,\n    next: Option[Chain]\n}\n\
        struct Tree {\n    v: Int,\n    kids: [Tree]\n}\n\
        fn count(t: Tree) -> Int {\n    let mut n = 1\n    for k in t.kids {\n        n = n + count(k)\n    }\n    n\n}\n\
        fn last(c: Chain) -> Int {\n    match c.next {\n        Some(n) => last(n),\n        None => c.v\n    }\n}\n\
        fn main() {\n    let leaf = Tree { v: 1, kids: [] }\n    println(\"{count(Tree { v: 0, kids: [leaf, Tree { v: 2, kids: [leaf] }] })}\")\n    println(\"{last(Chain { v: 1, next: Some(Chain { v: 2, next: Some(Chain { v: 3, next: None }) }) })}\")\n}\n";
    assert_stdout_on_both(src, "4\n3\n");
}

#[test]
fn a_recursive_type_still_rejects_wrong_values() {
    let src = "enum Tree {\n    Leaf,\n    Wrap(Tree)\n}\nfn main() {\n    let t = Wrap(3)\n}\n";
    let output = run(src, "--vm", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        text(&output.stderr).contains("type mismatch"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_missing_variant_is_reported_at_any_nesting_depth() {
    let src = "enum Tree {\n    Leaf,\n    Wrap(Tree)\n}\nfn f(t: Tree) -> Int {\n    match t {\n        Wrap(a) => match a {\n            Wrap(b) => match b {\n                Wrap(c) => match c {\n                    Wrap(d) => match d {\n                        Wrap(e) => match e {\n                            Wrap(g) => 1\n                        },\n                        Leaf => 2\n                    },\n                    Leaf => 3\n                },\n                Leaf => 4\n            },\n            Leaf => 5\n        },\n        Leaf => 6\n    }\n}\nfn main() {\n}\n";
    let output = run(src, "--vm", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        text(&output.stderr).contains("exhaustive match"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn long_recursive_values_do_not_overflow_the_stack() {
    let src = format!(
        "{LIST}fn main() {{\n    let a = build(100000)\n    let b = build(100000)\n    println(\"{{a == b}}\")\n    let s = \"{{a}}\"\n    println(\"{{sum(a)}}\")\n}}\n"
    );
    assert_stdout_on_both(&src, "true\n4999950000\n");
}

#[test]
fn generic_functions_take_recursive_types() {
    // Monomorphization used to expand the type forever and crash the compiler.
    let src = format!(
        "{TREE}fn id[A](x: A) -> A {{\n    x\n}}\nfn main() {{\n    println(\"{{depth(id(Wrap(id(Leaf))))}}\")\n}}\n"
    );
    assert_stdout_on_both(&src, "1\n");
}

#[test]
fn type_errors_name_a_recursive_type_briefly() {
    let src = "enum Tree {\n    Leaf,\n    Wrap(Tree)\n}\nfn f(t: Tree) -> Int {\n    t\n}\nfn main() {\n}\n";
    let output = run(src, "--vm", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("expected `Int64`, found `Tree`"),
        "{stderr}"
    );
}

#[test]
fn closures_capture_locals_and_parameters() {
    let src = "import io { println }\n\
        import collections { map, filter, reduce }\n\
        fn add_all(xs: [Int], k: Int) -> [Int] {\n    map(xs, |x| x + k)\n}\n\
        fn main() {\n    let k = 10\n    let name = \"n\"\n    let xs = [1, 2, 3]\n    println(\"{map(xs, |x| x + k)}\")\n    println(\"{add_all(xs, 2)}\")\n    println(\"{filter(xs, |x| x > k - 9)}\")\n    println(\"{reduce(xs, 0, |a, x| a + x * k)}\")\n    println(\"{map(xs, |x| \"{name}{x}\")}\")\n    println(\"{(|x: Int| x + k)(5)}\")\n}\n";
    assert_stdout_on_both(
        src,
        "[11, 12, 13]\n[3, 4, 5]\n[2, 3]\n60\n[\"n1\", \"n2\", \"n3\"]\n15\n",
    );
}

#[test]
fn a_closure_captures_by_value_when_it_is_created() {
    let src = "import io { println }\n\
        import collections { map }\n\
        fn main() {\n    let mut k = 1\n    let f = |x: Int| x + k\n    k = 100\n    println(\"{map([1], f)} {k}\")\n    let mut ys = [1]\n    let g = |i: Int| ys[0] + i\n    ys[0] = 50\n    println(\"{map([1], g)} {ys}\")\n}\n";
    assert_stdout_on_both(src, "[2] 100\n[2] [50]\n");
}

#[test]
fn closures_nest_and_capture_any_value() {
    let src = "import io { println }\n\
        import collections { map, reduce }\n\
        struct P {\n    x: Int\n}\n\
        enum E {\n    A(Int),\n    B\n}\n\
        fn f(p: P, e: E) -> [Int] {\n    map([1, 2], |a| reduce([10, 20], 0, |acc, b| acc + a * b + p.x + match e {\n        A(n) => n,\n        B => 0\n    }))\n}\n\
        fn main() {\n    println(\"{f(P { x: 1 }, A(2))}\")\n    let m = [1, 2]\n    println(\"{map([0, 1], |i| m[i])}\")\n}\n";
    assert_stdout_on_both(src, "[36, 66]\n[1, 2]\n");
}

#[test]
fn closures_capture_in_handlers_watches_and_generics() {
    let src = "import io { println }\n\
        import collections { map }\n\
        event T(n: Int)\n\
        let mut a = 1\n\
        watch a {\n    let k = a * 2\n    println(\"{map([1], |x| x + k)}\")\n}\n\
        on T { n ->\n    a = n\n    println(\"{map([1], |x| x + n)}\")\n}\n\
        fn twice[V](v: V) -> [V] {\n    map([1, 2], |i| v)\n}\n\
        fn main() {\n    emit T(5)\n    println(\"{twice(7)} {twice(\"s\")}\")\n}\n";
    assert_stdout_on_both(src, "[3]\n[6]\n[11]\n[7, 7] [\"s\", \"s\"]\n");
}

#[test]
fn a_captured_error_and_a_bad_capture_are_reported() {
    let body = "import io { println }\nimport collections { map }\nfn main() {\n";
    let cases = [
        ("    let mut k = 1\n    let ys = map([1], |x| match x {\n        1 => {\n            k = 2\n            k\n        },\n        _ => 0\n    })\n    println(\"{ys}\")\n", "immutable"),
        ("    let k = \"s\"\n    println(\"{map([1], |x| x * k)}\")\n", "type mismatch"),
    ];
    for (code, needle) in cases {
        let output = run(&format!("{body}{code}}}\n"), "--vm", &[]);
        assert_eq!(output.status.code(), Some(1), "{code}: {output:?}");
        assert!(
            text(&output.stderr).contains(needle),
            "{code}: {}",
            text(&output.stderr)
        );
    }
    // A runtime error inside a capturing closure stops the program as usual.
    let src = format!(
        "{body}    let z = 0\n    println(\"{{map([1], |x| x / z)}}\")\n    println(\"no\")\n}}\n"
    );
    for engine in ["--vm", "--jit"] {
        let output = run(&src, engine, &[]);
        assert_eq!(output.status.code(), Some(1), "{engine}: {output:?}");
        assert_eq!(text(&output.stdout), "", "{engine}");
        assert!(
            text(&output.stderr).contains("division by zero"),
            "{engine}"
        );
    }
}

#[test]
fn functions_take_call_and_return_functions() {
    let src = "import io { println }\n\
        fn apply(f: fn(Int) -> Int, x: Int) -> Int {\n    f(x)\n}\n\
        fn twice(f: fn(Int) -> Int) -> fn(Int) -> Int {\n    |x| f(f(x))\n}\n\
        fn adder(k: Int) -> fn(Int) -> Int {\n    |x| x + k\n}\n\
        fn double(x: Int) -> Int {\n    x * 2\n}\n\
        fn greet(name: String, f: fn(String) -> String) -> String {\n    f(name)\n}\n\
        fn run_it(f: fn()) {\n    f()\n}\n\
        struct Op {\n    name: String,\n    f: fn(Int) -> Int\n}\n\
        fn main() {\n    println(\"{apply(double, 4)} {apply(|x| x + 1, 4)}\")\n    let k = 100\n    println(\"{apply(|x| x + k, 4)}\")\n    let add2 = adder(2)\n    println(\"{add2(5)} {adder(3)(5)}\")\n    println(\"{twice(double)(3)} {twice(adder(1))(3)}\")\n    println(greet(\"a\", |s| s + \"!\"))\n    run_it(|| println(\"ran\"))\n    let d = double\n    println(\"{d(21)}\")\n    let op = Op { name: \"neg\", f: |x| 0 - x }\n    println(\"{op.name} {(op.f)(3)} {op.f(4)}\")\n    let fs = [double, adder(1)]\n    println(\"{fs[0](10)} {fs[1](10)}\")\n}\n";
    assert_stdout_on_both(src, "8 5\n104\n7 8\n12 5\na!\nran\n42\nneg -3 -4\n20 11\n");
}

#[test]
fn generic_functions_take_function_types() {
    let src = "import io { println }\n\
        import collections { push }\n\
        fn each[T, U](xs: [T], f: fn(T) -> U) -> [U] {\n    let mut out: [U] = []\n    for x in xs {\n        out = push(out, f(x))\n    }\n    out\n}\n\
        fn len_of(s: String) -> Int {\n    1\n}\n\
        fn main() {\n    println(\"{each([1], |x: Int| x + 1)} {each([\"a\"], len_of)}\")\n}\n";
    assert_stdout_on_both(src, "[2] [1]\n");
}

#[test]
fn a_direct_closure_call_captures_before_its_arguments_run() {
    let src = "fn main() -> Int {\n    let mut k = 1\n    return (|x: Int| k)(match 0 {\n        _ => {\n            k = 2\n            0\n        }\n    })\n}\n";
    for engine in ["--vm", "--jit"] {
        let output = run(src, engine, &[]);
        assert_eq!(output.status.code(), Some(1), "{engine}: {output:?}");
    }
}

#[test]
fn function_types_are_checked() {
    let body = "fn double(x: Int) -> Int {\n    x * 2\n}\nfn apply(f: fn(Int) -> Int, x: Int) -> Int {\n    f(x)\n}\n";
    let cases = [
        ("fn main() {\n    let y = apply(3, 1)\n}\n", "type mismatch"),
        (
            "fn s(x: String) -> Int {\n    1\n}\nfn main() {\n    let y = apply(s, 1)\n}\n",
            "type mismatch",
        ),
        (
            "fn main() {\n    let y = apply(|a: Int, b: Int| a, 1)\n}\n",
            "type mismatch",
        ),
        (
            "fn g(f: fn(Int) -> Int) -> String {\n    f(1)\n}\nfn main() {\n}\n",
            "type mismatch",
        ),
        (
            "fn g(f: fn(Int) -> Int) -> Int {\n    f(\"a\")\n}\nfn main() {\n}\n",
            "type mismatch",
        ),
        (
            "fn g(f: fn(Int) -> Int) -> Int {\n    f(1, 2)\n}\nfn main() {\n}\n",
            "type mismatch",
        ),
        (
            "fn main() {\n    let f = double\n    let y = f == double\n}\n",
            "type mismatch",
        ),
    ];
    for (code, needle) in cases {
        let output = run(&format!("{body}{code}"), "--vm", &[]);
        assert_eq!(output.status.code(), Some(1), "{code}: {output:?}");
        assert!(
            text(&output.stderr).contains(needle),
            "{code}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn function_types_survive_fmt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.fl");
    let src =
        "fn apply(f: fn(Int, [String]) -> Option[Int], g: fn()) -> fn(Int) -> fn() {\n    h\n}\n";
    fs::write(&path, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["fmt", path.to_str().unwrap()])
        .output()
        .expect("spawn flux");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(text(&output.stdout), src);
}

#[test]
fn a_closure_without_parameters_parses() {
    // `|| x` used to panic the parser.
    let src = "import io { println }\nfn call(f: fn() -> Int) -> Int {\n    f()\n}\nfn main() {\n    println(\"{call(|| 3)}\")\n}\n";
    assert_stdout_on_both(src, "3\n");
}

#[test]
fn a_local_annotation_can_name_a_type_parameter() {
    let src = "import io { println }\nimport option { None }\nfn f[T](x: T) -> [T] {\n    let a: [T] = [x]\n    let b: T = x\n    if true {\n        let c: Option[T] = None\n    }\n    a\n}\nfn main() {\n    println(\"{f(1)} {f(\"s\")}\")\n}\n";
    assert_stdout_on_both(src, "[1] [\"s\"]\n");
}

#[test]
fn derived_values_follow_what_a_function_value_reads() {
    let src = "import io { println }\n\
        event E(n: Int)\n\
        let mut s = 1\n\
        let f = |x: Int| x + s\n\
        let y = f(1)\n\
        rfn r(x: Int) -> Int {\n    x * s\n}\n\
        fn ap(g: fn(Int) -> Int) -> Int {\n    g(10)\n}\n\
        let z = ap(r)\n\
        watch y {\n    println(\"y={y}\")\n}\n\
        watch z {\n    println(\"z={z}\")\n}\n\
        on E { n ->\n    s = n\n}\n\
        fn main() {\n    emit E(5)\n}\n";
    assert_stdout_on_both(src, "y=2\nz=10\ny=6\nz=50\n");
}

#[test]
fn an_effect_cannot_reach_a_derived_value_through_a_function_value() {
    let prelude = "import io { println }\nfn loud(x: Int) -> Int {\n    println(\"side\")\n    x\n}\nfn ap(f: fn(Int) -> Int) -> Int {\n    f(1)\n}\n";
    for code in [
        "derived y = ap(loud)\n",
        "fn pass() -> Int {\n    ap(loud)\n}\nderived y = pass()\n",
        "derived y = ap(|x| loud(x))\n",
        "let f = loud\nderived y = f(1)\n",
        "let fs = [loud]\nderived y = fs[0](1)\n",
        "enum F {\n    Op(fn(Int) -> Int)\n}\nderived y = match Op(loud) {\n    Op(f) => f(1)\n}\n",
    ] {
        let output = run(&format!("{prelude}{code}"), "--vm", &[]);
        assert_eq!(output.status.code(), Some(1), "{code}: {output:?}");
        assert!(
            text(&output.stderr).contains("E017"),
            "{code}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn a_binding_that_holds_only_a_result_is_not_effectful() {
    let src = "import io { println }\nfn loud(x: Int) -> Int {\n    println(\"side\")\n    x\n}\nlet x = loud(1)\nderived y = x + 1\nfn main() {\n    println(\"ok\")\n}\n";
    assert_stdout_on_both(src, "side\nok\n");
}

#[test]
fn a_function_inside_an_enum_keeps_its_reactive_dependencies() {
    let src = "import io { println }\n\
        event Tick(n: Int)\n\
        enum F {\n    Op(fn(Int) -> Int)\n}\n\
        let mut s = 1\n\
        rfn r(x: Int) -> Int {\n    x + s\n}\n\
        let f = Op(r)\n\
        derived y = match f {\n    Op(g) => g(1)\n}\n\
        watch y {\n    println(\"{y}\")\n}\n\
        on Tick { n ->\n    s = n\n}\n\
        fn main() {\n    emit Tick(5)\n}\n";
    assert_stdout_on_both(src, "2\n6\n");
}

#[test]
fn return_inside_a_closure_is_a_compile_error() {
    let src = "fn main() -> Int {\n    let f: fn(Int) -> Int = |x: Int| match x {\n        0 => {\n            return 1\n            2\n        },\n        _ => 2\n    }\n    f(0)\n}\n";
    let output = run(src, "--vm", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        text(&output.stderr).contains("`return` inside a closure"),
        "{}",
        text(&output.stderr)
    );
}
