//! Cross-stage contracts: symbol resolution and type checking must agree with VM execution.

use flux_cli::pipeline::{compile, source_from};
use flux_diagnostics::FluxError;
use flux_driver::check_file;
use flux_runtime::RuntimeValue;

fn run_value(source: &str, name: &str) -> RuntimeValue {
    let compiled = compile(source_from("<core-contract>", source))
        .unwrap_or_else(|failure| panic!("compilation failed: {:?}", failure.diagnostics.errors));
    compiled.executor.get_value(name).expect("binding value")
}

#[test]
fn user_function_can_have_builtin_name() {
    let value = run_value("fn abs(x: Int) -> Int { x + 1 }\nlet n = abs(1)\n", "n");
    assert_eq!(value, RuntimeValue::int_i64(2));
}

#[test]
fn user_function_named_print_remains_pure() {
    let value = run_value(
        "fn print(x: Int) -> Int { x + 1 }\nlet mut input = 1\nlet result = print(input)\n",
        "result",
    );
    assert_eq!(value, RuntimeValue::int_i64(2));
}

#[test]
fn generic_specialization_cannot_capture_user_function() {
    let value = run_value(
        "fn id[T](x: T) -> T { x }\nfn id_Int(x: Int) -> Int { x + 1 }\nlet n = id(1)\n",
        "n",
    );
    assert_eq!(value, RuntimeValue::int_i64(1));
}

#[test]
fn cast_to_unrelated_type_is_rejected() {
    let failure = match compile(source_from("<invalid-cast>", "let x = true as String\n")) {
        Ok(_) => panic!("invalid cast compiled"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|error| matches!(error, FluxError::TypeMismatch { .. })));
}

#[test]
fn builtin_rejects_wrong_argument_type_before_execution() {
    let directory = tempfile::tempdir().expect("temporary project");
    let source = directory.path().join("main.fl");
    std::fs::write(&source, "import math { abs }\nlet n = abs(\"hello\")\n").expect("write source");
    let failure = match check_file(&source) {
        Ok(_) => panic!("invalid builtin call passed type checking"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|error| matches!(error, FluxError::TypeMismatch { .. })));
}

#[test]
fn derived_binding_rejects_transitive_io_effect() {
    let directory = tempfile::tempdir().expect("temporary project");
    let source = directory.path().join("main.fl");
    std::fs::write(
        &source,
        "import io { println }\nlet mut input = 1\nfn side() -> Int { println(\"effect\")\n  2 }\nfn indirect() -> Int { side() }\nlet result = input + indirect()\n",
    )
    .expect("write source");
    let failure = match check_file(&source) {
        Ok(_) => panic!("side effect in derived value passed type checking"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|error| matches!(error, FluxError::SideEffectInDerived { .. })));
}

#[test]
fn forward_binding_uses_its_inferred_type() {
    let value = run_value("let a = b + \"x\"\nlet b = \"y\"\n", "a");
    assert_eq!(value, RuntimeValue::String("yx".into()));
}

#[test]
fn empty_string_source_is_readable_in_handlers() {
    // `""` used to parse as an interpolation with no parts, which left the
    // source untyped inside `watch`/`on` bodies ("undefined name symbol #…").
    let src = "event Tick\nlet mut s = \"\"\non Tick {\n  s = s + \"x\"\n}\n";
    let mut compiled = compile(source_from("<empty-string>", src))
        .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
    compiled.executor.emit("Tick", RuntimeValue::Unit);
    compiled.executor.emit("Tick", RuntimeValue::Unit);
    assert_eq!(
        compiled.executor.get_value("s"),
        Some(RuntimeValue::String("xx".into()))
    );
}

#[test]
fn continue_skips_iteration_on_vm_and_jit() {
    // JIT may fall back to the VM here; either way the value must match.
    let src = "import collections\nfn f(n: Int) -> Int {\n  let mut acc = 0\n  for i in range(n) {\n    if i % 2 == 0 { continue }\n    acc = acc + i\n  }\n  acc\n}\nlet mut a = 10\nlet r = f(a)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<continue>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("r"),
            Some(RuntimeValue::int_i64(25)),
            "{mode:?}"
        );
    }
}

#[test]
fn newline_escape_splits_lines() {
    let src = "import str { str_split }\nimport collections { len }\nlet n = len(str_split(\"a\\nb\\nc\", \"\\n\"))\n";
    assert_eq!(run_value(src, "n"), RuntimeValue::int_i64(3));
}

#[test]
fn empty_array_takes_its_element_type_from_context() {
    let src = "import collections { push, len }\nfn names() -> [String] {\n  let mut xs: [String] = []\n  xs = push(xs, \"a\")\n  xs\n}\nfn none() -> [Int] { [] }\nfn count(xs: [String]) -> Int { len(xs) }\nlet a = names()\nlet b = len(none())\nlet c = count([])\nlet d = push([], 1.5)\n";
    assert_eq!(
        run_value(src, "a"),
        RuntimeValue::array(vec![RuntimeValue::String("a".into())])
    );
    assert_eq!(run_value(src, "b"), RuntimeValue::int_i64(0));
    assert_eq!(run_value(src, "c"), RuntimeValue::int_i64(0));
    assert_eq!(
        run_value(src, "d"),
        RuntimeValue::array(vec![RuntimeValue::float_f64(1.5)])
    );
}

