//! Module contract: every module has its own namespace (all of its items plus
//! its own imports, stdlib included); only `pub` items are exported; a
//! module's imports resolve from its directory and its own package's
//! dependencies; a module imported twice is one instance; a module never
//! sees the root's names. Type names are still program-global.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write_tree(root: &Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

fn flux(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("spawn flux")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs `dir/<cwd>/main.fl` on both engines and returns stdout (must succeed).
fn run_ok(files: &[(&str, &str)], cwd: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    write_tree(dir.path(), files);
    let mut outputs = Vec::new();
    for engine in ["--vm", "--jit"] {
        let out = flux(&["run", engine, "main.fl"], &dir.path().join(cwd));
        assert_eq!(
            out.status.code(),
            Some(0),
            "{engine}: stdout={} stderr={}",
            text(&out.stdout),
            text(&out.stderr)
        );
        outputs.push(text(&out.stdout));
    }
    assert_eq!(outputs[0], outputs[1], "VM and JIT differ");
    outputs.remove(0)
}

/// `flux check` must fail with `needle` in stderr.
fn check_fails(files: &[(&str, &str)], cwd: &str, needle: &str) {
    let dir = tempfile::tempdir().unwrap();
    write_tree(dir.path(), files);
    let out = flux(&["check", "main.fl"], &dir.path().join(cwd));
    let err = text(&out.stderr);
    assert!(!out.status.success(), "expected failure: {err}");
    assert!(err.contains(needle), "missing `{needle}` in:\n{err}");
}

const MAIN_TWICE: &str = "import io { println }\nimport lib { twice }\nfn main() -> Int {\n  println(twice(5))\n  0\n}\n";

#[test]
fn pub_fn_can_use_private_items_of_its_module() {
    let out = run_ok(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "fn helper(x: Int) -> Int { x + 1 }\nconst BASE = 100\npub fn twice(x: Int) -> Int { helper(x) * 2 + BASE }\n",
            ),
        ],
        ".",
    );
    assert_eq!(out, "112\n");
}

#[test]
fn a_module_uses_its_own_imports_selected_aliased_and_stdlib() {
    let out = run_ok(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "import helpers { inc }\nimport helpers as h\nimport str { to_string }\npub fn twice(x: Int) -> Int { inc(x) + h.inc(x) }\npub fn show(x: Int) -> String { to_string(x) }\n",
            ),
            ("helpers.fl", "pub fn inc(x: Int) -> Int { x + 1 }\n"),
        ],
        ".",
    );
    assert_eq!(out, "12\n");
}

#[test]
fn same_private_name_in_two_modules_stays_separate() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { fa }\nimport b { fb }\nfn main() -> Int {\n  println(fa() * 10 + fb())\n  0\n}\n",
            ),
            ("a.fl", "fn helper() -> Int { 1 }\npub fn fa() -> Int { helper() }\n"),
            ("b.fl", "fn helper() -> Int { 2 }\npub fn fb() -> Int { helper() }\n"),
        ],
        ".",
    );
    assert_eq!(out, "12\n");
}

#[test]
fn diamond_import_is_one_module() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { fa }\nimport b { fb }\nfn main() -> Int {\n  println(fa() + fb())\n  0\n}\n",
            ),
            ("a.fl", "import shared { base }\npub fn fa() -> Int { base() + 1 }\n"),
            ("b.fl", "import shared { base }\npub fn fb() -> Int { base() + 2 }\n"),
            ("shared.fl", "pub fn base() -> Int { 10 }\n"),
        ],
        ".",
    );
    assert_eq!(out, "23\n");
}

#[test]
fn transitive_path_dependencies_resolve_per_package() {
    let out = run_ok(
        &[
            (
                "app/flux.toml",
                "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n",
            ),
            ("app/main.fl", MAIN_TWICE),
            (
                "lib/flux.toml",
                "[package]\nname = \"lib\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
            ),
            (
                "lib/lib.fl",
                "import base { one }\npub fn twice(x: Int) -> Int { x * 2 + one() }\n",
            ),
            ("base/flux.toml", "[package]\nname = \"base\"\n"),
            ("base/lib.fl", "pub fn one() -> Int { 1 }\n"),
        ],
        "app",
    );
    assert_eq!(out, "11\n");
}

#[test]
fn a_package_cannot_import_its_dependencys_dependencies() {
    check_fails(
        &[
            (
                "app/flux.toml",
                "[package]\nname = \"app\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n",
            ),
            ("app/main.fl", "import base { one }\nlet a = one()\n"),
            (
                "lib/flux.toml",
                "[package]\nname = \"lib\"\n\n[dependencies]\nbase = { path = \"../base\" }\n",
            ),
            ("lib/lib.fl", "pub fn twice(x: Int) -> Int { x * 2 }\n"),
            ("base/flux.toml", "[package]\nname = \"base\"\n"),
            ("base/lib.fl", "pub fn one() -> Int { 1 }\n"),
        ],
        "app",
        "module not found: base",
    );
}

