//! Error messages say what is wrong, where, and how to fix it (0.5): close
//! names, the import that provides a library name, the methods a type has,
//! a missing match case by name, argument counts in words, hints for
//! `Option`/`Result`, parse errors in words, warnings drawn as warnings,
//! no repeats of an error already reported, and `flux explain`.

use std::path::Path;
use std::process::Command;

fn flux(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn flux");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned()
            + &String::from_utf8_lossy(&output.stdout),
    )
}

/// `flux check` on `src`: its exit status and output.
fn check(src: &str) -> (Option<i32>, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    flux(dir.path(), &["check", "main.fl"])
}

fn says(src: &str, needles: &[&str]) {
    let (status, out) = check(src);
    assert_eq!(status, Some(1), "{src}\n{out}");
    for needle in needles {
        assert!(out.contains(needle), "{src}\nmissing {needle:?} in\n{out}");
    }
}

#[test]
fn an_undefined_name_gets_a_close_name_or_its_import() {
    says(
        "fn main() {\n    let total = 5\n    let y = totl + 1\n}\n",
        &[
            "undefined name `totl`",
            "a name with a similar spelling exists: `total`",
        ],
    );
    says(
        "fn main() {\n    let m = map_new()\n}\n",
        &["`map_new` is in the `collections` library: add `import collections { map_new }`"],
    );
}

#[test]
fn later_errors_in_a_block_are_reported_too() {
    says(
        "fn main() {\n    let m = map_new()\n    let total = 5\n    let y = totl + 1\n    let z = m\n}\n",
        &["undefined name `map_new`", "undefined name `totl`"],
    );
    let (_, out) = check("fn main() {\n    let m = map_new()\n    let z = m\n}\n");
    assert!(!out.contains("undefined name `m`"), "{out}");
}

#[test]
fn an_unknown_method_names_a_close_one_or_the_types_methods() {
    says(
        "fn main() {\n    let xs = [1, 2]\n    let n = xs.lenght()\n}\n",
        &[
            "no method `lenght` for type `[Int]`",
            "there is a method with a similar name: `len`",
        ],
    );
    says(
        "struct P {\n    n: Int,\n}\nimpl P {\n    fn double(self) -> Int {\n        self.n * 2\n    }\n}\nfn main() {\n    let p = P { n: 1 }\n    let d = p.dubble()\n    let t = p.triple()\n}\n",
        &["similar name: `double`", "to give `P` this method, write `impl P { fn triple(self) … }`"],
    );
    let (_, out) = check("fn main() {\n    let s = \"a\".shout()\n}\n");
    assert!(out.contains("`String` has: chars, contains"), "{out}");
    assert!(!out.contains("impl String"), "{out}");
}

#[test]
fn a_match_names_the_cases_it_misses() {
    says(
        "enum Shape {\n    Circle,\n    Square,\n    Triangle,\n}\nfn sides(s: Shape) -> Int {\n    match s {\n        Shape::Circle => 0,\n    }\n}\nfn main() {\n}\n",
        &[
            "E036",
            "this `match` does not cover `Shape::Square` or `Shape::Triangle`",
            "add an arm for `Shape::Square` or `Shape::Triangle`, or `_ => …`",
        ],
    );
    says(
        "fn f(n: Int) -> Int {\n    match n {\n        0 => 1,\n    }\n}\nfn main() {\n}\n",
        &["does not cover every `Int` value", "catch-all arm `_ => …`"],
    );
}

#[test]
fn argument_counts_and_types_read_as_written() {
    says(
        "fn area(w: Int, h: Int) -> Int {\n    w * h\n}\nfn main() {\n    let a = area(3)\n}\n",
        &[
            "`area` takes 2 arguments but 1 was given",
            "`area` is called as `area(Int, Int)`",
        ],
    );
    says(
        "fn main() {\n    let xs = [3, 5, 8]\n    let e = xs.find(|x| x % 2 == 0)\n    let n = e + 1\n}\n",
        &["found `Option[Int] and Int`", "take the value with `.unwrap_or(default)`"],
    );
    says(
        "fn main() {\n    let n = 3\n    let s = \"n = \" + n\n}\n",
        &["to join text and a value, interpolate"],
    );
}

