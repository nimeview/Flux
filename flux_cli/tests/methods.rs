//! Paths and methods: `a::b`, `(op.f)(3)`, `impl Type { }` with `self`,
//! `mut self`, type functions and `Self`, traits with `Self`, and methods on
//! library types, scoped to the files that import them. Every program runs
//! on both engines, which must agree.

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

const POINT: &str = "import io { println }\nstruct Point {\n    x: Int,\n    y: Int\n}\nimpl Point {\n    fn new(x: Int, y: Int) -> Self {\n        Self { x: x, y: y }\n    }\n    fn origin() -> Point {\n        Self::new(0, 0)\n    }\n    fn len2(self) -> Int {\n        self.x * self.x + self.y * self.y\n    }\n    fn plus(self, other: Self) -> Self {\n        Point::new(self.x + other.x, self.y + other.y)\n    }\n    fn map(self, f: fn(Int) -> Int) -> Point {\n        Point { x: f(self.x), y: f(self.y) }\n    }\n    fn shift(mut self, by: Int) {\n        self.x += by\n        self.y += by\n    }\n}\n";

#[test]
fn methods_type_functions_and_self() {
    prints(
        &format!("{POINT}fn main() {{\n    let p = Point::new(3, 4)\n    let q = p.plus(Point::origin()).plus(Point::new(1, 1))\n    let k = 10\n    println(\"{{p.len2()}} {{q.x}} {{q.y}} {{p.map(|v| v * k).x}}\")\n}}\n"),
        "25 4 5 30\n",
    );
}

#[test]
fn mut_self_changes_the_place_it_is_called_on() {
    prints(
        &format!("{POINT}struct Path {{\n    points: [Point]\n}}\nfn at(i: Int) -> Int {{\n    println(\"index {{i}}\")\n    i\n}}\nfn main() {{\n    let mut p = Point::new(1, 2)\n    let before = p\n    p.shift(10)\n    let mut path = Path {{ points: [p, Point::origin()] }}\n    path.points[at(1)].shift(5)\n    println(\"{{p.x}} {{p.y}} {{before.x}} {{path.points[1].x}}\")\n}}\n"),
        "index 1\n11 12 1 5\n",
    );
}

#[test]
fn a_failed_mut_self_method_leaves_a_source_unchanged() {
    let (status, stdout, stderr) = both(
        "import io { println }\nstruct C {\n    n: Int,\n    d: Int\n}\nimpl C {\n    fn split(mut self) {\n        self.n = 99\n        self.n = 10 / self.d\n    }\n}\nevent Go\nlet mut c = C { n: 1, d: 0 }\nwatch c {\n    println(\"n={c.n}\")\n}\non Go {\n    c.split()\n}\nfn main() {\n    emit Go\n}\n",
    );
    assert_eq!(status, Some(1));
    assert_eq!(stdout, "n=1\n");
    assert!(stderr.contains("integer division by zero"), "{stderr}");
}

#[test]
fn mut_self_mistakes_are_reported() {
    let head = "struct C {\n    n: Int\n}\nimpl C {\n    fn bump(mut self) {\n        self.n += 1\n    }\n}\n";
    rejects(
        &format!("{head}fn main() {{\n    let mut c = C {{ n: 1 }}\n    let x = c.bump()\n}}\n"),
        "a call of the `mut self` method `bump` inside an expression",
    );
    rejects(
        &format!("{head}fn main() {{\n    let c = C {{ n: 1 }}\n    c.bump()\n}}\n"),
        "cannot assign to immutable local `c`",
    );
    rejects(
        &format!(
            "{head}fn make() -> C {{\n    C {{ n: 1 }}\n}}\nfn main() {{\n    make().bump()\n}}\n"
        ),
        "inside an expression",
    );
    rejects(
        "struct C {\n    n: Int\n}\nimpl C {\n    fn twice(mut self) -> Int {\n        1\n    }\n}\nfn main() {\n}\n",
        "cannot also return a value",
    );
}

#[test]
fn method_mistakes_are_reported() {
    rejects(
        "struct P {\n    x: Int\n}\nimpl P {\n    fn a(self) -> Int {\n        1\n    }\n}\nimpl P {\n    fn a(self) -> Int {\n        2\n    }\n}\nfn main() {\n}\n",
        "`P` already has a method `a`",
    );
    rejects(
        "fn main() {\n    let x = Self::new()\n}\n",
        "`Self` is only known inside an `impl`",
    );
    rejects(
        "fn f(self) -> Int {\n    1\n}\nfn main() {\n}\n",
        "`self` is only for methods inside an `impl`",
    );
}