#[test]
fn a_module_does_not_see_the_roots_names() {
    check_fails(
        &[
            (
                "main.fl",
                "import lib { f }\nconst SECRET = 1\nlet a = f()\n",
            ),
            ("lib.fl", "pub fn f() -> Int { SECRET }\n"),
        ],
        ".",
        "undefined name `SECRET`",
    );
}

#[test]
fn private_items_of_a_nested_import_stay_private() {
    check_fails(
        &[
            ("main.fl", MAIN_TWICE),
            (
                "lib.fl",
                "import helpers { hidden }\npub fn twice(x: Int) -> Int { hidden(x) }\n",
            ),
            ("helpers.fl", "fn hidden(x: Int) -> Int { x }\n"),
        ],
        ".",
        "`hidden` in module `helpers` is private",
    );
}

/// A module's source changes only in that module (E032): the importer
/// asks for a change through the module's event, and reads the result.
#[test]
fn imported_reactive_binding_drives_root_derived_values() {
    check_fails(
        &[
            (
                "main.fl",
                "import counter { count }\nevent Tick\non Tick {\n  count = count + 1\n}\n",
            ),
            ("counter.fl", "pub let mut count = 1\n"),
        ],
        ".",
        "`count` belongs to another module",
    );
    let dir = tempfile::tempdir().unwrap();
    write_tree(
        dir.path(),
        &[
            (
                "main.fl",
                "import counter { count, Tick }\nlet doubled = count * 2\n",
            ),
            (
                "counter.fl",
                "pub let mut count = 1\npub event Tick\non Tick {\n  count = count + 1\n}\n",
            ),
        ],
    );
    let compiled = flux_cli::pipeline::compile_file(
        &dir.path().join("main.fl"),
        flux_runtime::ExecutionMode::Vm,
    )
    .unwrap_or_else(|f| panic!("{:?}", f.diagnostics.errors));
    let mut ex = compiled.executor;
    assert_eq!(ex.get_number("doubled"), Some(2.0));
    ex.emit("__mod_1__Tick", flux_runtime::RuntimeValue::Unit);
    assert_eq!(ex.get_number("doubled"), Some(4.0));
}

#[test]
fn imported_struct_type_is_usable_and_type_names_are_per_module() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { mk, Pt }\nfn main() -> Int {\n  let p = mk()\n  println(p.x)\n  0\n}\n",
            ),
            ("a.fl", "pub struct Pt { pub x: Int }\npub fn mk() -> Pt { Pt { x: 7 } }\n"),
        ],
        ".",
    );
    assert_eq!(out, "7\n");
    // Two modules may each declare `Pt`; each is its own type.
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport a { mk }\nimport b { mk2 }\nfn main() -> Int {\n  println(\"{mk()} {mk2()}\")\n  0\n}\n",
            ),
            (
                "a.fl",
                "pub struct Pt { pub x: Int }\npub fn mk() -> Pt { Pt { x: 1 } }\n",
            ),
            (
                "b.fl",
                "pub struct Pt { pub y: Int }\npub fn mk2() -> Pt { Pt { y: 2 } }\n",
            ),
        ],
        ".",
    );
    assert_eq!(out, "Pt { x: 1 } Pt { y: 2 }\n");
    // A type of another module needs an import, and `pub`.
    check_fails(
        &[
            ("main.fl", "import a { mk }\nlet p: Pt = mk()\n"),
            (
                "a.fl",
                "pub struct Pt { pub x: Int }\npub fn mk() -> Pt { Pt { x: 1 } }\n",
            ),
        ],
        ".",
        "unknown type `Pt`",
    );
    check_fails(
        &[
            ("main.fl", "import a\nlet p = a::Pt { x: 1 }\n"),
            ("a.fl", "struct Pt { x: Int }\n"),
        ],
        ".",
        "`Pt` in module `a` is private",
    );
}

