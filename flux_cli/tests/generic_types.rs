//! Generic structs and enums: type arguments come from field values and from
//! context (an annotation, a parameter, a return type), never written at
//! construction. Every program runs on both engines, which must agree.

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

const TYPES: &str = "import io { println }
struct Pair[T] { a: T, b: T }
enum Tree[T] { Leaf, Node { v: T, l: Tree[T], r: Tree[T] } }
enum Opt[T] { Nothing, Just(T) }
";

#[test]
fn type_arguments_come_from_values_and_context() {
    prints(
        &format!(
            "{TYPES}
fn sum(t: Tree[Int]) -> Int {{
    match t {{
        Leaf => 0,
        Node {{ v, l, r }} => v + sum(l) + sum(r),
    }}
}}

fn nothing() -> Opt[String] {{
    Nothing
}}

fn pick(c: Bool) -> Opt[Int] {{
    if c {{ Just(1) }} else {{ Nothing }}
}}

fn main() {{
    let p = Pair {{ a: 1, b: 2 }}
    let q: Pair[String] = Pair {{ a: \"x\", b: \"y\" }}
    let t: Tree[Int] = Node {{ v: 1, l: Leaf, r: Node {{ v: 2, l: Leaf, r: Leaf }} }}
    let p2 = p with {{ b: 5 }}
    println(\"{{p}} {{q}} {{p2}} {{sum(t)}}\")
    let xs = [Nothing, Just(2)]
    let o: Option[Pair[Int]] = Some(p)
    println(\"{{xs}} {{nothing()}} {{pick(true)}} {{pick(false)}} {{o}}\")
    println(\"{{p == Pair {{ a: 1, b: 2 }}}} {{pick(false) == Nothing}} {{q.a}}\")
    let n = match pick(true) {{
        Just(v) => v + 1,
        Nothing => 0,
    }}
    let Pair {{ a, b }} = p
    println(\"{{n}} {{a + b}}\")
}}
"
        ),
        "Pair { a: 1, b: 2 } Pair { a: \"x\", b: \"y\" } Pair { a: 1, b: 5 } 3\n\
         [Opt::Nothing, Opt::Just(2)] Opt::Nothing Opt::Just(1) Opt::Nothing Some(Pair { a: 1, b: 2 })\n\
         true true x\n\
         2 3\n",
    );
}

#[test]
fn generic_functions_take_and_return_generic_types() {
    prints(
        &format!(
            "{TYPES}import collections {{ map }}

fn first[T](p: Pair[T]) -> T {{
    p.a
}}

fn twice[T](x: T) -> Pair[T] {{
    Pair {{ a: x, b: x }}
}}

fn size[T](t: Tree[T]) -> Int {{
    match t {{
        Leaf => 0,
        Node {{ v, l, r }} => 1 + size(l) + size(r),
    }}
}}

let mut current: Opt[Int] = Nothing

event Set(n: Int)
on Set(n) {{
    current = Just(n)
}}

act fn main() {{
    let s = \"s\"
    let leaf: Tree[String] = Leaf
    println(\"{{first(twice(3))}} {{twice(s)}} {{map([1, 2], twice)}}\")
    println(\"{{size(Node {{ v: s, l: leaf, r: Node {{ v: s, l: Leaf, r: Leaf }} }})}}\")
    emit Set(4)
    println(\"{{current}}\")
}}
"
        ),
        "3 Pair { a: \"s\", b: \"s\" } [Pair { a: 1, b: 1 }, Pair { a: 2, b: 2 }]\n2\nOpt::Just(4)\n",
    );
}

#[test]
fn several_parameters_and_nested_generic_types() {
    prints(
        "import io { println }
struct Entry[K, V] { key: K, values: [V] }
enum Either[A, B] { Left(A), Right(B) }
struct Wrap[T] { inner: Option[T] }

fn describe(e: Either[Int, String]) -> String {
    match e {
        Left(n) => \"left {n}\",
        Right(s) => \"right {s}\",
    }
}

fn main() {
    let e = Entry { key: \"k\", values: [1.5, 2.5] }
    let w: Wrap[Int] = Wrap { inner: None }
    let nested: Wrap[Wrap[Int]] = Wrap { inner: Some(w) }
    let r = \"r\"
    println(\"{e} {e.values[1]} {w} {nested}\")
    println(\"{describe(Left(1))} {describe(Right(r))}\")
}
",
        "Entry { key: \"k\", values: [1.5, 2.5] } 2.5 Wrap { inner: None } Wrap { inner: Some(Wrap { inner: None }) }\nleft 1 right r\n",
    );
}

#[test]
fn generic_types_are_shared_across_modules() {
    let (status, stdout, stderr) = both_files(&[
        (
            "main.fl",
            "import io { println }
import geo
import geo { Tree }
struct Pair[T] { x: T }

fn main() {
    let p: geo::Pair[Int] = geo::Pair { a: 1, b: 2 }
    let mine = Pair { x: \"own\" }
    let t: Tree[String] = Leaf
    match geo::single(3) {
        Node(n) => println(\"{geo::swap(p)} {mine} {t} {n}\"),
        Leaf => println(\"leaf\"),
    }
}
",
        ),
        (
            "geo.fl",
            "pub struct Pair[T] { pub a: T, pub b: T }
pub enum Tree[T] { Leaf, Node(T) }

pub fn swap[T](p: Pair[T]) -> Pair[T] {
    Pair { a: p.b, b: p.a }
}

pub fn single(n: Int) -> Tree[Int] {
    Node(n)
}
",
        ),
    ]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "Pair { a: 2, b: 1 } Pair { x: \"own\" } Tree::Leaf 3\n"
    );
}

#[test]
fn type_arguments_must_agree_and_be_known() {
    let main = |body: &str| format!("{TYPES}fn main() {{\n{body}}}\n");
    // The values fix `T`; a later one must agree.
    rejects(
        &main("    let p = Pair { a: 1, b: \"s\" }\n"),
        &["expected `Int64`, found `String`"],
    );
    rejects(
        &main("    let t: Opt[String] = Just(1)\n"),
        &["expected `Opt[String]`, found `Opt[Int64]`"],
    );
    rejects(
        &main("    let c = true\n    let x = if c { Just(1) } else { Just(\"s\") }\n"),
        &["expected `Opt[Int64]`, found `Opt[String]`"],
    );
    rejects(
        &main("    let p = Pair { a: 1, b: 2 }\n    let q = p with { a: \"s\" }\n"),
        &["expected `Int64`, found `String`"],
    );
    // Nothing fixes `T`: an annotation must.
    rejects(
        &main("    let o = Nothing\n"),
        &[
            "expected `known type arguments for Opt`, found `Opt[_]`",
            "such as `: Opt[Int]`",
        ],
    );
    rejects(
        &main("    let xs = [Leaf]\n"),
        &["found `Tree[_]`", "such as `: [Tree[Int]]`"],
    );
    // A generic type is written with all its arguments.
    rejects(
        &main("    let p: Pair = Pair { a: 1, b: 2 }\n"),
        &["expected `Pair[T]`, found `Pair without type arguments`"],
    );
    rejects(
        &main("    let p: Pair[Int, Int] = Pair { a: 1, b: 2 }\n"),
        &["expected `Pair[T]`, found `Pair with 2 type arguments`"],
    );
    rejects(
        "struct Bad[T, T] { a: T }\nfn main() {\n}\n",
        &["duplicate definition of `T`"],
    );
}

/// Later elements of an array literal fix what the first left unknown; this
/// was an error for `[None, Some(1)]` before 0.3.
#[test]
fn an_array_takes_unknown_parts_from_later_elements() {
    prints(
        "import io { println }\nfn main() {\n    let ys = [None, Some(1)]\n    let y = ys[1].unwrap_or(0) + 1\n    println(\"{ys} {y}\")\n}\n",
        "[None, Some(1)] 2\n",
    );
}

/// A generic call's result is known to an enclosing generic call; this was
/// an error ("could not be unified") before 0.3.
#[test]
fn generic_calls_nest() {
    prints(
        "import io { println }\nfn id[T](x: T) -> T {\n    x\n}\nfn one[T](x: T) -> [T] {\n    [x]\n}\nfn head[T](xs: [T]) -> T {\n    xs[0]\n}\nfn main() {\n    let a = \"a\"\n    println(\"{id(id(3))} {head(one(a))} {head(one(id(2.5)))}\")\n}\n",
        "3 a 2.5\n",
    );
}

/// Review findings: an unknown type argument may not hide in `with`, a
/// binding in a match arm, or a closure's type, and generic calls agree with
/// the checker on joins and refinements.
#[test]
fn unknown_type_arguments_do_not_escape() {
    let o = "import io { println }\nenum O[T] { N, S(T) }\nstruct B[T] { x: T }\n";
    rejects(
        &format!(
            "{o}fn main() {{\n    let b: B[O[String]] = B {{ x: N }} with {{ x: S(1) }}\n}}\n"
        ),
        &["expected `B[O[String]]`, found `B[O[Int64]]`"],
    );
    prints(
        &format!("{o}fn main() {{\n    let c = B {{ x: N }} with {{ x: S(1) }}\n    let d: B[O[Int]] = c\n    println(\"{{d}}\")\n}}\n"),
        "B { x: O::S(1) }\n",
    );
    rejects(
        &format!("{o}fn main() {{\n    match true {{\n        true => {{\n            let mut x = N\n            x = S(1)\n            println(\"{{x}}\")\n        }},\n        false => println(\"no\")\n    }}\n}}\n"),
        &["found `O[_]`"],
    );
    rejects(
        &format!("{o}fn main() {{\n    let mut f = || N\n    f = || S(1)\n}}\n"),
        &["found `O[_]`", "such as `: fn() -> O[Int]`"],
    );
    prints(
        &format!(
            "{o}fn id[T](x: T) -> T {{\n    x\n}}\nfn second[T](a: T, b: T) -> T {{\n    b\n}}\nfn zapply[T, U](x: T, f: fn(T) -> U) -> U {{\n    f(x)\n}}\nfn main() {{\n    let b = true\n    let x = id([N, S(1)])\n    let y = id(if b {{ N }} else {{ S(2) }})\n    let z = second(N, S(3))\n    let n = id(zapply(1, |v| v + 1))\n    println(\"{{x}} {{y}} {{z}} {{n}}\")\n}}\n"
        ),
        "[O::N, O::S(1)] O::N O::S(3) 2\n",
    );
}

/// A generic function that wraps its argument in a generic type each time
/// stops at the nesting limit, quickly.
#[test]
fn wrapping_recursion_hits_the_specialization_limit() {
    let start = std::time::Instant::now();
    rejects(
        "struct Box[T] { x: T }\nfn f[T](x: T) -> Int {\n    f(Box { x: x })\n}\nfn main() {\n    let n = f(1)\n}\n",
        &["finite generic specialization"],
    );
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
}

/// Second review round: a generic instance's user-typed field keeps its full
/// type for patterns, equality, and generic calls, and unknown parts cannot
/// hide in `with`, closures, or function-typed arguments.
#[test]
fn fields_of_generic_instances_keep_their_types() {
    let head = "import io { println }\nstruct P { n: Int }\nstruct F { f: fn() -> Int }\nstruct B[T] { x: T }\nenum E[T] { V(T) }\nenum O[T] { N, S(T) }\nstruct Pair[T] { a: T, b: T }\n";
    rejects(
        &format!("{head}fn main() {{\n    let b = B {{ x: P {{ n: 1 }} }}\n    match b.x {{\n        p => {{\n            let s: String = p.n\n        }}\n    }}\n}}\n"),
        &["expected `String`, found `Int64`"],
    );
    prints(
        &format!("{head}fn id[T](x: T) -> T {{\n    x\n}}\nfn take[T](p: P, x: T) -> T {{\n    x\n}}\nfn main() {{\n    let b = B {{ x: P {{ n: 1 }} }}\n    let xs = id([b.x, P {{ n: 2 }}])\n    let k = take(b.x, 2)\n    match V(P {{ n: 1 }}) {{\n        V {{ n }} => println(\"{{n + k}} {{xs}}\")\n    }}\n}}\n"),
        "3 [P { n: 1 }, P { n: 2 }]\n",
    );
    rejects(
        &format!("{head}fn main() {{\n    let b = B {{ x: F {{ f: || 1 }} }}\n    println(\"{{b == b}}\")\n}}\n"),
        &["compatible equality types"],
    );
    rejects(
        &format!("{head}fn main() {{\n    let p = Pair {{ a: N, b: N }} with {{ a: S(1), b: S(\"s\") }}\n}}\n"),
        &["expected `O[Int64]`, found `O[String]`"],
    );
    rejects(
        &format!("{head}fn main() {{\n    let mut b = B {{ x: || N }}\n    b.x = || S(1)\n}}\n"),
        &["found `O[_]`"],
    );
    rejects(
        &format!("{head}fn main() {{\n    let fs: [fn() -> O[String]] = [|| N, || S(1)]\n}}\n"),
        &["expected `[fn() -> O[String]]`, found `[fn() -> O[Int64]]`"],
    );
}
