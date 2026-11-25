//! Places and records: assigning part of a value (`p.x = v`,
//! `todos[i].done = v`), compound assignment (`+=` and the like),
//! `p with { x: v }`, and the unused-result warning. Every program runs on
//! both engines, which must agree.

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

const TASK: &str = "import io { println }\nstruct Task {\n    name: String,\n    done: Bool,\n    hours: Int\n}\nstruct Board {\n    tasks: [Task],\n    title: String\n}\n";

#[test]
fn fields_and_paths_are_assigned_in_place() {
    prints(
        &format!("{TASK}fn main() {{\n    let mut b = Board {{ tasks: [Task {{ name: \"a\", done: false, hours: 1 }}, Task {{ name: \"b\", done: false, hours: 2 }}], title: \"t\" }}\n    let before = b\n    b.title = \"board\"\n    b.tasks[1].done = true\n    b.tasks[0].hours += 5\n    let mut grid = [[1, 2], [3, 4]]\n    grid[1][0] = 30\n    grid[0][1] -= 1\n    println(\"{{b.title}} {{b.tasks[0].hours}} {{b.tasks[1].done}} {{grid}}\")\n    println(\"{{before.title}} {{before.tasks[1].done}}\")\n}}\n"),
        "board 6 true [[1, 1], [30, 4]]\nt false\n",
    );
}

#[test]
fn each_index_is_evaluated_once_before_the_value() {
    prints(
        "import io { println }\nfn at(i: Int) -> Int {\n    println(\"index {i}\")\n    i\n}\nfn value(v: Int) -> Int {\n    println(\"value {v}\")\n    v\n}\nfn main() {\n    let mut grid = [[1, 2], [3, 4]]\n    grid[at(1)][at(0)] = value(9)\n    grid[at(0)][at(1)] += value(10)\n    println(\"{grid}\")\n}\n",
        "index 1\nindex 0\nvalue 9\nindex 0\nindex 1\nvalue 10\n[[1, 12], [9, 4]]\n",
    );
}

#[test]
fn compound_assignment_follows_its_operator() {
    prints(
        "import io { println }\nimport collections { map_new }\nfn main() {\n    let mut n = 7\n    n += 3\n    n -= 1\n    n *= 4\n    n /= 3\n    n %= 7\n    let mut x = 1.5\n    x *= 2.0\n    let mut s = \"ab\"\n    s += \"c\"\n    let mut m: Map[String, Int] = map_new()\n    m[\"a\"] = 1\n    m[\"a\"] += 2\n    println(\"{n} {x} {s} {m}\")\n}\n",
        "5 3.0 abc {\"a\": 3}\n",
    );
}

#[test]
fn a_failed_place_assignment_stops_with_its_error() {
    let cases = [
        ("let mut x = 120i8\n    x += 10i8", "integer overflow"),
        (
            "let mut xs = [[1]]\n    xs[0][5] = 2",
            "index out of bounds",
        ),
        (
            "let mut m: Map[String, [Int]] = map_new()\n    m[\"a\"][0] = 1",
            "key not found",
        ),
    ];
    for (body, error) in cases {
        let (status, stdout, stderr) = both(&format!(
            "import io {{ println }}\nimport collections {{ map_new }}\nfn main() {{\n    println(\"start\")\n    {body}\n    println(\"never\")\n}}\n"
        ));
        assert_eq!(status, Some(1), "{body}");
        assert_eq!(stdout, "start\n", "{body}");
        assert!(stderr.contains(error), "{body}\n{stderr}");
    }
}

#[test]
fn places_are_type_checked() {
    let head = "struct P {\n    x: Int\n}\n";
    rejects(
        &format!("{head}fn main() {{\n    let mut p = P {{ x: 1 }}\n    p.y = 2\n}}\n"),
        "expected `field `y``, found `P`",
    );
    rejects(
        &format!("{head}fn main() {{\n    let mut p = P {{ x: 1 }}\n    p.x = \"a\"\n}}\n"),
        "expected `Int64`, found `String`",
    );
    rejects(
        &format!("{head}fn main() {{\n    let p = P {{ x: 1 }}\n    p.x = 2\n}}\n"),
        "cannot assign to immutable local `p`",
    );
    rejects(
        "fn main() {\n    let mut n = 1\n    n += \"a\"\n}\n",
        "type mismatch",
    );
    rejects(
        "fn main() {\n    let mut xs = [1]\n    xs[0] += 1.5\n}\n",
        "expected `Int64`, found `Float64`",
    );
}

#[test]
fn a_field_of_a_source_is_a_write() {
    prints(
        "import io { println }\nstruct P {\n    x: Int\n}\nevent Go(n: Int)\nlet mut p = P { x: 1 }\nlet px = p.x * 10\nwatch px {\n    println(\"px={px}\")\n}\non Go(n) {\n    p.x += n\n}\nfn main() {\n    emit Go(2)\n    emit Go(0)\n}\n",
        "px=10\npx=30\n",
    );
    let (status, _, stderr) = both(
        "struct P {\n    x: Int\n}\nlet mut p = P { x: 1 }\nfn reset() {\n    p.x = 0\n}\nfn main() {\n}\n",
    );
    assert_eq!(status, Some(1));
    assert!(stderr.contains("`reset` changes `p`"), "{stderr}");
    assert!(stderr.contains("E027"), "{stderr}");
}