#[test]
fn enums_traits_and_type_functions_of_modules_keep_their_own_names() {
    // `shapes` and `other` both have a variant `Circle`, and the root a third:
    // the root's own wins bare, imported ones are bare once their enum is
    // imported, and every one is reachable qualified.
    let out = run_ok(
        &[
            ("shapes.fl", "pub enum Shape {\n    Circle(Float),\n    Rect { w: Float, h: Float },\n    Dot\n}\n\npub trait Area {\n    fn area(self) -> Float;\n}\n\nimpl Area for Shape {\n    fn area(self) -> Float {\n        match self {\n            Circle(r) => 3.0 * r * r,\n            Rect { w, h } => w * h,\n            Dot => 0.0\n        }\n    }\n}\n\npub struct Point {\n    pub x: Int\n}\n\nimpl Point {\n    fn new(x: Int) -> Self {\n        Point { x: x }\n    }\n\n    fn twice(self) -> Int {\n        self.x * 2\n    }\n}\n\npub fn unit() -> Shape {\n    Circle(1.0)\n}\n\nstruct Hidden {\n    n: Int\n}\n"),
            ("other.fl", "pub enum Light {\n    Circle,\n    Off\n}\n"),
            ("main.fl", "import io { println }\nimport shapes { Shape, Point, Area, unit }\nimport other\n\nenum Local {\n    Circle,\n    Square\n}\n\nfn describe(s: Shape) -> String {\n    match s {\n        Circle(r) => \"circle {r}\",\n        Shape::Rect { w, h } => \"rect {w}x{h}\",\n        shapes::Shape::Dot => \"dot\"\n    }\n}\n\nfn main() {\n    let a = Circle\n    let b = Shape::Circle(2.0)\n    let c = Rect { w: 2.0, h: 3.0 }\n    let l = other::Light::Circle\n    println(\"{a} {b} {c} {l}\")\n    println(\"{describe(b)} {describe(c)} {describe(Dot)} {describe(unit())}\")\n    println(\"{c.area()} {Point::new(4).twice()} {shapes::Point::new(1)}\")\n    println(\"{b == Shape::Circle(2.0)} {a == Local::Circle}\")\n}\n"),
        ],
        ".",
    );
    assert_eq!(
        out,
        "Local::Circle Shape::Circle(2.0) Shape::Rect { h: 3.0, w: 2.0 } Light::Circle\n\
         circle 2.0 rect 2.0x3.0 dot circle 1.0\n\
         6.0 8 Point { x: 1 }\n\
         true true\n"
    );
    // Messages name types as declared.
    check_fails(
        &[
            ("shapes.fl", "pub enum Shape {\n    Circle(Float),\n    Rect { w: Float, h: Float },\n    Dot\n}\n\npub trait Area {\n    fn area(self) -> Float;\n}\n\nimpl Area for Shape {\n    fn area(self) -> Float {\n        match self {\n            Circle(r) => 3.0 * r * r,\n            Rect { w, h } => w * h,\n            Dot => 0.0\n        }\n    }\n}\n\npub struct Point {\n    pub x: Int\n}\n\nimpl Point {\n    fn new(x: Int) -> Self {\n        Point { x: x }\n    }\n\n    fn twice(self) -> Int {\n        self.x * 2\n    }\n}\n\npub fn unit() -> Shape {\n    Circle(1.0)\n}\n\nstruct Hidden {\n    n: Int\n}\n"),
            ("main.fl", "import shapes { Shape }\nfn main() {\n    let s = Shape::Nope\n}\n"),
        ],
        ".",
        "enum `Shape` has no variant `Nope`",
    );
}

#[test]
fn review_findings_for_module_types_stay_fixed() {
    let geo = "pub struct Point {\n    pub x: Int\n}\npub enum Color {\n    Red,\n    Green\n}\npub trait Area {\n    fn area(self) -> Int;\n}\nstruct Hidden {\n    x: Int\n}\n";
    let other = "pub struct Point {\n    pub y: Int\n}\npub fn n() -> Int {\n    1\n}\n";
    // An imported unit variant in a pattern is the variant, not a binding.
    check_fails(
        &[
            ("geo.fl", geo),
            (
                "main.fl",
                "import geo { Color }\nfn f(c: Color) -> Int {\n    match c {\n        Red => 1\n    }\n}\nfn main() {}\n",
            ),
        ],
        ".",
        "does not cover `Color::Green`",
    );
    // Two modules' `Point` types specialize a generic function apart; a type
    // parameter named like an imported type is the parameter; qualified type
    // names work in annotations and `impl` heads.
    let out = run_ok(
        &[
            ("geo.fl", geo),
            ("other.fl", other),
            (
                "main.fl",
                "import io { println }\nimport geo\nimport other { n }\nstruct Local {\n    w: Int\n}\nimpl geo::Area for Local {\n    fn area(self) -> Int {\n        self.w\n    }\n}\nimpl Area for other::Point {\n    fn area(self) -> Int {\n        self.y\n    }\n}\nfn id[T](x: T) -> T {\n    x\n}\nfn same[Point](x: Point) -> Point {\n    x\n}\nfn pick(c: geo::Color) -> Int {\n    match c {\n        Red => 1,\n        Green => 2\n    }\n}\nfn first(o: Option[geo::Point]) -> Int {\n    match o {\n        Some(p) => p.x,\n        None => 0\n    }\n}\nfn main() {\n    let p = id(geo::Point { x: 1 })\n    let q = id(other::Point { y: 2 })\n    println(\"{p} {q} {same(3)} {pick(Green)} {first(Some(p))} {Local { w: 4 }.area()} {q.area()} {n()}\")\n}\n",
            ),
        ],
        ".",
    );
    assert_eq!(out, "Point { x: 1 } Point { y: 2 } 3 2 1 4 2 1\n");
    // Internal names cannot be written, and messages show declared names.
    check_fails(
        &[
            ("geo.fl", geo),
            (
                "main.fl",
                "import geo { Point }\nfn main() {\n    let h = __mod_1__Hidden { x: 1 }\n}\n",
            ),
        ],
        ".",
        "names starting with `__mod_` are reserved",
    );
    check_fails(
        &[
            ("geo.fl", geo),
            (
                "main.fl",
                "import geo { Area, Point }\nimpl Area for Point {}\nfn main() {}\n",
            ),
        ],
        ".",
        "impl of `Area` for `Point` is missing method `area`",
    );
}

