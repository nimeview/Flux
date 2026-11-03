use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_syntax::parse_program;

use super::super::{state::SCOPE_LOCAL_COPIES, typecheck_with_context, TypeCheckContext};

fn check_work(source: &str, budget: usize) -> usize {
    // Match the compiler driver's stack allowance rather than the test harness's
    // small default stack; the budget still stops a work regression early.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(flux_syntax::COMPILER_STACK)
            .spawn_scoped(scope, || check_work_on_compiler_stack(source, budget))
            .unwrap()
            .join()
            .unwrap()
    })
}

fn check_work_on_compiler_stack(source: &str, budget: usize) -> usize {
    let mut diagnostics = DiagnosticBag::new();
    let ast = parse_program(SourceId(0), source, &mut diagnostics).expect("parse");
    let hir = lower_program(&ast, &mut diagnostics).expect("lower");
    let ctx = TypeCheckContext::from_program(&hir);
    ctx.closure_check_limit.set(Some(budget));
    SCOPE_LOCAL_COPIES.with(|n| n.set(0));
    assert!(
        typecheck_with_context(&hir, SourceId(0), &mut diagnostics, &ctx).is_some(),
        "{:?}",
        diagnostics.errors
    );
    assert!(ctx.closure_body_checks.get() <= budget);
    SCOPE_LOCAL_COPIES.with(|n| n.get())
}

#[test]
fn nested_closure_body_checks_are_linear_in_depth() {
    let depth = 24;
    let mut expr = "1".to_owned();
    for i in 0..depth {
        expr = format!("len(map(xs, |x{i}| {expr}))");
    }
    let source = format!(
        "import collections {{ map, len }}\nfn main() {{ let xs = [1]\nlet n = {expr}\n0 }}\n"
    );
    check_work(&source, 4 * depth);
}

#[test]
fn unrelated_locals_are_not_copied_into_each_callback() {
    let width = 256;
    for annotation in ["", ": It"] {
        let mut source = "import collections { map, len }\nimport io { println }\nstruct It { v: Int }\nfn main() { let xs = [It { v: 1 }]\n".to_owned();
        for i in 0..width {
            source.push_str(&format!(
                "let a{i} = map(xs, |i{annotation}| i.v + {i})\nprintln(\"{{len(a{i})}}\")\n"
            ));
        }
        source.push_str("}\n");
        let copies = check_work(&source, 4 * width);
        assert!(
            copies <= 8 * width,
            "copied {copies} locals for {width} callbacks ({annotation})"
        );
    }
}