#[test]
fn unannotated_empty_array_binding_is_rejected() {
    for src in [
        "let mut xs = []\n",
        "fn f() -> Int {\n  let xs = []\n  0\n}\nlet r = f()\n",
    ] {
        let failure = match compile(source_from("<empty-array>", src)) {
            Ok(_) => panic!("`[]` without an element type compiled: {src}"),
            Err(failure) => failure,
        };
        assert!(
            failure.diagnostics.errors.iter().any(|e| matches!(
                e,
                FluxError::TypeMismatch { expected, .. } if expected.contains("element type annotation")
            )),
            "{:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn closure_params_infer_from_typed_params_and_locals() {
    // Inside a fn, `xs: [String]` / `let ws: [String]` drive callback param
    // types (they used to fall back to `Int`); the hint does not leak out.
    let src = "import collections { filter, map }\nimport str { str_len, str_split }\nlet nums = [1, 2, 3]\nfn non_empty(xs: [String]) -> [String] { filter(xs, |w| str_len(w) > 0) }\nfn shout(s: String) -> [String] {\n  let ws: [String] = str_split(s, \" \")\n  map(ws, |w| w + \"!\")\n}\nfn big(nums: [Float]) -> [Float] { filter(nums, |x| x > 1.0) }\nlet a = non_empty([\"a\", \"\", \"b\"])\nlet b = shout(\"hi there\")\nlet c = big([0.5, 2.5])\nlet d = filter(nums, |x| x > 1)\n";
    let strings = |xs: &[&str]| {
        RuntimeValue::array(
            xs.iter()
                .map(|s| RuntimeValue::String((*s).into()))
                .collect(),
        )
    };
    assert_eq!(run_value(src, "a"), strings(&["a", "b"]));
    assert_eq!(run_value(src, "b"), strings(&["hi!", "there!"]));
    assert_eq!(
        run_value(src, "c"),
        RuntimeValue::array(vec![RuntimeValue::float_f64(2.5)])
    );
    assert_eq!(
        run_value(src, "d"),
        RuntimeValue::array(vec![RuntimeValue::int_i64(2), RuntimeValue::int_i64(3)])
    );
}

#[test]
fn text_conversions_follow_their_contract() {
    let src = "import str { to_string, parse_int, parse_float }\nlet a = to_string(42) + \"/\" + to_string(true) + \"/\" + to_string(\"x\")\nlet b = parse_int(\"-17\")\nlet c = parse_int(\" 3\")\nlet d = parse_int(\"99999999999999999999\")\nlet e = parse_float(\"2.5e1\")\nlet f = parse_float(\"NaN\")\n";
    let ok = |v| RuntimeValue::Result(Ok(Box::new(v)));
    let is_err = |v: RuntimeValue| matches!(v, RuntimeValue::Result(Err(_)));
    assert_eq!(
        run_value(src, "a"),
        RuntimeValue::String("42/true/x".into())
    );
    assert_eq!(run_value(src, "b"), ok(RuntimeValue::int_i64(-17)));
    assert!(is_err(run_value(src, "c")), "no implicit trimming");
    assert!(is_err(run_value(src, "d")), "out of Int64 range");
    assert_eq!(run_value(src, "e"), ok(RuntimeValue::float_f64(25.0)));
    assert!(is_err(run_value(src, "f")), "non-finite rejected");
}

#[test]
fn parse_int_rejects_non_string_argument() {
    let failure = match compile(source_from(
        "<parse>",
        "import str { parse_int }\nlet n = parse_int(5)\n",
    )) {
        Ok(_) => panic!("parse_int(Int) compiled"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|e| matches!(e, FluxError::TypeMismatch { .. })));
}

#[test]
fn assigning_a_local_does_not_make_a_same_named_module_binding_a_source() {
    // `total = …` inside `f` targets the local; the module `let total` stays
    // an ordinary immutable binding (it used to fail with E016).
    let src = "fn f() -> Int {\n  let mut total = 0\n  for total2 in [1, 2] { total = total + total2 }\n  total\n}\nlet total = f()\n";
    assert_eq!(run_value(src, "total"), RuntimeValue::int_i64(3));
}

#[test]
fn assignment_in_else_if_branch_updates_the_source() {
    let src = "event Go\nlet mut c = 0\nlet d = c * 2\non Go {\n  if c > 10 {\n    c = 1\n  } else if c == 0 {\n    c = 5\n  }\n}\n";
    let mut compiled = compile(source_from("<else-if>", src))
        .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
    compiled.executor.emit("Go", RuntimeValue::Unit);
    assert_eq!(
        compiled.executor.get_value("c"),
        Some(RuntimeValue::int_i64(5))
    );
    assert_eq!(
        compiled.executor.get_value("d"),
        Some(RuntimeValue::int_i64(10))
    );
}

#[test]
fn if_statement_runs_the_selected_branch_on_vm_and_jit() {
    let src = "fn f(c: Int) -> Int {\n  let mut r = 0\n  if c > 10 {\n    r = 1\n  } else if c == 0 {\n    r = 5\n  } else {\n    r = 9\n  }\n  r\n}\nlet mut x = 0\nlet a = f(x)\nlet b = f(x + 20)\nlet c = f(x + 3)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<if>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [("a", 5), ("b", 1), ("c", 9)] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(want)),
                "{mode:?} {name}"
            );
        }
    }
}

#[test]
fn trailing_if_else_is_the_function_value() {
    let src = "fn size(c: Int) -> String {\n  if c > 10 {\n    let s = \"big\"\n    s\n  } else if c == 0 {\n    \"zero\"\n  } else {\n    \"small\"\n  }\n}\nfn pick(c: Int) -> Int {\n  if c > 10 { return 7 } else { 3 }\n}\nlet mut x = 0\nlet a = size(x)\nlet b = size(x + 20)\nlet c = size(x + 3)\nlet d = pick(x + 20)\nlet e = pick(x)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<tail-if>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        let get = |name| compiled.executor.get_value(name);
        assert_eq!(
            get("a"),
            Some(RuntimeValue::String("zero".into())),
            "{mode:?}"
        );
        assert_eq!(
            get("b"),
            Some(RuntimeValue::String("big".into())),
            "{mode:?}"
        );
        assert_eq!(
            get("c"),
            Some(RuntimeValue::String("small".into())),
            "{mode:?}"
        );
        assert_eq!(get("d"), Some(RuntimeValue::int_i64(7)), "{mode:?}");
        assert_eq!(get("e"), Some(RuntimeValue::int_i64(3)), "{mode:?}");
    }
}