#[test]
fn an_error_type_is_not_reported_again() {
    let (_, out) = check(
        "fn main() {\n    let raw = \"x12\"\n    let n = raw.slic(1, 3)\n    let m = match n.parse_int() {\n        Ok(v) => v,\n        Err(e) => 0,\n    }\n    let k = m + 1\n}\n",
    );
    assert!(out.contains("no method `slic`"), "{out}");
    assert!(!out.contains("<error>"), "{out}");
}

#[test]
fn parse_errors_say_what_to_write() {
    says(
        "fn f(k: Int) -> Int {\n    match k {\n        0 => 1\n        _ => 2\n    }\n}\nfn main() {\n}\n",
        &["expected `,` before this match arm"],
    );
    says(
        "fn main() {\n    let m = {}\n}\n",
        &["`{}` is not a value: an empty map is `Map::new()`"],
    );
    says(
        "fn f(a: Int b: Int) {\n}\nfn main() {\n}\n",
        &["expected `,` after this type"],
    );
    says(
        "fn main() {\n    let x = (1, 2\n}\n",
        &["the `(` on line 2 is not closed"],
    );
    says(
        "live fn Item() {\n    pub event Ping\n}\nlet items = [Item()]\nact fn main() {\n    emit items[0].Ping()\n}\n",
        &["`emit` names an instance directly: bind it first"],
    );
    let (_, out) = check("fn main() {\n    let x = 1 +\n}\n");
    assert!(out.contains("expected an expression, found `}`"), "{out}");
    assert!(
        !out.contains("unary_expr") && !out.contains("eq_op"),
        "{out}"
    );
}

#[test]
fn warnings_look_like_warnings() {
    let (status, out) = check("fn main() {\n    let unused = 5\n}\n");
    assert_eq!(status, Some(0), "{out}");
    assert!(out.contains("! unused local `unused`"), "{out}");
    assert!(!out.contains("x unused local"), "{out}");
}

#[test]
fn helps_fit_the_case() {
    says(
        "live fn C() {\n    pub let mut n = 1\n}\nlet c = C()\nfn show() -> Int {\n    c.n\n}\nfn main() {\n}\n",
        &["`c` holds a component instance", "`act fn main` for `main`"],
    );
    says(
        "let mut x = 1\nconst C = x + 1\nfn main() {\n}\n",
        &["`const C` reads a value that can change"],
    );
    says(
        "extern fn ask() -> Int\nderived d = ask()\nfn main() {\n}\n",
        &["a derived value cannot call the host function `ask`"],
    );
    says(
        "fn main() {\n    let mut total = 0\n    let f = |n: Int| match n {\n        _ => {\n            total = total + n\n            n\n        }\n    }\n    let y = f(1)\n}\n",
        &["a closure cannot change `total`: it holds a copy of it", "return the new value"],
    );
    says(
        "import io { printn }\nfn main() {\n}\n",
        &["`io` has a name with a similar spelling: `println`"],
    );
    says(
        "struct Point {\n    x: Int,\n}\nfn f(p: Piont) -> Int {\n    1\n}\nfn main() {\n}\n",
        &["a type with a similar name exists: `Point`"],
    );
}

#[test]
fn flux_explain_prints_a_codes_chapter() {
    let dir = tempfile::tempdir().unwrap();
    let (status, out) = flux(dir.path(), &["explain", "E001"]);
    assert_eq!(status, Some(0), "{out}");
    assert!(out.starts_with("E001\n"), "{out}");
    let (status, out) = flux(dir.path(), &["explain", "E999"]);
    assert_eq!(status, Some(1), "{out}");
    let (_, out) = check("fn main() {\n    let y = nope\n}\n");
    assert!(out.contains("run `flux explain E001`"), "{out}");
}