#[test]
fn with_copies_a_struct_with_some_fields_changed() {
    prints(
        "import io { println }\nstruct P {\n    x: Int,\n    y: Int,\n    name: String\n}\nlet mut p = P { x: 1, y: 2, name: \"p\" }\nlet moved = p with { x: p.x + 100 }\nfn make() -> P {\n    println(\"make\")\n    P { x: 0, y: 0, name: \"m\" }\n}\nfn main() {\n    let a = make() with { y: 5, name: \"a\" }\n    let b = a with { name: \"b\" }\n    println(\"{a.y} {a.name} {b.name} {b.y} {snap(moved).x}\")\n    let with = 3\n    println(\"{with + 1}\")\n}\n",
        "make\n5 a b 5 101\n4\n",
    );
    let head = "struct P {\n    x: Int\n}\nfn main() {\n    let p = P { x: 1 }\n";
    rejects(
        &format!("{head}    let q = p with {{ z: 2 }}\n}}\n"),
        "field `z`",
    );
    rejects(
        &format!("{head}    let q = p with {{ x: \"a\" }}\n}}\n"),
        "type mismatch",
    );
    rejects(
        &format!("{head}    let q = p with {{ x: 1, x: 2 }}\n}}\n"),
        "duplicate definition of `x`",
    );
    rejects(
        "fn main() {\n    let n = 1\n    let q = n with { x: 2 }\n}\n",
        "expected `a struct`",
    );
}

#[test]
fn a_dropped_collection_result_warns() {
    let (status, stdout, stderr) = both(
        "import io { println }\nimport collections { push, len, set, range }\nfn grow(xs: [Int]) -> [Int] {\n    push(xs, 1)\n}\nfn main() {\n    let mut xs = [1]\n    push(xs, 2)\n    len(xs)\n    for i in range(2) {\n        set(xs, 0, i)\n    }\n    println(\"{grow(xs)}\")\n}\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "[1, 1]\n");
    let warned: Vec<&str> = stderr
        .lines()
        .filter_map(|l| l.trim().strip_prefix("x the result of `"))
        .collect();
    assert_eq!(warned.len(), 3, "{stderr}");
    assert!(stderr.contains("write `xs = push(xs, v)`"), "{stderr}");
    assert!(stderr.contains("write `xs[i] = v`"), "{stderr}");
}

#[test]
fn places_and_with_survive_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let src = "struct P {\n    x: Int\n}\n\nfn main() {\n    let mut ps = [P { x: 1 }]\n    ps[0].x+=2\n    ps[0] = ps[0]   with {x:3}\n}\n";
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(stdout.contains("    ps[0].x += 2\n"), "{stdout}");
    assert!(
        stdout.contains("    ps[0] = ps[0] with { x: 3 }\n"),
        "{stdout}"
    );
    // Formatting again changes nothing.
    std::fs::write(dir.path().join("main.fl"), &stdout).unwrap();
    let (_, again, _) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(again, stdout);
}

#[test]
fn evaluation_order_holds_when_the_value_changes_the_root() {
    // `set` reads `xs` before its other arguments.
    prints(
        "import collections { set }\nimport io { println }\nfn main() {\n    let mut xs = [1]\n    xs = set(xs, 0, match 0 { _ => { xs = [3, 4]; 9 } })\n    println(\"{xs}\")\n}\n",
        "[9]\n",
    );
    // `part += v` reads the part before `v`.
    prints(
        "import io { println }\nfn main() {\n    let mut xs = [1]\n    xs[0] += match 0 { _ => { xs[0] = 10; 2 } }\n    println(\"{xs}\")\n}\n",
        "[3]\n",
    );
}

#[test]
fn a_watch_that_assigns_a_part_follows_the_whole_value() {
    let (status, stdout, stderr) = both(
        "import io { println }\nstruct P {\n    x: Int\n}\nlet mut p = P { x: 0 }\nwatch {\n    p.x += 0\n    println(\"ran\")\n}\nact fn change() {\n    p.x = 1\n}\nfn main() {\n    change()\n    println(\"{snap(p).x}\")\n}\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "ran\nran\n1\n");
    assert!(
        stderr.contains("this watch wakes itself through `p`"),
        "{stderr}"
    );
}

#[test]
fn with_gives_generic_calls_its_type() {
    prints(
        "import io { println }\nstruct P {\n    x: Int\n}\nfn id[T](v: T) -> T {\n    v\n}\nfn main() {\n    let p = P { x: 1 }\n    println(\"{id(p with { x: 2 }).x}\")\n}\n",
        "2\n",
    );
}

#[test]
fn a_dropped_result_inside_an_if_in_a_loop_warns() {
    let (_, _, stderr) = both(
        "import collections { push }\nfn main() {\n    let xs = [1]\n    for i in [0] {\n        if i == 0 {\n            push(xs, 3)\n        }\n    }\n}\n",
    );
    assert!(
        stderr.contains("the result of `push` is not used"),
        "{stderr}"
    );
}