#[test]
fn trailing_if_needs_else_and_matching_branch_types() {
    for src in [
        "fn f(c: Int) -> Int {\n  if c > 10 { 1 } else { \"no\" }\n}\nlet a = f(1)\n",
        "fn f(c: Int) -> Int {\n  if c > 10 { 1 }\n}\nlet a = f(1)\n",
        "fn f(c: Int) -> Int {\n  if c > 10 { 1 } else { 2.5 }\n}\nlet a = f(1)\n",
    ] {
        let failure = match compile(source_from("<tail-if>", src)) {
            Ok(_) => panic!("compiled: {src}"),
            Err(failure) => failure,
        };
        assert!(
            failure
                .diagnostics
                .errors
                .iter()
                .any(|e| matches!(e, FluxError::TypeMismatch { .. })),
            "{src}: {:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn block_match_arms_run_statements_then_yield_their_value() {
    let src = "import str { parse_int }\nfn g(s: String) -> Int {\n  let mut total = 10\n  let r = match parse_int(s) {\n    Ok(v) => {\n      let bonus = v * 2\n      total = total + bonus\n      total\n    },\n    Err(_) => 0\n  }\n  r + total\n}\nlet mut x = \"5\"\nlet a = g(x)\nlet b = g(\"nope\")\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<arms>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("a"),
            Some(RuntimeValue::int_i64(40)),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("b"),
            Some(RuntimeValue::int_i64(10)),
            "{mode:?}"
        );
    }
}

