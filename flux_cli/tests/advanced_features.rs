//! Traits, method dispatch, enums/structs, match exhaustiveness, and generics:
//! what compiles (on VM and JIT) and the diagnostic each misuse gets.

use flux_cli::pipeline::{compile, compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, RuntimeValue};

/// `(code, message)` of every error `src` produces; panics if it compiles.
fn errors(src: &str) -> Vec<(&'static str, String)> {
    match compile(source_from("<advanced>", src)) {
        Ok(_) => panic!("expected a compile error:\n{src}"),
        Err(failure) => failure
            .diagnostics
            .errors
            .iter()
            .map(|e| (e.code(), e.to_string()))
            .collect(),
    }
}

fn assert_values(src: &str, expected: &[(&str, i64)]) {
    for mode in [ExecutionMode::Vm, ExecutionMode::Jit] {
        let compiled = compile_with_mode(source_from("<advanced>", src), mode)
            .unwrap_or_else(|e| panic!("{mode:?}: {:?}", e.diagnostics.errors));
        for (name, want) in expected {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(*want)),
                "{mode:?} `{name}`"
            );
        }
    }
}

const AREA: &str =
    "trait Area {\n    fn area(x: Sq) -> Int;\n}\nstruct Sq { w: Int }\nstruct Ci { r: Int }\n";

#[test]
fn methods_dispatch_on_the_receivers_type() {
    // A variable receiver used to fail to type-check, and codegen picked the
    // first impl with a matching method name, whatever the receiver's type.
    let src = format!(
        "{AREA}enum Shape {{ Dot, Line(Int) }}\n\
impl Area for Sq {{\n    fn area(self: Sq) -> Int {{ self.w * self.w }}\n}}\n\
impl Area for Ci {{\n    fn area(self: Ci) -> Int {{ self.r * self.r * 3 }}\n}}\n\
impl Area for Shape {{\n    fn area(self: Shape) -> Int {{\n        match self {{\n            Dot => 0,\n            Line(n) => n\n        }}\n    }}\n}}\n\
fn total(s: Sq, c: Ci) -> Int {{ s.area() + c.area() }}\n\
fn shape_area(s: Shape) -> Int {{ s.area() }}\n\
let a = total(Sq {{ w: 2 }}, Ci {{ r: 1 }})\nlet b = Ci {{ r: 2 }}.area()\nlet c = shape_area(Line(7))\n\
derived all = a + b + c\n"
    );
    assert_values(&src, &[("a", 7), ("b", 12), ("c", 7)]);
}

#[test]
fn calling_a_method_no_impl_provides_is_e019() {
    let src = format!(
        "{AREA}impl Area for Sq {{\n    fn area(self: Sq) -> Int {{ self.w }}\n}}\nlet r = Ci {{ r: 2 }}.area()\n"
    );
    let errs = errors(&src);
    assert!(
        errs.contains(&("E019", "no method `area` for type `Ci`".into())),
        "{errs:?}"
    );
}

