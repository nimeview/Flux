//! Trait bounds on type parameters (`[T: Ord + Display]`), the library
//! traits `Eq`, `Ord`, and `Display`, and impls for generic types
//! (`impl[T] Pair[T]`). Every program runs on both engines, which must agree.

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

/// Runs `files` (the first is `main.fl`) on both engines.
fn both_files(files: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    for (name, src) in files {
        std::fs::write(dir.path().join(name), src).unwrap();
    }
    let vm = flux(
        dir.path(),
        &["run", "--vm", "--no-cache", "--quiet", "main.fl"],
    );
    let jit = flux(
        dir.path(),
        &["run", "--jit", "--no-cache", "--quiet", "main.fl"],
    );
    assert_eq!(vm, jit, "{}", files[0].1);
    vm
}

fn prints(src: &str, want: &str) {
    let (status, stdout, stderr) = both_files(&[("main.fl", src)]);
    assert_eq!(status, Some(0), "{src}\n{stderr}");
    assert_eq!(stdout, want, "{src}");
}

fn rejects(src: &str, needles: &[&str]) {
    let (status, _, stderr) = both_files(&[("main.fl", src)]);
    assert_eq!(status, Some(1), "{src}");
    for needle in needles {
        assert!(
            stderr.contains(needle),
            "{src}\nmissing {needle:?} in\n{stderr}"
        );
    }
}

const DECLS: &str = "import io { println }

struct Pair[T] { a: T, b: T }

struct Point { x: Int, y: Int }

struct F { f: fn() -> Int }

trait Area {
    fn area(self) -> Int;
}

impl Area for Point {
    fn area(self) -> Int {
        self.x * self.y
    }
}

impl Display for Point {
    fn to_string(self) -> String {
        \"({self.x}, {self.y})\"
    }
}

impl[T] Pair[T] {
    fn new(a: T, b: T) -> Self {
        Self { a: a, b: b }
    }
    fn swap(self) -> Pair[T] {
        Pair { a: self.b, b: self.a }
    }
    fn first(self) -> T {
        self.a
    }
    fn set_first(mut self, v: T) {
        self.a = v
    }
}

impl[T: Display] Display for Pair[T] {
    fn to_string(self) -> String {
        \"<{self.a.to_string()}, {self.b.to_string()}>\"
    }
}

fn largest[T: Ord](xs: [T]) -> T {
    let mut best = xs[0]
    for x in xs {
        if x > best {
            best = x
        }
    }
    best
}

fn show[T: Display](x: T) -> String {
    x.to_string()
}

fn total[T: Area](xs: [T]) -> Int {
    let mut sum = 0
    for x in xs {
        sum += x.area()
    }
    sum
}

fn same[T: Eq](a: T, b: T) -> Bool {
    a == b
}

fn both[T: Ord + Display](a: T, b: T) -> String {
    if a < b { b.to_string() } else { a.to_string() }
}
";