#[test]
fn block_match_arms_reject_statements_expressions_cannot_run() {
    let arm = |body: &str| {
        format!("event E\nlet mut m = 0\nfn f(n: Int) -> Int {{\n  let k = 1\n  match n {{\n    _ => {{\n      {body}\n      0\n    }}\n  }}\n}}\nlet a = f(1)\n")
    };
    // Control flow is allowed in a function's arms (see
    // `match_arm_control_flow_acts_on_the_enclosing_function`); effects and
    // module or immutable assignments are not.
    for (body, needle) in [
        ("emit E", "`emit`"),
        ("m = 2", "assigning a module binding"),
        ("k = 2", "immutable local"),
    ] {
        let src = arm(body);
        let failure = match compile(source_from("<arms>", &src)) {
            Ok(_) => panic!("compiled: {body}"),
            Err(failure) => failure,
        };
        assert!(
            failure.diagnostics.errors.iter().any(|e| matches!(
                e,
                FluxError::Parse { message, .. } if message.contains(needle)
            )),
            "{body}: {:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn block_match_arm_let_annotation_is_checked() {
    let src = "import str { parse_int }\nlet a = match parse_int(\"1\") {\n  Ok(v) => {\n    let s: String = v\n    1\n  },\n  Err(_) => 0\n}\n";
    let failure = match compile(source_from("<arms>", src)) {
        Ok(_) => panic!("annotation mismatch compiled"),
        Err(failure) => failure,
    };
    assert!(failure
        .diagnostics
        .errors
        .iter()
        .any(|e| matches!(e, FluxError::TypeMismatch { .. })));
}

#[test]
fn option_and_result_constructors_take_their_type_from_context() {
    let src = "import option { Some, None }\nimport result { Ok, Err }\nfn half(n: Int) -> Option[Int] {\n  if n % 2 == 0 { Some(n / 2) } else { None }\n}\nfn check(n: Int) -> Result[Int, String] {\n  if n > 0 { Ok(n) } else { Err(\"negative\") }\n}\nfn first_big(n: Int) -> Option[Int] {\n  match n { k if k < 10 => None, k => Some(k) }\n}\nfn unwrap_or_zero(o: Option[Int]) -> Int { match o { Some(v) => v, None => 0 } }\nlet mut x = 4\nlet a = half(x)\nlet b = half(x + 1)\nlet c = check(x)\nlet d = check(x - 9)\nlet e = first_big(x + 20)\nlet f = unwrap_or_zero(None)\nlet g: Option[String] = None\nlet nothing = None\nlet good = Ok(42)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<ctors>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        let get = |name| compiled.executor.get_value(name).expect(name);
        let some = |v| RuntimeValue::Option(Some(Box::new(v)));
        assert_eq!(get("a"), some(RuntimeValue::int_i64(2)), "{mode:?}");
        assert_eq!(get("b"), RuntimeValue::Option(None), "{mode:?}");
        assert_eq!(
            get("c"),
            RuntimeValue::Result(Ok(Box::new(RuntimeValue::int_i64(4))))
        );
        assert_eq!(
            get("d"),
            RuntimeValue::Result(Err(Box::new(RuntimeValue::String("negative".into()))))
        );
        assert_eq!(get("e"), some(RuntimeValue::int_i64(24)), "{mode:?}");
        assert_eq!(get("f"), RuntimeValue::int_i64(0), "{mode:?}");
        // Bare `None` is the Option value (it used to evaluate to the string "None").
        assert_eq!(get("g"), RuntimeValue::Option(None), "{mode:?}");
        assert_eq!(get("nothing"), RuntimeValue::Option(None), "{mode:?}");
        assert_eq!(
            get("good"),
            RuntimeValue::Result(Ok(Box::new(RuntimeValue::int_i64(42))))
        );
    }
}

#[test]
fn option_and_result_constructors_still_reject_wrong_payloads() {
    for src in [
        "import option { Some }\nfn f() -> Option[Int] { Some(\"s\") }\nlet a = f()\n",
        "import result { Ok, Err }\nfn f(n: Int) -> Result[Int, String] {\n  if n > 0 { Ok(n) } else { Err(5) }\n}\nlet a = f(1)\n",
        "import option { Some, None }\nfn f(n: Int) -> Int {\n  if n > 0 { Some(n) } else { None }\n}\nlet a = f(1)\n",
    ] {
        let failure = match compile(source_from("<ctors>", src)) {
            Ok(_) => panic!("compiled: {src}"),
            Err(failure) => failure,
        };
        assert!(
            failure
                .diagnostics
                .errors
                .iter()
                .any(|e| matches!(e, FluxError::TypeMismatch { .. })),
            "{src}: {:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn get_returns_some_in_bounds_and_none_otherwise() {
    let src = "import collections { get }\nlet xs = [\"a\", \"b\", \"c\"]\nlet mut i = 1\nlet a = get(xs, i)\nlet b = get(xs, i + 5)\nlet c = get(xs, i - 2)\nlet d = get([10, 20], 0 as UInt8)\nlet e = match get(xs, 2) { Some(s) => s, None => \"?\" }\nfn first(xs: [Int]) -> Option[Int] { get(xs, 0) }\nlet f = first([])\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<get>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        let get = |name| compiled.executor.get_value(name).expect(name);
        let some = |v| RuntimeValue::Option(Some(Box::new(v)));
        assert_eq!(get("a"), some(RuntimeValue::String("b".into())), "{mode:?}");
        assert_eq!(get("b"), RuntimeValue::Option(None), "{mode:?}");
        assert_eq!(
            get("c"),
            RuntimeValue::Option(None),
            "negative index, {mode:?}"
        );
        assert_eq!(get("d"), some(RuntimeValue::int_i64(10)), "{mode:?}");
        assert_eq!(get("e"), RuntimeValue::String("c".into()), "{mode:?}");
        assert_eq!(get("f"), RuntimeValue::Option(None), "{mode:?}");
    }
}

#[test]
fn get_requires_an_array_and_an_integer_index() {
    for src in [
        "import collections { get }\nlet a = get([1], \"0\")\n",
        "import collections { get }\nlet a = get(5, 0)\n",
        "import collections { get }\nlet a = get([1], 0.5)\n",
    ] {
        let failure = match compile(source_from("<get>", src)) {
            Ok(_) => panic!("compiled: {src}"),
            Err(failure) => failure,
        };
        assert!(failure
            .diagnostics
            .errors
            .iter()
            .any(|e| matches!(e, FluxError::TypeMismatch { .. })));
    }
}

#[test]
fn keywords_end_at_a_word_boundary() {
    // `3` followed by a line `assert_eq(…)` used to parse as `3 as sert_eq…`.
    let src = "fn assert_eq(a: Int, b: Int) -> Int { a + b }\nfn iffy(a: Int) -> Int { a + 1 }\nfn f() -> Int {\n  let x = 3\n  assert_eq(x, 1)\n}\nfn g() -> Int {\n  let inputs = [1, 2]\n  let mut s = 0\n  for i in inputs { s = s + i }\n  let elsewhere = s\n  iffy(elsewhere)\n}\nlet a = f()\nlet b = g()\nlet c = 3 as Float\n";
    assert_eq!(run_value(src, "a"), RuntimeValue::int_i64(4));
    assert_eq!(run_value(src, "b"), RuntimeValue::int_i64(4));
    assert_eq!(run_value(src, "c"), RuntimeValue::float_f64(3.0));
}

#[test]
fn floats_always_display_a_decimal_point() {
    let src = "import str { to_string }\nlet a = to_string(5.0)\nlet b = to_string(2.5)\nlet c = to_string(10.0 * 0.2)\nlet d = to_string(1.0 / 0.0)\nlet e = to_string(3)\nlet f = to_string(-0.0)\n";
    for (name, want) in [
        ("a", "5.0"),
        ("b", "2.5"),
        ("c", "2.0"),
        ("d", "inf"),
        ("e", "3"),
        ("f", "-0.0"),
    ] {
        assert_eq!(
            run_value(src, name),
            RuntimeValue::String(want.into()),
            "{name}"
        );
    }
}

#[test]
fn literal_match_patterns_compare_with_equality() {
    let src = "fn name(n: Int) -> String {\n  match n { 0 => \"zero\", -1 => \"minus one\", k if k > 100 => \"big\", _ => \"other\" }\n}\nfn yes(s: String) -> Bool { match s { \"y\" => true, \"yes\" => true, _ => false } }\nlet mut x = 0\nlet a = name(x)\nlet b = name(x - 1)\nlet c = name(x + 500)\nlet d = name(x + 7)\nlet e = yes(\"yes\")\nlet f = yes(\"nope\")\nlet trueish = 1\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<lit>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        let get = |name| compiled.executor.get_value(name).expect(name);
        let s = |v: &str| RuntimeValue::String(v.into());
        assert_eq!(get("a"), s("zero"), "{mode:?}");
        assert_eq!(get("b"), s("minus one"), "{mode:?}");
        assert_eq!(get("c"), s("big"), "{mode:?}");
        assert_eq!(get("d"), s("other"), "{mode:?}");
        assert_eq!(get("e"), RuntimeValue::Bool(true), "{mode:?}");
        assert_eq!(get("f"), RuntimeValue::Bool(false), "{mode:?}");
    }
}

#[test]
fn literal_patterns_need_a_catch_all_and_a_matching_type() {
    for (src, needle) in [
        (
            "let n = 1\nlet a = match n { 0 => 1, 1 => 2 }\n",
            "catch-all",
        ),
        (
            "let n = 1\nlet a = match n { \"x\" => 1, _ => 2 }\n",
            "String",
        ),
        (
            "let n = 1.0\nlet a = match n { 1.5 => 1, _ => 2 }\n",
            "literal pattern must be",
        ),
    ] {
        let failure = match compile(source_from("<lit>", src)) {
            Ok(_) => panic!("compiled: {src}"),
            Err(failure) => failure,
        };
        let text = format!("{:?}", failure.diagnostics.errors);
        assert!(text.contains(needle), "{src}: {text}");
    }
}

#[test]
fn module_init_follows_dependencies_through_function_calls() {
    // `a = f()` reads `C` through `f`; `C` is computed from `B`, both declared later.
    let src = "fn f() -> Int { C * 2 }\nfn depth(n: Int) -> Int { if n > 0 { depth(n - 1) + 1 } else { 0 } }\nlet a = f()\nconst C = B + 1\nconst B = 4\nlet d = depth(3)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<init>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("a"),
            Some(RuntimeValue::int_i64(10)),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("d"),
            Some(RuntimeValue::int_i64(3)),
            "{mode:?}"
        );
    }
}

