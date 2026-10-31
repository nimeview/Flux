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
        RuntimeValue::Array(vec![RuntimeValue::String("a".into())])
    );
    assert_eq!(run_value(src, "b"), RuntimeValue::int_i64(0));
    assert_eq!(run_value(src, "c"), RuntimeValue::int_i64(0));
    assert_eq!(
        run_value(src, "d"),
        RuntimeValue::Array(vec![RuntimeValue::float_f64(1.5)])
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
        RuntimeValue::Array(
            xs.iter()
                .map(|s| RuntimeValue::String((*s).into()))
                .collect(),
        )
    };
    assert_eq!(run_value(src, "a"), strings(&["a", "b"]));
    assert_eq!(run_value(src, "b"), strings(&["hi!", "there!"]));
    assert_eq!(
        run_value(src, "c"),
        RuntimeValue::Array(vec![RuntimeValue::float_f64(2.5)])
    );
    assert_eq!(
        run_value(src, "d"),
        RuntimeValue::Array(vec![RuntimeValue::int_i64(2), RuntimeValue::int_i64(3)])
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
    for (body, needle) in [
        ("for i in [1] { }", "`for`"),
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