#[test]
fn each_module_sees_only_the_variants_of_enums_it_declares_or_imports() {
    let geo = "pub enum Color {\n    Red,\n    Green\n}\nenum Hidden {\n    Secret\n}\npub fn n() -> Int {\n    1\n}\n";
    // A nested unit variant of an imported enum is the variant.
    check_fails(
        &[
            ("geo.fl", geo),
            (
                "main.fl",
                "import geo { Color }\nfn f(c: Option[Color]) -> Int {\n    match c {\n        Some(Red) => 1,\n        None => 0\n    }\n}\nfn main() {}\n",
            ),
        ],
        ".",
        "does not cover",
    );
    // A private enum's variant is not visible, and its name is free.
    check_fails(
        &[
            ("geo.fl", geo),
            (
                "main.fl",
                "import geo { n }\nfn main() {\n    let h = Secret\n}\n",
            ),
        ],
        ".",
        "undefined name `Secret`",
    );
    let out = run_ok(
        &[
            ("geo.fl", geo),
            (
                "main.fl",
                "import io { println }\nimport geo { n, Color }\nfn f(Secret: Int) -> Int {\n    Secret + n()\n}\nfn g(c: Option[Color]) -> Int {\n    match c {\n        Some(Red) => 1,\n        Some(Green) => 2,\n        None => 0\n    }\n}\nfn main() {\n    println(\"{f(1)} {g(Some(Green))}\")\n}\n",
            ),
        ],
        ".",
    );
    assert_eq!(out, "2 2\n");
}

const BANK: &str = "pub struct Account {
    pub owner: String,
    balance: Int
}

impl Account {
    fn open(owner: String) -> Self {
        Self { owner: owner, balance: 0 }
    }
    fn deposit(mut self, amount: Int) {
        self.balance += amount
    }
    fn balance(self) -> Int {
        self.balance
    }
}
";

/// 0.4: a field without `pub` is private to its module: another module
/// reads `pub` fields and uses the type's functions, and cannot name the
/// private ones (E033) in field reads, writes, literals, `with`, or `let`
/// patterns.
#[test]
fn private_fields_are_visible_only_in_their_module() {
    let out = run_ok(
        &[
            (
                "main.fl",
                "import io { println }\nimport bank { Account }\nfn main() -> Int {\n  let mut a = Account::open(\"ann\")\n  a.deposit(5)\n  let b = a with { owner: \"bo\" }\n  println(\"{a.owner} {a.balance()} {b.owner}\")\n  0\n}\n",
            ),
            ("bank.fl", BANK),
        ],
        ".",
    );
    assert_eq!(out, "ann 5 bo\n");
    for body in [
        "  let n = a.balance\n",
        "  a.balance = 3\n",
        "  let b = Account { owner: \"x\", balance: 1 }\n",
        "  let c = a with { balance: 9 }\n",
        "  let Account { owner, balance } = a\n",
    ] {
        check_fails(
            &[
                (
                    "main.fl",
                    &format!("import bank {{ Account }}\nfn main() {{\n  let mut a = Account::open(\"ann\")\n{body}}}\n"),
                ),
                ("bank.fl", BANK),
            ],
            ".",
            "field `balance` of `Account` is private to its module",
        );
    }
}
