//! Differential VM/JIT check on generated programs: integer arithmetic (with
//! division by zero and overflow), loops that reassign locals, `if`/`match`
//! operands, indexing, user enums with payloads, calls, and recursion. The VM is the reference; the JIT must give
//! the same value or the same runtime error. Deterministic and bounded.

use std::sync::{Mutex, MutexGuard, OnceLock};

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_diagnostics::FluxError;
use flux_runtime::ExecutionMode;

fn jit_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// Small deterministic generator (64-bit LCG); no external dependency.
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn literal(&mut self) -> String {
        match self.pick(6) {
            0 => "0".into(),
            1 => "1".into(),
            2 => "(0 - 3)".into(),
            3 => "9223372036854775807".into(),
            _ => self.pick(20).to_string(),
        }
    }

    /// An `Int` expression over `vars`; `call` names a function `Int -> Int`.
    fn expr(&mut self, depth: u32, vars: &[&str], call: Option<&str>) -> String {
        if depth == 0 {
            return match self.pick(2) {
                0 => self.literal(),
                _ => vars[self.pick(vars.len() as u64) as usize].to_string(),
            };
        }
        let d = depth - 1;
        match self.pick(10) {
            0..=3 => {
                let op = ["+", "-", "*", "/", "%"][self.pick(5) as usize];
                format!(
                    "({} {op} {})",
                    self.expr(d, vars, call),
                    self.expr(d, vars, call)
                )
            }
            4 => format!(
                "(if {} {} {} {{ {} }} else {{ {} }})",
                self.expr(d, vars, call),
                ["<", "==", ">=", "!="][self.pick(4) as usize],
                self.expr(d, vars, call),
                self.expr(d, vars, call),
                self.expr(d, vars, call)
            ),
            5 => format!(
                "(match {} {{ 0 => {}, 1 => {}, _ => {} }})",
                self.expr(d, vars, call),
                self.expr(d, vars, call),
                self.expr(d, vars, call),
                self.expr(d, vars, call)
            ),
            6 if call.is_some() => {
                format!("{}({})", call.unwrap_or("g"), self.expr(d, vars, call))
            }
            // Indexing, often out of bounds (negative or past the end).
            8 => format!(
                "([{}, {}, {}][{}])",
                self.expr(d, vars, call),
                self.expr(d, vars, call),
                self.expr(d, vars, call),
                self.expr(d, vars, call)
            ),
            // A user enum with payloads (built and matched in compiled code).
            7 if call.is_some() => format!(
                "(match mk({}) {{ A(v) => v, B => {}, C(v) => v - {} }})",
                self.expr(d, vars, call),
                self.expr(d, vars, call),
                self.expr(d, vars, call)
            ),
            _ => self.expr(0, vars, call),
        }
    }

    fn program(&mut self) -> String {
        let g = self.expr(2, &["p"], None);
        let init = self.expr(2, &["a", "b"], Some("g"));
        let cond = self.expr(2, &["a", "x", "i"], Some("g"));
        let then_e = self.expr(2, &["x", "i", "b"], Some("g"));
        let else_e = self.expr(2, &["x", "i", "a"], Some("g"));
        let steps = self.pick(6);
        let (a, b) = (self.literal(), self.literal());
        format!(
            "enum E {{ A(Int), B, C(Int) }}\n\
fn mk(p: Int) -> E {{\n    if p % 3 == 0 {{\n        A(p)\n    }} else {{\n        if p % 3 == 1 {{\n            B\n        }} else {{\n            C(p)\n        }}\n    }}\n}}\n\
fn g(p: Int) -> Int {{ {g} }}\n\
fn h(n: Int) -> Int {{\n    if n <= 0 {{\n        0\n    }} else {{\n        (n % 7) + h(n - 1)\n    }}\n}}\n\
fn f(a: Int, b: Int) -> Int {{\n    let mut x = {init}\n    let mut i = 0\n    while i < {steps} {{\n        if {cond} > 3 {{\n            x = {then_e}\n        }} else {{\n            x = {else_e}\n        }}\n        i = i + 1\n    }}\n    x + h(i)\n}}\n\
let r = f({a}, {b})\n"
        )
    }
}