#[test]
fn impl_must_match_its_trait_e020() {
    let cases = [
        (
            "struct Sq { w: Int }\nimpl Area for Sq {\n    fn area(self: Sq) -> Int { self.w }\n}\n",
            "unknown trait `Area`",
        ),
        (
            "impl Area for Sq {\n    fn perimeter(self: Sq) -> Int { self.w }\n}\n",
            "trait `Area` has no method `perimeter`",
        ),
        (
            "impl Area for Sq {\n    fn perimeter(self: Sq) -> Int { self.w }\n}\n",
            "impl of `Area` for `Sq` is missing method `area`",
        ),
        (
            "impl Area for Sq {\n    fn area(self: Sq) -> String { \"big\" }\n}\n",
            "`area` returns `String`, but trait `Area` declares `Int`",
        ),
        (
            "impl Area for Sq {\n    fn area(self: Sq, k: Int) -> Int { k }\n}\n",
            "`area` takes 2 parameter(s), but trait `Area` declares 1",
        ),
    ];
    for (body, message) in cases {
        let src = if body.starts_with("struct") {
            body.to_string()
        } else {
            format!("{AREA}{body}")
        };
        let errs = errors(&src);
        assert!(
            errs.contains(&("E020", message.to_string())),
            "{message}: {errs:?}"
        );
    }
    // A trait parameter type mismatch beyond the receiver.
    let src = "trait Scale {\n    fn scale(x: Sq, k: Int) -> Int;\n}\nstruct Sq { w: Int }\nimpl Scale for Sq {\n    fn scale(self: Sq, k: Float) -> Int { self.w }\n}\n";
    let errs = errors(src);
    assert!(
        errs.contains(&(
            "E020",
            "parameter `k` of `scale` is `Float`, but trait `Scale` declares `Int`".into()
        )),
        "{errs:?}"
    );
}

#[test]
fn match_must_cover_every_variant_reported_once() {
    let src = "enum Color { Red, Green, Blue }\nfn name(c: Color) -> Int {\n    match c {\n        Red => 1,\n        Green => 2\n    }\n}\nlet r = name(Blue)\n";
    let errs = errors(src);
    let missing: Vec<_> = errs
        .iter()
        .filter(|(_, m)| m.contains("missing Blue"))
        .collect();
    assert_eq!(missing.len(), 1, "{errs:?}");
}

#[test]
fn unknown_variant_arm_is_one_error_without_a_cascade() {
    // The arm's binding used to be reported as `undefined name `symbol #SymbolId(…)``.
    let src = "enum Shape { Circle(Int), Square(Int) }\nfn area(s: Shape) -> Int {\n    match s {\n        Circle(r) => r * r,\n        Square(w) => w * w,\n        Triangle(x) => x\n    }\n}\nlet r = area(Circle(2))\n";
    let errs = errors(src);
    assert_eq!(
        errs,
        vec![(
            "E005",
            "type mismatch: expected `one of Circle, Square`, found `Triangle`".into()
        )],
    );
}

#[test]
fn enums_structs_and_generics_check_their_shapes() {
    let errs = errors(
        "struct P { x: Int, y: Int }\nlet a = P { x: 1 }\nlet b = P { x: 1, y: 2, z: 3 }\nlet c = P { x: \"s\", y: 2 }\nlet d = P { x: 1, y: 2 }.w\n",
    );
    for want in [
        "type mismatch: expected `field `y``, found `missing field`",
        "type mismatch: expected `field from x, y`, found `z`",
        "type mismatch: expected `Int64`, found `String`",
        "type mismatch: expected `field `w``, found `P { x: Int64, y: Int64 }`",
    ] {
        assert!(errs.iter().any(|(_, m)| m == want), "{want}: {errs:?}");
    }
    let errs = errors(
        "enum Shape { Circle(Int), Dot }\nlet a = Circle(\"big\")\nlet b = Dot(3)\nlet c = Circle\n",
    );
    for want in [
        "type mismatch: expected `Int64`, found `String`",
        "type mismatch: expected `payload-free variant Dot`, found `payload provided`",
        "type mismatch: expected `payload for Circle`, found `no payload`",
    ] {
        assert!(errs.iter().any(|(_, m)| m == want), "{want}: {errs:?}");
    }
    let errs = errors("fn id[T](x: T) -> T { x }\nlet c: Int = id(\"s\")\n");
    assert!(
        errs.iter()
            .any(|(_, m)| m == "type mismatch: expected `Int64`, found `String`"),
        "{errs:?}"
    );
    assert_values(
        "import collections { len }\nfn id[T](x: T) -> T { x }\nlet a = id(4)\nlet b = len([id(\"s\"), id(\"t\")])\nderived all = a + b\n",
        &[("a", 4), ("b", 2)],
    );
}
