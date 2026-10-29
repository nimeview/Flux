use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_hir::lower_program;
use flux_ir::{Codegen, IrCodegen};
use flux_runtime::{Executor, RuntimeValue};
use flux_syntax::parse_program;
use flux_types::typecheck_program;

fn compile(src: &str) -> Executor {
    let mut diags = DiagnosticBag::new();
    let source_id = SourceId(0);
    let ast = parse_program(source_id, src, &mut diags).expect("parse");
    let hir = lower_program(&ast, &mut diags).expect("hir");
    let typed = typecheck_program(&hir, source_id, &mut diags).expect("types");
    let _ = typed;
    let ir = IrCodegen.emit(&hir, source_id, &mut diags).expect("ir");
    Executor::new(ir).expect("valid program")
}

#[test]
fn dynamic_if_deps_skip_untaken_branch() {
    let mut ex = compile(
        "let mut flag = true\n\
         let x = 1\n\
         let mut y = 100\n\
         let z = if flag { x } else { y }\n",
    );

    assert_eq!(ex.get_value("z"), Some(RuntimeValue::int_i64(1)));

    // flag=true → z tracks x, not y
    ex.set_source("y", RuntimeValue::int_i64(200));
    assert_eq!(ex.get_value("z"), Some(RuntimeValue::int_i64(1)));

    // flip branch → z now tracks y
    ex.set_source("flag", RuntimeValue::Bool(false));
    assert_eq!(ex.get_value("z"), Some(RuntimeValue::int_i64(200)));

    // flag=false → x changes should not invalidate z
    ex.set_source("x", RuntimeValue::int_i64(5));
    assert_eq!(ex.get_value("z"), Some(RuntimeValue::int_i64(200)));

    ex.set_source("y", RuntimeValue::int_i64(300));
    assert_eq!(ex.get_value("z"), Some(RuntimeValue::int_i64(300)));
}

#[test]
fn game_score_lead_with_watch_recomputes() {
    let mut ex = compile(
        "let mut home = 0\n\
         let mut away = 0\n\
         let lead = if home > away { home - away } else { if away > home { away - home } else { 0 } }\n\
         let mut possessions = 0\n\
         import collections\n\
         watch {\n\
         }\n\
         event ScoreHome\n\
         event ScoreAway\n\
         on ScoreHome { home = home + 3 }\n\
         on ScoreAway { away = away + 3 }\n",
    );

    assert_eq!(ex.get_number("lead"), Some(0.0));
    ex.emit("ScoreHome", RuntimeValue::Unit);
    ex.emit("ScoreHome", RuntimeValue::Unit);
    ex.emit("ScoreAway", RuntimeValue::Unit);
    assert_eq!(ex.get_number("lead"), Some(3.0));
}

#[test]
fn lazy_if_only_evaluates_taken_branch() {
    let ex = compile(
        "let zero = 0\n\
         let mut flag = false\n\
         let ok = if flag { 1 / zero } else { 42 }\n",
    );
    assert_eq!(ex.get_value("ok"), Some(RuntimeValue::int_i64(42)));
}