#[test]
fn reading_an_immutable_let_from_a_fn_suggests_const() {
    let failure = match compile(source_from(
        "<e002>",
        "let greeting = \"hi\"\nfn f() -> String { greeting }\nlet r = f()\n",
    )) {
        Ok(_) => panic!("reactive read in a pure fn compiled"),
        Err(failure) => failure,
    };
    assert!(failure.diagnostics.errors.iter().any(|e| matches!(
        e,
        FluxError::ReactiveInPureContext { help, .. } if help.contains("const greeting")
    )));
}

#[test]
fn a_line_starting_with_minus_is_an_error_not_a_subtraction() {
    for src in [
        "import io { println }\nfn g() -> Int {\n  println(\"x\")\n  -1\n}\nlet r = g()\n",
        "let a = 5 // note\n  - 2\n",
    ] {
        let failure = match compile(source_from("<minus>", src)) {
            Ok(_) => panic!("silently compiled as subtraction: {src}"),
            Err(failure) => failure,
        };
        assert!(
            failure.diagnostics.errors.iter().any(|e| matches!(
                e,
                FluxError::Parse { message, .. } if message.contains("starts with `-`")
            )),
            "{src}: {:?}",
            failure.diagnostics.errors
        );
    }
    // Operators at the end of a line, a leading `+`, and unary minus still work.
    let ok = "let a = 5 -\n  2\nlet b = 5\n  + 2\nlet c = 2 * -3 - -1\nlet d = -a\n";
    assert_eq!(run_value(ok, "a"), RuntimeValue::int_i64(3));
    assert_eq!(run_value(ok, "b"), RuntimeValue::int_i64(7));
    assert_eq!(run_value(ok, "c"), RuntimeValue::int_i64(-5));
    assert_eq!(run_value(ok, "d"), RuntimeValue::int_i64(-3));
}

#[test]
fn struct_fields_and_enum_payloads_can_be_user_types() {
    // Named field/payload types used to stay as empty `enum` placeholders.
    let src = "struct In { v: Int }\nstruct Out { inner: In }\nenum Shape { Dot(In), Empty }\nfn deep(o: Out) -> Int { o.inner.v }\nfn weight(s: Shape) -> Int { match s { Dot(p) => p.v * 2, Empty => 0 } }\nlet a = deep(Out { inner: In { v: 3 } })\nlet b = weight(Dot(In { v: 5 }))\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<types>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("a"),
            Some(RuntimeValue::int_i64(3)),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("b"),
            Some(RuntimeValue::int_i64(10)),
            "{mode:?}"
        );
    }
}

#[test]
fn push_keeps_value_semantics_when_appending_in_place() {
    // `x = push(x, v)` appends in place; other holders must not see it.
    let src = "import collections { push, len, range }\n\
fn aliased() -> Int {\n    let mut xs = [1, 2]\n    let ys = xs\n    xs = push(xs, 3)\n    len(ys) * 10 + len(xs)\n}\n\
fn reads_self() -> Int {\n    let mut xs: [Int] = []\n    for i in range(3) { xs = push(xs, len(xs) * 10) }\n    let mut t = 0\n    for x in xs { t = t + x }\n    t\n}\n\
fn other_target() -> Int {\n    let xs = [1]\n    let mut ys = push(xs, 2)\n    ys = push(ys, 3)\n    len(xs) * 10 + len(ys)\n}\n\
fn grow(xs: [Int]) -> Int {\n    let mut ys = xs\n    ys = push(ys, 1)\n    len(ys)\n}\n\
fn caller() -> Int {\n    let xs = [5, 6]\n    let n = grow(xs)\n    len(xs) * 10 + n\n}\n\
let a = aliased()\nlet b = reads_self()\nlet c = other_target()\nlet d = caller()\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<push>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [("a", 23), ("b", 30), ("c", 13), ("d", 23)] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(want)),
                "{name} {mode:?}"
            );
        }
    }
}

#[test]
fn pop_returns_the_array_without_its_last_element() {
    // SPEC: `pop: fn([T]) -> [T]`. It used to return `[rest, last]`.
    let src = "import collections { pop, len }\nlet a = pop([1, 2, 3])\nlet b: [Int] = []\nlet c = pop(b)\nlet n = len(pop(a))\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<pop>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        let ints = |xs: &[i64]| {
            RuntimeValue::array(xs.iter().map(|&x| RuntimeValue::int_i64(x)).collect())
        };
        assert_eq!(
            compiled.executor.get_value("a"),
            Some(ints(&[1, 2])),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("c"),
            Some(ints(&[])),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("n"),
            Some(RuntimeValue::int_i64(1)),
            "{mode:?}"
        );
    }
}

#[test]
fn match_arm_control_flow_acts_on_the_enclosing_function() {
    let src = "import result { Ok, Err }\nimport str { parse_int }\n\
fn add(a: String, b: String) -> Result[Int, String] {\n    let x = match parse_int(a) {\n        Ok(v) => v,\n        Err(e) => return Err(e)\n    }\n    let y = match parse_int(b) {\n        Ok(v) => v,\n        Err(e) => return Err(e)\n    }\n    Ok(x + y)\n}\n\
fn ok_or(r: Result[Int, String], d: Int) -> Int {\n    match r {\n        Ok(v) => v,\n        Err(_) => d\n    }\n}\n\
fn classify(x: Int) -> Int {\n    match x {\n        0 => {\n            let mut t = 1\n            t = t + 1\n            t\n        },\n        _ => {\n            if x < 0 {\n                return -1\n            }\n            x\n        }\n    }\n}\n\
fn first_neg(xs: [Int]) -> Int {\n    let mut found = 0\n    let mut seen = 0\n    for x in xs {\n        let skip = match x {\n            0 => continue,\n            _ => false\n        }\n        seen = seen + 1\n        let stop = match x < 0 {\n            true => {\n                found = x\n                break\n            },\n            _ => false\n        }\n    }\n    found * 10 + seen\n}\n\
fn names() -> Int {\n    let returned = 4\n    let breaker = 1\n    let continued = 2\n    breaker\n    continued\n    returned\n}\n\
let a = ok_or(add(\"1\", \"2\"), 0)\nlet b = ok_or(add(\"1\", \"x\"), -9)\nlet c = classify(0) + classify(-4) * 100 + classify(5) * 1000\nlet d = first_neg([3, 0, 5, -2, -7])\nlet e = names()\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<arm-flow>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [("a", 3), ("b", -9), ("c", 4902), ("d", -17), ("e", 4)] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(want)),
                "{name} {mode:?}"
            );
        }
    }
}

