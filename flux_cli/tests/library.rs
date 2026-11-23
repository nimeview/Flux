//! Library methods: methods on strings, arrays, maps, `Option`, `Result`, and
//! numbers; the changing ones (`push`, `sort`, …) on places; the new library
//! functions. Every program runs on both engines, which must agree.

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
fn strings_arrays_maps_options_and_numbers_have_methods() {
    prints(
        "import io { println }\nimport collections { map_new }\nfn main() {\n    let text = \"a, b,c\"\n    let padded = \"  x \"\n    let parts = text.split(\",\")\n    println(\"{parts.len()} {text.len()} {text.to_upper()} {padded.trim()}\")\n    let xs = [3, 1, 2]\n    println(\"{xs.contains(2)} {xs.sorted()} {xs.reversed()} {xs.first()} {xs.last()} {xs.slice(0, 2)}\")\n    println(\"{xs.map(|x| x * 2)} {xs.filter(|x| x > 1)} {xs.reduce(0, |a, x| a + x)}\")\n    println(\"{xs.find(|x| x > 1)} {xs.any(|x| x == 9)} {xs.all(|x| x > 0)}\")\n    let names = [\"a\", \"b\"]\n    println(\"{xs.enumerate()} {xs.zip(names)} {xs + [4]}\")\n    let o = Some(4)\n    let n: Option[Int] = None\n    let missing = \"none\"\n    println(\"{o.unwrap_or(0)} {n.unwrap_or(7)} {o.map(|v| v + 1)} {o.is_some()} {n.is_none()} {o.ok_or(missing)}\")\n    let r: Result[Int, String] = Err(\"bad\")\n    let bang = \"!\"\n    println(\"{r.unwrap_or(1)} {r.map_err(|e| e + bang)} {r.is_err()} {Ok(2).and_then(|v| Ok(v * 3))}\")\n    let mut m: Map[String, Int] = map_new()\n    m[\"y\"] = 2\n    let (x, y) = (\"x\", \"y\")\n    println(\"{m.get_or(x, 0)} {m.get_or(y, 0)} {m.entries()} {m.has(y)} {m.keys()}\")\n    let pair = \"k=v\"\n    let two = \"a\\nb\"\n    let spaced = \" one  two \"\n    let eq = \"=\"\n    println(\"{pair.split_once(eq)} {two.lines()} {spaced.words()} {(-3).abs()}\")\n}\n",
        "3 6 A, B,C x\ntrue [1, 2, 3] [2, 1, 3] Some(3) Some(2) [3, 1]\n[6, 2, 4] [3, 2] 6\nSome(3) false true\n[(0, 3), (1, 1), (2, 2)] [(3, \"a\"), (1, \"b\")] [3, 1, 2, 4]\n4 7 Some(5) true true Ok(4)\n1 Err(\"bad!\") true Ok(6)\n0 2 [(\"y\", 2)] true [\"y\"]\nSome((\"k\", \"v\")) [\"a\", \"b\"] [\"one\", \"two\"] 3\n",
    );
}

#[test]
fn changing_methods_change_the_place() {
    prints(
        "import io { println }\nimport collections { map_new, push }\nstruct Bag {\n    items: [Int]\n}\nfn main() {\n    let mut xs = [3, 1, 2]\n    xs.push(5)\n    xs.sort()\n    let copy = xs\n    xs.reverse()\n    xs.pop()\n    xs.set(0, 9)\n    let mut bag = Bag { items: [2, 1] }\n    bag.items.sort_by(|x| x)\n    let mut m: Map[String, Int] = map_new()\n    m.set(\"a\", 1)\n    m.set(\"b\", 2)\n    m.remove(\"a\")\n    let fresh = push(xs, 7)\n    println(\"{xs} {copy} {bag.items} {m} {fresh}\")\n}\n",
        "[9, 3, 2] [1, 2, 3, 5] [1, 2] {\"b\": 2} [9, 3, 2, 7]\n",
    );
}

#[test]
fn sorted_by_keeps_equal_keys_in_order() {
    prints(
        "import io { println }\nstruct P {\n    name: String,\n    age: Int\n}\nfn main() {\n    let ps = [P { name: \"c\", age: 2 }, P { name: \"a\", age: 1 }, P { name: \"b\", age: 2 }]\n    let by_age = ps.sorted_by(|p| p.age).map(|p| p.name)\n    let by_name = ps.sorted_by(|p| p.name).map(|p| p.name)\n    println(\"{by_age} {by_name}\")\n}\n",
        "[\"a\", \"c\", \"b\"] [\"a\", \"b\", \"c\"]\n",
    );
}

