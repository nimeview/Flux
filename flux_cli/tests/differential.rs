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