#[test]
fn traits_may_use_self() {
    prints(
        "import io { println }\ntrait Shape {\n    fn area(self) -> Int;\n    fn grow(self, by: Int) -> Self;\n}\nstruct Sq {\n    side: Int\n}\nimpl Shape for Sq {\n    fn area(self) -> Int {\n        self.side * self.side\n    }\n    fn grow(self, by: Int) -> Self {\n        Sq { side: self.side + by }\n    }\n}\nfn main() {\n    let s = Sq { side: 2 }\n    println(\"{s.area()} {s.grow(3).area()}\")\n}\n",
        "4 25\n",
    );
}

#[test]
fn paths_name_module_items_and_variants() {
    prints(
        "import io\nimport collections as c\nenum Shape {\n    Circle(Int),\n    Dot\n}\nfn area(s: Shape) -> Int {\n    match s {\n        Shape::Circle(r) => 3 * r * r,\n        Shape::Dot => 0\n    }\n}\nfn main() {\n    let xs = [Shape::Circle(2), Circle(1), Shape::Dot]\n    let n = match Some(Shape::Dot) {\n        Some(Shape::Dot) => 1,\n        _ => 0\n    }\n    io::println(\"{area(xs[0])} {area(xs[1])} {area(xs[2])} {n} {c::len(xs)}\")\n}\n",
        "12 3 0 1 3\n",
    );
    rejects(
        "enum S {\n    A\n}\nfn main() {\n    let x = S::B\n}\n",
        "enum `S` has no variant `B`",
    );
    rejects(
        "fn main() {\n    let x = a::b::c\n}\n",
        "a path has two parts",
    );
}