#[test]
fn library_method_mistakes_are_reported() {
    rejects(
        "fn main() {\n    let mut xs = [1]\n    let y = xs.push(2)\n}\n",
        "`push`, which changes its receiver, inside an expression",
    );
    rejects(
        "fn main() {\n    let xs = [1]\n    xs.push(2)\n}\n",
        "cannot assign to immutable local `xs`",
    );
    rejects(
        "struct P {\n    a: Int\n}\nfn main() {\n    let mut xs = [P { a: 1 }]\n    xs.sort()\n}\n",
        "an array of numbers or strings",
    );
    rejects(
        "fn main() {\n    let xs = [1]\n    let y = xs.nope()\n}\n",
        "no method `nope`",
    );
    rejects(
        "fn main() {\n    let xs = [1]\n    let y = xs.contains(\"a\")\n}\n",
        "expected `Int64`, found `String`",
    );
    rejects(
        "fn main() {\n    let o = Some(1)\n    let y = o.and_then(|v| v + 1)\n}\n",
        "a function returning an Option",
    );
    let (status, stdout, stderr) =
        both("import io { println }\nfn main() {\n    println(\"start\")\n    let xs = [1, 2]\n    println(\"{xs.slice(1, 5)}\")\n}\n");
    assert_eq!(status, Some(1));
    assert_eq!(stdout, "start\n");
    assert!(stderr.contains("index out of bounds"), "{stderr}");
}

#[test]
fn a_types_own_method_comes_before_the_librarys() {
    prints(
        "import io { println }\nimpl String {\n    fn len(self) -> Int {\n        100\n    }\n}\nfn find(x: Int) -> Int {\n    x + 1\n}\nfn main() {\n    let s = \"abc\"\n    let xs = [5]\n    println(\"{s.len()} {find(1)} {xs.find(|x| x > 1)}\")\n}\n",
        "100 2 Some(5)\n",
    );
}

#[test]
fn library_method_types_follow_their_arguments() {
    // `[] + [1]` is `[Int]`; `and_then` keeps the receiver's error type;
    // `get_or` and `unwrap_or` take the default's type; a generic argument
    // is inferred through a library method.
    prints(
        r#"import io { println }
import collections { map_new }
fn id[T](x: T) -> T {
    x
}
fn main() {
    let joined = [] + [1]
    println("{joined[0] + 1}")
    let r: Result[Int, String] = Ok(1)
    let s = r.and_then(|v| Ok(v + 1))
    let t: Result[Int, String] = s
    let m: Map[String, Int] = map_new()
    let k = "k"
    let g = m.get_or(k, 3)
    println("{t} {g + 1} {id("abc".len()) + 1}")
}
"#,
        "2\nOk(2) 4 4\n",
    );
}

#[test]
fn only_library_changing_methods_store_back() {
    // A user's `mut self` method named `sorted` does not make the library's
    // `sorted` store back; a module `let` changed by `push` in a handler is
    // a reactive source; a captured receiver may call a method that does
    // not change it.
    prints(
        r#"import io { println }
struct Box {
    v: Int
}
impl Box {
    fn push(self, n: Int) -> Int {
        self.v + n
    }
    fn sorted(mut self) {
        self.v = 0
    }
}
event Tick
let mut log = [1]
on Tick {
    log.push(2)
}
fn main() {
    let mut xs = [3, 1]
    let ys = xs.sorted()
    let b = Box { v: 1 }
    let f = |n: Int| match n {
        _ => {
            b.push(n)
            n + b.v
        }
    }
    emit Tick()
    println("{xs} {ys} {f(2)} {snap(log)}")
}
"#,
        "[3, 1] [1, 3] 3 [1, 2]\n",
    );
    rejects(
        "fn main() {\n    let mut xs = [1]\n    let f = |n: Int| match n {\n        _ => {\n            xs.push(n)\n            n\n        }\n    }\n    let y = f(1)\n}\n",
        "cannot assign to immutable local `xs`",
    );
}

#[test]
fn a_huge_slice_index_is_out_of_bounds() {
    let (status, stdout, stderr) = both(
        "import io { println }\nfn main() {\n    let xs = [1, 2]\n    let i = 9223372036854775807 as UInt64 + 1 as UInt64\n    println(\"start\")\n    println(\"{xs.slice(0, i)}\")\n}\n",
    );
    assert_eq!(status, Some(1));
    assert_eq!(stdout, "start\n");
    assert!(stderr.contains("index out of bounds"), "{stderr}");
}

#[test]
fn a_dropped_method_result_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "fn main() {\n    let xs = [3, 1]\n    for x in xs {\n        xs.sorted()\n    }\n}\n",
    )
    .unwrap();
    let (status, _, stderr) = flux(dir.path(), &["check", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(stderr.contains("E030"), "{stderr}");
    assert!(
        stderr.contains("`.sorted()` only computes a value"),
        "{stderr}"
    );
}