#[test]
fn match_arm_control_flow_is_checked_and_limited_to_functions() {
    let errors = |src: &str| match compile(source_from("<arm-flow>", src)) {
        Ok(_) => panic!("compiled:\n{src}"),
        Err(failure) => failure
            .diagnostics
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
    };
    // A `return` in an arm is checked against the function's return type.
    let errs = errors(
        "fn f(x: Int) -> Int {\n    let y = match x {\n        0 => return \"zero\",\n        _ => x\n    }\n    y\n}\nlet r = f(1)\n",
    );
    assert!(
        errs.iter()
            .any(|m| m == "type mismatch: expected `Int64`, found `String`"),
        "{errs:?}"
    );
    // Reactive contexts keep expression-only arms.
    let errs = errors(
        "let mut a = 1\nderived d = match a {\n    1 => {\n        if a > 0 {\n            a = 2\n        }\n        3\n    },\n    _ => 0\n}\n",
    );
    assert!(
        errs.iter().any(|m| m.contains("outside a function")),
        "{errs:?}"
    );
}

#[test]
fn try_operator_unwraps_or_returns_from_the_function() {
    let src = "import result { Ok, Err }\nimport option { Some, None }\nimport str { parse_int }\nimport collections { get }\n\
fn add(a: String, b: String) -> Result[Int, String] {\n    Ok(parse_int(a)? + parse_int(b)?)\n}\n\
fn second(xs: [Int]) -> Option[Int] {\n    let v = get(xs, 1)?\n    Some(v * 10)\n}\n\
fn sum_all(items: [String]) -> Result[Int, String] {\n    let mut t = 0\n    for s in items {\n        t = t + parse_int(s)?\n    }\n    Ok(t)\n}\n\
fn code(r: Result[Int, String]) -> Int {\n    match r {\n        Ok(v) => v,\n        Err(_) => -1\n    }\n}\n\
fn or_zero(o: Option[Int]) -> Int {\n    match o {\n        Some(v) => v,\n        None => 0\n    }\n}\n\
let a = code(add(\"2\", \"3\"))\nlet b = code(add(\"2\", \"x\"))\nlet c = or_zero(second([1, 2]))\nlet d = or_zero(second([1]))\nlet e = code(sum_all([\"1\", \"2\", \"3\"]))\nlet f = code(sum_all([\"1\", \"oops\", \"3\"]))\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<try>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [
            ("a", 5),
            ("b", -1),
            ("c", 20),
            ("d", 0),
            ("e", 6),
            ("f", -1),
        ] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(want)),
                "{name} {mode:?}"
            );
        }
    }
}

#[test]
fn try_operator_misuse_is_e021() {
    let prelude = "import result { Ok, Err }\nimport option { Some, None }\nimport str { parse_int }\nimport collections { get }\n";
    for (body, needle) in [
        (
            "fn f(a: String) -> Int { parse_int(a)? }",
            "but it returns `Int64`",
        ),
        (
            "fn f(a: String) -> Option[Int] { Some(parse_int(a)?) }",
            "use `?` on an `Option`",
        ),
        (
            "fn f(xs: [Int]) -> Result[Int, Int] { Ok(get(xs, 0)?) }",
            "use `?` on a `Result`",
        ),
        (
            "fn f(a: String) -> Result[Int, Int] { Ok(parse_int(a)?) }",
            "the error types must match",
        ),
        (
            "fn f(a: String) { let x = parse_int(a)? }",
            "only allowed in a function with a declared",
        ),
        (
            "derived d = parse_int(\"1\")?",
            "only allowed in a function with a declared",
        ),
    ] {
        let src = format!("{prelude}{body}\n");
        let failure = match compile(source_from("<try>", &src)) {
            Ok(_) => panic!("compiled: {body}"),
            Err(failure) => failure,
        };
        assert!(
            failure
                .diagnostics
                .errors
                .iter()
                .any(|e| e.code() == "E021" && e.to_string().contains(needle)),
            "{body}: {:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn interpolation_shows_any_printable_expression() {
    let src = "struct P { x: Int, name: String }\nconst n = 3\nconst p = P { x: 4, name: \"pt\" }\nfn label(v: Int) -> String { \"v{v}\" }\n\
let a = \"n={n}\"\nlet b = \"x={p.x} name={p.name}\"\nlet c = \"sum={n + 1}, half={n / 2.0}, ok={n > 2}\"\nlet d = \"call={label(7)} nested={\"<{n}>\"}\"\nlet e = \"braces \\{literal\\} and {  n  }\"\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<interp>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [
            ("a", "n=3"),
            ("b", "x=4 name=pt"),
            ("c", "sum=4, half=1.5, ok=true"),
            ("d", "call=v7 nested=<3>"),
            ("e", "braces {literal} and 3"),
        ] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::String(want.into())),
                "{name} {mode:?}"
            );
        }
    }
    let failure = match compile(source_from(
        "<interp>",
        "struct P { x: Int }\nconst p = P { x: 4 }\nlet a = \"p={p}\"\n",
    )) {
        Ok(_) => panic!("a struct was interpolated"),
        Err(failure) => failure,
    };
    assert!(
        failure.diagnostics.errors.iter().any(|e| e
            .to_string()
            .contains("a number, Bool, Char, or String inside `{…}`")),
        "{:?}",
        failure.diagnostics.errors
    );
}