/// `r`'s value, or the runtime error text; and whether the JIT compiled
/// `f` itself (rather than falling back to the VM).
fn run(src: &str, mode: ExecutionMode) -> (Result<String, String>, bool) {
    match compile_with_mode(source_from("<diff>", src), mode) {
        Ok(compiled) => {
            let native = compiled
                .executor
                .jit_stats()
                .is_some_and(|s| !s.fallback_nodes.iter().any(|n| n == "f"));
            let value = compiled
                .executor
                .get_value("r")
                .map(|v| v.display_string())
                .unwrap_or_default();
            (Ok(value), native)
        }
        Err(failed) => {
            let runtime: Vec<_> = failed
                .diagnostics
                .errors
                .iter()
                .filter_map(|e| match e {
                    FluxError::Runtime { message, .. } => Some(message.clone()),
                    _ => None,
                })
                .collect();
            assert!(
                !runtime.is_empty(),
                "generated program did not compile:\n{src}\n{:?}",
                failed.diagnostics.errors
            );
            (Err(runtime.join("; ")), false)
        }
    }
}

#[test]
fn jit_matches_vm_on_generated_programs() {
    let _guard = jit_test_lock();
    let mut failures = Vec::new();
    let (mut values, mut errors, mut native) = (0, 0, 0);
    for seed in 0..300u64 {
        let src = Gen(seed.wrapping_mul(0x9E37_79B9) + 1).program();
        flux_jit::reset_host_for_tests();
        let (vm, _) = run(&src, ExecutionMode::Vm);
        flux_jit::reset_host_for_tests();
        let (jit, compiled) = run(&src, ExecutionMode::Jit);
        native += u32::from(compiled);
        match &vm {
            Ok(_) => values += 1,
            Err(_) => errors += 1,
        }
        if vm != jit {
            failures.push(format!("seed {seed}: vm {vm:?} != jit {jit:?}\n{src}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Meaningful only if both outcomes occur and the JIT compiles `f` itself.
    assert!(
        values > 50 && errors > 10 && native * 10 >= values * 9,
        "values {values}, errors {errors}, natively compiled {native}"
    );
}

/// Generates reactive programs over several kinds of value: every effect goes
/// through `log`, so the output records what ran and in what order.
struct ReactiveGen(Gen);

impl ReactiveGen {
    fn pick(&mut self, n: u64) -> u64 {
        self.0.pick(n)
    }

    fn int_expr(&mut self, depth: u32, derived: u32) -> String {
        if depth == 0 {
            return match self.pick(6) {
                0 => "a".to_string(),
                1 => "b".to_string(),
                2 => "len(xs)".to_string(),
                3 if derived > 0 => format!("d{}", self.pick(derived as u64)),
                _ => self.pick(6).to_string(),
            };
        }
        let d = depth - 1;
        match self.pick(10) {
            0..=2 => {
                let op = ["+", "-", "*", "%", "/"][self.pick(5) as usize];
                format!(
                    "({} {op} {})",
                    self.int_expr(d, derived),
                    self.int_expr(d, derived)
                )
            }
            3 => format!(
                "(if {} > {} {{ {} }} else {{ {} }})",
                self.int_expr(d, derived),
                self.int_expr(d, derived),
                self.int_expr(d, derived),
                self.int_expr(d, derived)
            ),
            4 => format!(
                "(match e {{ Low(n) => n + {}, High => {} }})",
                self.int_expr(d, derived),
                self.int_expr(d, derived)
            ),
            5 => format!("xs[{}]", self.int_expr(d, derived)),
            6 => format!("twice({})", self.int_expr(d, derived)),
            7 => "read_a()".to_string(),
            8 => "str_len(read_b())".to_string(),
            _ => self.int_expr(0, derived),
        }
    }

    fn handler_stmt(&mut self, depth: u32) -> String {
        match self.pick(9) {
            0..=2 => format!("a = {}", self.int_expr(2, 0)),
            3 => format!("b = {}", self.int_expr(1, 0)),
            4 => format!("xs = [{}, {}]", self.int_expr(1, 0), self.int_expr(1, 0)),
            5 => "s = s + \"x\"".to_string(),
            6 => format!(
                "e = if {} > 2 {{ High }} else {{ Low(n) }}",
                self.int_expr(1, 0)
            ),
            7 if depth > 0 => format!(
                "if {} > 1 {{\n        {}\n    }}",
                self.int_expr(1, 0),
                self.handler_stmt(depth - 1)
            ),
            _ if depth > 0 => format!("emit U({})", self.int_expr(1, 0)),
            _ => "b = n".to_string(),
        }
    }

    fn program(&mut self) -> String {
        let mut src = String::from(
            "import io { println }\n\
             import collections { len }\n\
             import str { str_len }\n\
             enum E {\n    Low(Int),\n    High\n}\n\
             event T(n: Int)\n\
             event U(n: Int)\n\
             let mut a = 1\n\
             let mut b = 2\n\
             let mut xs = [1, 2]\n\
             let mut s = \"s\"\n\
             let mut e = Low(1)\n\
             fn twice(n: Int) -> Int {\n    n * 2\n}\n\
             rfn read_a() -> Int {\n    a + 1\n}\n\
             rfn read_b() -> String {\n    \"{b}\"\n}\n",
        );
        let derived = 1 + self.pick(4) as u32;
        for i in 0..derived {
            let expr = self.int_expr(2, i);
            src.push_str(&format!("let d{i} = {expr}\n"));
        }
        // Handlers without output can run natively; those with it cannot.
        for event in ["T", "U"] {
            let count = 1 + self.pick(2);
            for _ in 0..count {
                let stmts: Vec<String> = (0..1 + self.pick(3))
                    .map(|_| {
                        // `U` handlers do not emit, so chains stay short.
                        let depth = if event == "T" { 1 } else { 0 };
                        self.handler_stmt(depth)
                    })
                    .collect();
                let log = if self.pick(2) == 0 {
                    format!("    println(\"{event} {{n}}\")\n")
                } else {
                    String::new()
                };
                src.push_str(&format!(
                    "on {event} {{ n ->\n{log}    {}\n}}\n",
                    stmts.join("\n    ")
                ));
            }
        }
        for i in 0..1 + self.pick(3) as u32 {
            let target = match self.pick(4) {
                0 => "a".to_string(),
                1 => "s".to_string(),
                _ => format!("d{}", self.pick(derived as u64)),
            };
            let write = match self.pick(3) {
                0 => format!("b = {}\n    ", self.int_expr(1, 0)),
                _ => String::new(),
            };
            src.push_str(&format!(
                "watch {target} {{\n    {write}println(\"w{i} {{{target}}} a={{a}} b={{b}} s={{s}} xs={{xs}}\")\n}}\n"
            ));
        }
        src.push_str("fn main() {\n");
        for _ in 0..1 + self.pick(5) {
            let value = self.pick(8) as i64 - 2;
            let event = if self.pick(3) == 0 { "U" } else { "T" };
            src.push_str(&format!("    emit {event}({value})\n"));
        }
        src.push_str("    println(\"end\")\n}\n");
        src
    }
}

fn run_reactive(dir: &std::path::Path, engine: &str) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["run", engine, "--no-cache", "--quiet", "main.fl"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn flux");
    format!(
        "{:?}\n{}{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// What an `rfn` reads is a dependency of the derived value that calls it,
/// whichever branch the call is in and whichever engine runs the `rfn`.
#[test]
fn derived_if_tracks_reads_through_an_rfn() {
    let _guard = jit_test_lock();
    let dir = tempfile::tempdir().unwrap();
    for (body, want) in [
        (
            "let mut b = \"\"\nrfn read() -> String { \"{a}\" }\nlet d = if c { read() } else { b }\n\
             on T {\n    a = 2\n}\n",
            "Some(0)\n1\n2\n",
        ),
        (
            "let mut b = 0\nrfn read() -> Int { a }\nlet d = if c { read() } else { b }\n\
             on T {\n    a = 2\n}\n",
            "Some(0)\n1\n2\n",
        ),
        (
            "let mut b = 0\nrfn left() -> String { \"{a}\" }\nrfn right() -> String { \"{b}\" }\n\
             let d = if c { left() } else { right() }\nevent U\n\
             on T {\n    a = 2\n    emit U\n    println(\"after U\")\n}\non U {\n    b = 5\n}\n",
            "Some(0)\n1\nafter U\n2\n",
        ),
    ] {
        std::fs::write(
            dir.path().join("main.fl"),
            format!(
                "import io {{ println }}\nlet mut a = 1\nlet mut c = true\nevent T\n{body}\
                 watch d {{ println(d) }}\nfn main() {{\n    emit T\n}}\n"
            ),
        )
        .unwrap();
        let vm = run_reactive(dir.path(), "--vm");
        assert!(vm.ends_with(want) || vm.starts_with(want), "{vm}");
        assert_eq!(vm, run_reactive(dir.path(), "--jit"));
    }
}

/// A failed recomputation ends the event: later handlers do not start.
#[test]
fn failed_settling_stops_later_handlers() {
    let _guard = jit_test_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "import io { println }\n\
         event T\n\
         let mut a = 1\n\
         let d = 10 / a\n\
         on T {\n    a = 0\n}\n\
         on T {\n    println(\"second handler ran\")\n}\n\
         watch d { println(\"d={d}\") }\n\
         fn main() {\n    emit T\n}\n",
    )
    .unwrap();
    let vm = run_reactive(dir.path(), "--vm");
    assert!(!vm.contains("second handler ran"), "{vm}");
    assert_eq!(vm, run_reactive(dir.path(), "--jit"));
}

/// A native failure replays on the VM, so the error, its trace, and writes
/// made before it are the VM's.
#[test]
fn runtime_error_reports_the_same_trace_on_both_engines() {
    let _guard = jit_test_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "import io { println }\n\
         event T(n: Int)\n\
         let mut w = 0\n\
         fn div(a: Int, b: Int) -> Int {\n    let c = a + 1\n    c / b\n}\n\
         fn outer(b: Int) -> Int {\n    if b == 0 {\n        div(1, b)\n    } else {\n        0\n    }\n}\n\
         on T { n ->\n    w = n\n    w = outer(n)\n}\n\
         watch w { println(\"w={w}\") }\n\
         fn main() {\n    emit T(0)\n}\n",
    )
    .unwrap();
    let vm = run_reactive(dir.path(), "--vm");
    let root = dir.path().canonicalize().unwrap();
    let shown = vm.replace(&format!("{}/", root.display()), "");
    assert!(
        shown.contains("integer division by zero in `on T`\n  at div (main.fl:6:5)\n  at outer (main.fl:10:9)\n  at on T (main.fl:17:5)\n  at main (main.fl:21:5)"),
        "{shown}"
    );
    assert_eq!(vm, run_reactive(dir.path(), "--jit"));
}

/// A derived `if` depends only on the branch it took, so a write to the other
/// branch's source must not rerun it while an outer handler's write is pending.
#[test]
fn untaken_branch_write_does_not_settle_a_derived_if() {
    let _guard = jit_test_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        "import io { println }\n\
         let mut a = 1\n\
         let mut b = 0\n\
         let mut c = true\n\
         let d = if c { a } else { b }\n\
         event T(n: Int)\n\
         event U(n: Int)\n\
         on T { n ->\n    a = 2\n    emit U(n)\n    println(\"after U\")\n}\n\
         on U { n ->\n    b = n\n}\n\
         watch d { println(\"d={d}\") }\n\
         fn main() {\n    emit T(5)\n}\n",
    )
    .unwrap();
    let want = "Some(0)\nd=1\nafter U\nd=2\n";
    assert_eq!(run_reactive(dir.path(), "--vm"), want);
    assert_eq!(run_reactive(dir.path(), "--jit"), want);
}

#[test]
fn jit_matches_vm_on_generated_reactive_programs() {
    let _guard = jit_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.fl");
    let mut failures = Vec::new();
    let (mut finished, mut failed, mut native_handlers, mut invalid) = (0, 0, 0, 0);
    let seeds = std::env::var("FLUX_DIFF_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200u64);
    for seed in 0..seeds {
        let src = ReactiveGen(Gen(seed.wrapping_mul(0x9E37_79B9) + 7)).program();
        std::fs::write(&path, &src).unwrap();
        let vm = run_reactive(dir.path(), "--vm");
        // A program the generator got wrong; warnings are fine.
        if vm
            .lines()
            .any(|l| l.starts_with("flux::E") && l != "flux::E015" && l != "flux::E018")
        {
            invalid += 1;
            continue;
        }
        let jit = run_reactive(dir.path(), "--jit");
        match vm.starts_with("Some(0)") {
            true => finished += 1,
            false => failed += 1,
        }
        if vm != jit {
            failures.push(format!("seed {seed}\n{src}\n--- VM\n{vm}\n--- JIT\n{jit}"));
        }
        // `--verbose` lists what fell back to the VM.
        let stats = std::process::Command::new(env!("CARGO_BIN_EXE_flux"))
            .args(["run", "--jit", "--verbose", "--no-cache", "main.fl"])
            .current_dir(dir.path())
            .stdin(std::process::Stdio::null())
            .output()
            .expect("spawn flux");
        let stats = String::from_utf8_lossy(&stats.stdout).into_owned();
        native_handlers += u32::from(stats.contains("JIT stats") && !stats.contains("__on_"));
    }
    let shown = failures.len().min(3);
    assert!(
        failures.is_empty(),
        "{} mismatches of {}:\n{}",
        failures.len(),
        finished + failed,
        failures[..shown].join("\n=====\n")
    );
    assert!(
        finished > seeds / 10 && failed > seeds / 20 && invalid < seeds / 2,
        "finished {finished}, failed {failed}, invalid {invalid}, native {native_handlers}"
    );
}