#[test]
fn older_forms_warn_and_migrate() {
    let dir = tempfile::tempdir().unwrap();
    let before = "import io as out\n\nstruct Op {\n    f: fn(Int) -> Int\n}\n\nfn main() {\n    let op = Op { f: |x| x * 2 }\n    out.println(\"{(op.f)(3)} {op.f(4)}\")\n}\n";
    std::fs::write(dir.path().join("main.fl"), before).unwrap();
    let run = ["run", "--vm", "--no-cache", "--quiet", "main.fl"];
    let (status, out_before, stderr) = flux(dir.path(), &run);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(out_before, "6 8\n");
    assert!(
        stderr.contains("`out.println` is now written `out::println`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("`op.f(…)` is now written `(op.f)(…)`"),
        "{stderr}"
    );
    let (status, _, stderr) = flux(dir.path(), &["migrate", "--write", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    let after = std::fs::read_to_string(dir.path().join("main.fl")).unwrap();
    assert_eq!(
        after,
        before
            .replace("out.println", "out::println")
            .replace("{op.f(4)}", "{(op.f)(4)}")
    );
    let (_, out_after, stderr) = flux(dir.path(), &run);
    assert_eq!(out_after, out_before);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn methods_on_library_types_are_seen_where_their_file_is_imported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("text.fl"),
        "pub fn noop() -> Int {\n    0\n}\nimpl String {\n    fn shout(self) -> String {\n        self + \"!\"\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("other.fl"),
        "pub fn use_it() -> String {\n    \"x\".shout()\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "import io { println }\nimport text\nfn main() {\n    let s = \"hi\".shout()\n    println(\"{s} {text::noop()}\")\n}\n",
    )
    .unwrap();
    for engine in ["--vm", "--jit"] {
        let (status, stdout, stderr) = flux(
            dir.path(),
            &["run", engine, "--no-cache", "--quiet", "main.fl"],
        );
        assert_eq!(status, Some(0), "{stderr}");
        assert_eq!(stdout, "hi! 0\n");
    }
    std::fs::write(
        dir.path().join("main.fl"),
        "import io { println }\nimport text\nimport other\nfn main() {\n    println(other::use_it())\n}\n",
    )
    .unwrap();
    let (status, _, stderr) = flux(dir.path(), &["run", "--vm", "--no-cache", "main.fl"]);
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("added to `String` in a module this file does not import"),
        "{stderr}"
    );
}

#[test]
fn methods_survive_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let src = "struct C {\n    n: Int\n}\n\nimpl C {\n    fn new( )->Self {\n        Self { n: 0 }\n    }\n    fn bump(mut self,by:Int) {\n        self.n += by\n    }\n}\n\nfn main() {\n    let mut c = C::new()\n    c.bump(1)\n}\n";
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(stdout.contains("impl C {\n"), "{stdout}");
    assert!(stdout.contains("    fn new() -> Self {"), "{stdout}");
    assert!(
        stdout.contains("    fn bump(mut self, by: Int) {"),
        "{stdout}"
    );
    assert!(stdout.contains("    let mut c = C::new()"), "{stdout}");
    std::fs::write(dir.path().join("main.fl"), &stdout).unwrap();
    let (_, again, _) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(again, stdout);
}

#[test]
fn review_findings_stay_fixed() {
    // Two type functions with one name.
    rejects(
        "import io { println }\nstruct C {\n    n: Int\n}\nimpl C {\n    fn make() -> Int {\n        1\n    }\n}\nimpl C {\n    fn make() -> Int {\n        2\n    }\n}\nfn main() {\n    println(\"{C::make()}\")\n}\n",
        "`C` already has a function `make`",
    );
    // A `mut self` call on an immutable module binding.
    rejects(
        "import io { println }\nstruct C {\n    n: Int\n}\nimpl C {\n    fn bump(mut self) {\n        self.n += 1\n    }\n}\nlet c = C { n: 0 }\nevent Go\non Go {\n    c.bump()\n    println(\"{c.n}\")\n}\nfn main() {\n    emit Go\n}\n",
        "declare it with `let mut`",
    );
    // An unknown path in a pattern is not a binding.
    rejects(
        "import io { println }\nenum E {\n    A\n}\nfn main() {\n    println(\"{match E::A { Bogus::B => 7 }}\")\n}\n",
        "`Bogus::B` is not a variant",
    );
    // A struct the module does not export.
    rejects(
        "import io\nstruct P {\n    x: Int\n}\nfn main() {\n    let p = io::P { x: 1 }\n}\n",
        "unknown export `P` in module `io`",
    );
    // Another type's method with a `mut self` method's name is just called.
    prints(
        "import io { println }\nstruct A {\n    n: Int\n}\nimpl A {\n    fn tick(mut self) {\n        self.n += 1\n    }\n}\nstruct B {\n    n: Int\n}\nimpl B {\n    fn tick(self) {\n        println(\"b tick {self.n}\")\n    }\n}\nfn main() {\n    let b = B { n: 3 }\n    b.tick()\n    let mut a = A { n: 0 }\n    a.tick()\n    println(\"{a.n}\")\n}\n",
        "b tick 3\n1\n",
    );
    // A bare `return` keeps the receiver as it is then.
    prints(
        "import io { println }\nstruct C {\n    n: Int\n}\nimpl C {\n    fn tick(mut self) {\n        self.n += 10\n        if self.n > 15 {\n            return\n        }\n        self.n += 1\n    }\n}\nfn main() {\n    let mut c = C { n: 0 }\n    c.tick()\n    c.tick()\n    println(\"{c.n}\")\n}\n",
        "21\n",
    );
    // `Self` in a trait matches the type in the impl; enums have type
    // functions; two enums may share a variant name.
    prints(
        "import io { println }\ntrait Cl {\n    fn copy(self) -> Self;\n}\nstruct C {\n    n: Int\n}\nimpl Cl for C {\n    fn copy(self) -> C {\n        self\n    }\n}\nenum A {\n    X\n}\nenum B {\n    X\n}\nimpl A {\n    fn make() -> Self {\n        A::X\n    }\n}\nfn main() {\n    println(\"{C { n: 4 }.copy().n} {A::make()} {B::X}\")\n}\n",
        "4 A::X B::X\n",
    );
    // A source-changing method in a plain fn warns (E027) but runs.
    let (status, _, stderr) = both(
        "struct C {\n    n: Int\n}\nimpl C {\n    fn bump(mut self) {\n        self.n += 1\n    }\n}\nlet mut c = C { n: 0 }\nfn change() {\n    c.bump()\n}\nfn main() {\n    change()\n}\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert!(
        stderr.contains("E027") && !stderr.contains("E002"),
        "{stderr}"
    );
}