#[test]
fn if_and_match_are_operands() {
    let src = "import collections { len }\n\
fn f(x: Int) -> Int {\n    let a = 1 + if x > 0 { 1 } else { 2 }\n    let b = 10 + match x { 0 => 1, _ => 2 }\n    let c = (if x > 5 { 100 } else { 0 }) * 2 + len([match x { 1 => 7, _ => 8 }, 3])\n    let d = match x { 1 => 3, _ => 4 } * if x == 1 { 10 } else { 1 }\n    a + b + c + d\n}\n\
fn g(iffy: Int, matches: Int) -> Int { iffy + matches }\n\
let r = f(1)\nlet r2 = f(0)\nlet r3 = g(1, 2)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<operands>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [("r", 46), ("r2", 20), ("r3", 3)] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(want)),
                "{name} {mode:?}"
            );
        }
    }
}

#[test]
fn deep_recursion_is_a_runtime_error_at_the_same_depth_on_vm_and_jit() {
    // The VM overflowed the native stack (process abort) near 2,800 calls,
    // while the JIT went much deeper. Runs on a 2 MB test thread on purpose.
    let program = |n: u64| {
        format!(
            "fn down(n: Int) -> Int {{\n    if n == 0 {{\n        0\n    }} else {{\n        1 + down(n - 1)\n    }}\n}}\nlet r = down({n})\n"
        )
    };
    let limit = flux_value::MAX_CALL_DEPTH;
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        // `down(limit - 1)` makes exactly `limit` nested calls.
        let ok =
            flux_cli::pipeline::compile_with_mode(source_from("<deep>", program(limit - 1)), mode)
                .unwrap_or_else(|failure| panic!("{mode:?}: {:?}", failure.diagnostics.errors));
        assert_eq!(
            ok.executor.get_value("r"),
            Some(RuntimeValue::int_i64(limit as i64 - 1)),
            "{mode:?}"
        );
        let failure = match flux_cli::pipeline::compile_with_mode(
            source_from("<deep>", program(limit)),
            mode,
        ) {
            Ok(_) => panic!("{mode:?}: one call past the limit succeeded"),
            Err(failure) => failure,
        };
        assert!(
            failure.diagnostics.errors.iter().any(|e| matches!(
                e,
                FluxError::Runtime { message, .. } if message.contains("call depth limit exceeded")
            )),
            "{mode:?}: {:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn in_place_push_keeps_the_array_when_an_arm_breaks_or_continues() {
    // `xs = push(xs, v)` moves `xs` out before `v` runs; a `continue`/`break`
    // in `v` skipped the assignment and left `xs` unbound (`len` gave 0).
    let src = "import collections { push, len }\n\
fn f() -> Int {\n    let mut xs: [Int] = []\n    let mut i = 0\n    while i < 5 {\n        i = i + 1\n        xs = push(xs, match i {\n            3 => continue,\n            5 => {\n                break\n            },\n            _ => i\n        })\n    }\n    len(xs)\n}\nlet r = f()\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<take>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("r"),
            Some(RuntimeValue::int_i64(3)),
            "{mode:?}"
        );
    }
}

fn assert_allocation_limit(src: &str) {
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let failure =
            match flux_cli::pipeline::compile_with_mode(source_from("<limits>", src), mode) {
                Ok(_) => panic!("{mode:?}: no allocation limit"),
                Err(failure) => failure,
            };
        assert!(
            failure.diagnostics.errors.iter().any(|e| matches!(
                e,
                FluxError::Runtime { message, .. } if message.contains("allocation limit exceeded")
            )),
            "{mode:?}: {:?}",
            failure.diagnostics.errors
        );
    }
}

#[test]
fn string_building_stays_within_the_allocation_limit() {
    // String `+` had no limit: a 1 GB string took the JIT over 20 s.
    assert_allocation_limit(
        "fn f() -> Int {\n    let mut s = \"x\"\n    let mut i = 0\n    while i < 40 {\n        s = s + s\n        i = i + 1\n    }\n    0\n}\nlet r = f()\n",
    );
}

#[test]
#[ignore = "pushes 10,000,000 elements (minutes in a debug build); run with --ignored"]
fn push_stays_within_the_collection_limit() {
    // `range` had the limit; `push` grew arrays without one.
    assert_allocation_limit(&format!(
        "import collections {{ push, len }}\nfn f() -> Int {{\n    let mut xs: [Int] = []\n    let mut i = 0\n    while i <= {} {{\n        xs = push(xs, i)\n        i = i + 1\n    }}\n    len(xs)\n}}\nlet r = f()\n",
        flux_value::MAX_COLLECTION_LEN
    ));
}

#[test]
fn a_block_arm_ending_in_a_statement_is_unit() {
    // A `match` used for its effects needed a dummy `0` at the end of each
    // block arm ("a block match arm must end with an expression").
    let src = "fn tally(xs: [Int]) -> Int {\n    let mut pos = 0\n    let mut neg = 0\n    for x in xs {\n        match x > 0 {\n            true => {\n                pos = pos + 1\n            },\n            _ => {\n                if x < 0 {\n                    neg = neg + 1\n                }\n            }\n        }\n    }\n    pos * 10 + neg\n}\nlet r = tally([1, -2, 0, 3, -4, -5])\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<unit-arm>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("r"),
            Some(RuntimeValue::int_i64(23)),
            "{mode:?}"
        );
    }
    // Mixing a `()` arm with a valued arm is an ordinary type mismatch.
    let failure = match compile(source_from(
        "<unit-arm>",
        "fn f(x: Int) -> Int {\n    let y = match x {\n        0 => {\n            let z = 1\n        },\n        _ => 5\n    }\n    y\n}\nlet r = f(1)\n",
    )) {
        Ok(_) => panic!("a () arm unified with Int"),
        Err(failure) => failure,
    };
    assert!(
        failure
            .diagnostics
            .errors
            .iter()
            .any(|e| e.to_string() == "type mismatch: expected `Unit`, found `Int64`"),
        "{:?}",
        failure.diagnostics.errors
    );
}

#[test]
fn str_chars_splits_into_characters() {
    let src = "import str { str_chars }\nimport collections { len }\nlet a = str_chars(\"héllo 🙂\")\nlet b = len(str_chars(\"\"))\n";
    let compiled = compile(source_from("<chars>", src))
        .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
    let chars = ["h", "é", "l", "l", "o", " ", "🙂"];
    assert_eq!(
        compiled.executor.get_value("a"),
        Some(RuntimeValue::array(
            chars
                .iter()
                .map(|c| RuntimeValue::String((*c).into()))
                .collect()
        ))
    );
    assert_eq!(
        compiled.executor.get_value("b"),
        Some(RuntimeValue::int_i64(0))
    );
}