#[test]
fn bounded_functions_and_generic_impls_run() {
    prints(
        &format!(
            "{DECLS}
fn main() {{
    let mut p = Pair::new(1, 2)
    let q = Pair {{ a: \"x\", b: \"y\" }}
    println(\"{{p.swap()}} {{q.swap()}} {{p.first()}} {{q.first()}}\")
    p.set_first(10)
    let words = [\"b\", \"a\"]
    let corners = Pair {{ a: Point {{ x: 0, y: 1 }}, b: Point {{ x: 2, y: 3 }} }}
    println(\"{{p}} {{largest([3, 9, 2])}} {{largest(words)}} {{both(4, 7)}}\")
    println(\"{{show(5)}} {{show(Point {{ x: 1, y: 2 }})}} {{show(p)}} {{show(corners)}}\")
    println(\"{{total([Point {{ x: 2, y: 3 }}, Point {{ x: 1, y: 1 }}])}} {{same(1, 1)}} {{same(p, p)}}\")
}}
"
        ),
        "Pair { a: 2, b: 1 } Pair { a: \"y\", b: \"x\" } 1 x\n\
         Pair { a: 10, b: 2 } 9 b 7\n\
         5 (1, 2) <10, 2> <(0, 1), (2, 3)>\n\
         7 true true\n",
    );
}

#[test]
fn generic_impls_work_on_enums_in_generic_code_and_across_modules() {
    let (status, stdout, stderr) = both_files(&[
        (
            "main.fl",
            "import io { println }
import shapes { Pair, Area }

enum Tree[T] { Leaf, Node { v: T, l: Tree[T], r: Tree[T] } }

impl[T: Ord] Tree[T] {
    fn size(self) -> Int {
        match self {
            Leaf => 0,
            Node { v, l, r } => 1 + l.size() + r.size()
        }
    }
    fn insert(self, x: T) -> Tree[T] {
        match self {
            Leaf => Node { v: x, l: Leaf, r: Leaf },
            Node { v, l, r } if x < v => Node { v: v, l: l.insert(x), r: r },
            Node { v, l, r } => Node { v: v, l: l, r: r.insert(x) }
        }
    }
}

struct Holder { p: Pair[Int] }

fn back[T](p: Pair[T]) -> T {
    p.swap().b
}

fn total[T: Area](x: T) -> Int {
    x.area()
}

fn main() {
    let s = \"x\"
    let h = Holder { p: Pair::new(1, 2) }
    let empty: Tree[String] = Leaf
    let t = empty.insert(\"b\").insert(\"a\").insert(\"c\")
    println(\"{Pair::new(1, 2).swap().swap()} {h.p.swap()} {back(Pair::new(5, 6))} {back(h.p)}\")
    println(\"{t.size()} {total(h.p)} {total(Pair::new(s, s))}\")
}
",
        ),
        (
            "shapes.fl",
            "pub struct Pair[T] { pub a: T, pub b: T }

pub trait Area {
    fn area(self) -> Int;
}

impl[T] Pair[T] {
    fn new(a: T, b: T) -> Self {
        Self { a: a, b: b }
    }
    fn swap(self) -> Pair[T] {
        Pair { a: self.b, b: self.a }
    }
}

impl[T] Area for Pair[T] {
    fn area(self) -> Int {
        2
    }
}
",
        ),
    ]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "Pair { a: 1, b: 2 } Pair { a: 2, b: 1 } 5 1\n3 2 2\n"
    );
}

#[test]
fn a_call_whose_type_lacks_a_bound_is_e031() {
    let main = |body: &str| format!("{DECLS}fn main() {{\n{body}}}\n");
    rejects(
        &main("    let x = largest([Point { x: 1, y: 2 }])\n"),
        &[
            "E031",
            "`Point` does not have trait `Ord`, which `largest` needs for `T`",
            "`Ord` holds for numbers and `String`",
        ],
    );
    rejects(
        &main("    let f = F { f: || 1 }\n    let x = show(f)\n"),
        &["`F` does not have trait `Display`", "impl Display for F"],
    );
    rejects(
        &main("    let f = F { f: || 1 }\n    let x = same(f, f)\n"),
        &["`F` does not have trait `Eq`"],
    );
    rejects(
        &main("    let x = total([Pair { a: 1, b: 2 }])\n"),
        &["`Pair[Int]` does not have trait `Area`"],
    );
    rejects(
        &main("    let f = F { f: || 1 }\n    let x = show(Pair { a: f, b: f })\n"),
        &[
            "`Pair[F]` does not have trait `Display`, which `show` needs for `T`",
            "its type arguments must meet that impl's bounds",
        ],
    );
    rejects(
        &main("    let x = both(Point { x: 1, y: 2 }, Point { x: 3, y: 4 })\n"),
        &["`Point` does not have trait `Ord`"],
    );
}

#[test]
fn bounds_and_generic_impls_are_written_one_way() {
    let with = |decl: &str| format!("{DECLS}{decl}\nfn main() {{\n}}\n");
    rejects(
        &with("fn g[T: Shiny](x: T) -> T {\n    x\n}"),
        &["unknown type `Shiny`"],
    );
    rejects(&with("impl Eq for Point {\n}"), &["`Eq` is built in"]);
    rejects(
        &with("impl Pair[Int] {\n    fn f(self) -> Int {\n        1\n    }\n}"),
        &["write `impl[T] Pair[T]` for all of them"],
    );
    rejects(
        &with("impl[T] Pair[Int] {\n    fn f(self) -> Int {\n        1\n    }\n}"),
        &["a generic impl gives its parameters to the type in order"],
    );
    rejects(
        &with("impl Display for F {\n    fn to_string(self) -> Int {\n        1\n    }\n}"),
        &["`Display` declares `fn to_string(self) -> String`"],
    );
}

/// A program's own trait named like a library one is the one bounds mean.
#[test]
fn a_declared_trait_named_display_is_the_programs_own() {
    prints(
        "import io { println }
trait Display {
    fn label(self) -> String;
}
impl Display for Int {
    fn label(self) -> String {
        \"int {self}\"
    }
}
fn show[T: Display](x: T) -> String {
    x.label()
}
fn main() {
    println(\"{show(3)}\")
}
",
        "int 3\n",
    );
}

/// Review findings: `Eq` ends on a type whose arguments grow; a method that
/// only shares a trait method's name does not give the trait; a module means
/// the library `Display` unless it sees its own; an unknown receiver asks
/// for an annotation; a generic method in an unused generic function, and
/// one without a declared result, are fine.
#[test]
fn trait_checks_and_generic_methods_hold_in_edge_cases() {
    prints(
        "import io { println }\nenum E[T] { End, Next(E[[T]]) }\nfn eq[T: Eq](x: T) -> Bool {\n    true\n}\nfn main() {\n    let e: E[Int] = End\n    println(\"{eq(e)}\")\n}\n",
        "true\n",
    );
    rejects(
        "struct P { x: Int }\nimpl P {\n    fn to_string(self) -> Bool {\n        true\n    }\n}\nfn needs[T: Display](x: T) -> Int {\n    99\n}\nfn main() {\n    let n = needs(P { x: 1 })\n}\n",
        &["`P` does not have trait `Display`"],
    );
    rejects(
        "enum E[T] { Empty, Value(T) }\nimpl[T] E[T] {\n    fn put(self, x: T) -> E[T] {\n        Value(x)\n    }\n}\nfn main() {\n    let a = Empty.put(1)\n}\n",
        &["a receiver whose type arguments are known where `put` is called"],
    );
    prints(
        "import io { println }\nstruct P[T] { x: T }\nimpl[T] P[T] {\n    fn get(self) {\n        self.x\n    }\n    fn read(self) -> T {\n        self.x\n    }\n}\nfn id[T](x: T) -> T {\n    x\n}\nfn unused[T](x: T) -> Int {\n    let p = P { x: 1 }\n    p.read()\n}\nfn main() {\n    println(\"{id(P { x: 1 }.get())}\")\n}\n",
        "1\n",
    );
    let (status, stdout, stderr) = both_files(&[
        (
            "main.fl",
            "import io { println }\nimport shapez { show }\ntrait Display {\n    fn label(self) -> String;\n}\nfn main() {\n    println(show(1))\n}\n",
        ),
        (
            "shapez.fl",
            "pub fn show[T: Display](x: T) -> String {\n    x.to_string()\n}\n",
        ),
    ]);
    assert_eq!((status, stdout.as_str()), (Some(0), "1\n"), "{stderr}");
}

/// Second review round: a method that calls itself with no declared result
/// does not crash the compiler, and `Eq` looks at each sibling on its own.
#[test]
fn recursive_inferred_methods_and_sibling_instances_check() {
    let (status, _, stderr) = both_files(&[(
        "main.fl",
        "struct P[T] { x: T }\nimpl[T] P[T] {\n    fn f(self) {\n        self.f()\n    }\n}\nfn main() {\n}\n",
    )]);
    assert_eq!(status, Some(0), "{stderr}");
    prints(
        "import io { println }\nstruct P[T] { x: Int }\nfn eq[T: Eq](x: T) -> Bool {\n    true\n}\nfn main() {\n    let a: P[Int] = P { x: 1 }\n    let b: P[fn() -> Int] = P { x: 1 }\n    println(\"{eq((a, b))} {eq((b, a))}\")\n}\n",
        "true true\n",
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "struct P[T] { x: T }\nimpl[T] P[T] {\n    fn f(self) {\n        self.f()\n    }\n}\nfn main() {\n    let x = P { x: 1 }.f()\n}\n",
    )
    .unwrap();
    let (status, _, stderr) = flux(dir.path(), &["check", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
}