#[test]
fn indexing_reads_an_element_or_fails_out_of_bounds() {
    let src = "import collections { len, range }\n\
fn sum(xs: [Int]) -> Int {\n    let mut t = 0\n    let mut i = 0\n    while i < len(xs) {\n        t = t + xs[i]\n        i = i + 1\n    }\n    t\n}\n\
fn grid() -> Int { [[1, 2], [3, 4]][1][0] }\n\
fn last(names: [String]) -> String { names[len(names) - 1] }\n\
let a = sum(range(1000))\nlet b = grid()\nlet c = last([\"a\", \"b\", \"c\"])\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<index>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("a"),
            Some(RuntimeValue::int_i64(499500)),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("b"),
            Some(RuntimeValue::int_i64(3)),
            "{mode:?}"
        );
        assert_eq!(
            compiled.executor.get_value("c"),
            Some(RuntimeValue::String("c".into())),
            "{mode:?}"
        );
        if let Some(stats) = compiled.executor.jit_stats() {
            assert!(
                !stats.fallback_nodes.iter().any(|n| n == "sum"),
                "{stats:?}"
            );
        }
        for bad in ["let r = [1, 2][2]\n", "let r = [1, 2][0 - 1]\n"] {
            let failure =
                match flux_cli::pipeline::compile_with_mode(source_from("<index>", bad), mode) {
                    Ok(_) => panic!("{mode:?}: {bad} did not fail"),
                    Err(failure) => failure,
                };
            assert!(
                failure.diagnostics.errors.iter().any(|e| matches!(
                    e,
                    FluxError::Runtime { message, .. } if message.contains("index out of bounds")
                )),
                "{mode:?} {bad}: {:?}",
                failure.diagnostics.errors
            );
        }
    }
    let failure = match compile(source_from("<index>", "let r = 5[0]\nlet s = [1][true]\n")) {
        Ok(_) => panic!("ill-typed indexing compiled"),
        Err(failure) => failure,
    };
    let messages: Vec<String> = failure
        .diagnostics
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("expected `an array to index`, found `Int64`")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("expected `an integer index`, found `Bool`")),
        "{messages:?}"
    );
}

#[test]
fn nested_patterns_match_through_payloads() {
    let src = "import option { Some, None }\nimport result { Ok, Err }\n\
enum Tok { Num(Int), Plus, LParen }\n\
fn kind(t: Option[Tok]) -> Int {\n    match t {\n        Some(Num(n)) => n,\n        Some(Plus) => -1,\n        Some(LParen) => -2,\n        None => -3\n    }\n}\n\
fn guarded(t: Option[Tok]) -> Int {\n    match t {\n        Some(Num(n)) if n > 10 => 100,\n        Some(Num(n)) => n,\n        _ => 0\n    }\n}\n\
fn lit(r: Result[Int, String]) -> Int {\n    match r {\n        Ok(0) => 1000,\n        Ok(n) => n,\n        Err(_) => -1\n    }\n}\n\
fn deep(o: Option[Option[Tok]]) -> Int {\n    match o {\n        Some(Some(Num(n))) => n * 2,\n        Some(Some(_)) => 1,\n        Some(None) => 2,\n        None => 3\n    }\n}\n\
fn early(t: Option[Tok]) -> Int {\n    let v = match t {\n        Some(Num(n)) => n,\n        _ => return -99\n    }\n    v + 1\n}\n\
let a = kind(Some(Num(7))) * 1000 + kind(Some(LParen)) * 10 + kind(None)\n\
let b = guarded(Some(Num(50))) + guarded(Some(Num(5))) + guarded(None)\n\
let c = lit(Ok(0)) + lit(Ok(5))\n\
let d = deep(Some(Some(Num(4)))) * 100 + deep(Some(None)) * 10 + deep(None)\n\
let e = early(Some(Num(1))) + early(None)\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<nested>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        for (name, want) in [("a", 6977), ("b", 105), ("c", 1005), ("d", 823), ("e", -97)] {
            assert_eq!(
                compiled.executor.get_value(name),
                Some(RuntimeValue::int_i64(want)),
                "{name} {mode:?}"
            );
        }
    }
    // Nested arms count toward exhaustiveness; a missing payload variant does not.
    let failure = match compile(source_from(
        "<nested>",
        "import option { Some, None }\nenum Tok { Num(Int), Plus, LParen }\nfn kind(t: Option[Tok]) -> Int {\n    match t {\n        Some(Num(n)) => n,\n        Some(Plus) => -1,\n        None => -3\n    }\n}\nlet a = kind(None)\n",
    )) {
        Ok(_) => panic!("a non-exhaustive nested match compiled"),
        Err(failure) => failure,
    };
    assert!(
        failure
            .diagnostics
            .errors
            .iter()
            .any(|e| e.to_string().contains("missing Some")),
        "{:?}",
        failure.diagnostics.errors
    );
}

#[test]
fn none_pattern_matches_only_none() {
    // `None` in a pattern was a binding named `None`, so a first `None => 0`
    // arm caught `Some(4)` too.
    let src = "import option { Some, None }\nfn f(o: Option[Int]) -> Int {\n    match o {\n        None => 0,\n        Some(v) => v\n    }\n}\nlet b = f(Some(4))\n";
    for mode in [
        flux_runtime::ExecutionMode::Vm,
        flux_runtime::ExecutionMode::Jit,
    ] {
        let compiled = flux_cli::pipeline::compile_with_mode(source_from("<none>", src), mode)
            .unwrap_or_else(|failure| panic!("{:?}", failure.diagnostics.errors));
        assert_eq!(
            compiled.executor.get_value("b"),
            Some(RuntimeValue::int_i64(4)),
            "{mode:?}"
        );
    }
}
