use flux_cli::pipeline::{compile, source_from};
use flux_runtime::Executor;

fn run(src: &str) -> Executor {
    let source = source_from("test.fl", src);
    compile(source).expect("compile should succeed").executor
}

#[test]
fn derived_multiplication() {
    let ex = run("let a = 5\nlet b = 2\nlet c = a * b\n");
    assert_eq!(ex.get_number("a"), Some(5.0));
    assert_eq!(ex.get_number("b"), Some(2.0));
    assert_eq!(ex.get_number("c"), Some(10.0));
}

#[test]
fn let_at_module_level_is_source() {
    let ex = run("let x = 42\nlet y = x + 1\n");
    assert_eq!(ex.get_number("x"), Some(42.0));
    assert_eq!(ex.get_number("y"), Some(43.0));
}

#[test]
fn if_expr_picks_correct_branch() {
    let ex = run("let a = 5\nlet b = 2\nlet biggest = if a > b { a } else { b }\n");
    assert_eq!(ex.get_number("biggest"), Some(5.0));
}

#[test]
fn propagation_after_source_update() {
    let mut ex = run("let mut a = 5\nlet c = a * 2\n");
    assert_eq!(ex.get_number("c"), Some(10.0));
    ex.set_source("a", flux_runtime::RuntimeValue::int_i64(10));
    assert_eq!(ex.get_number("c"), Some(20.0));
}

#[test]
fn fn_call_in_derived_binding() {
    let ex = run("fn double(x: Int) -> Int {\n  x * 2\n}\nlet a = 3\nlet b = double(a)\n");
    assert_eq!(ex.get_number("b"), Some(6.0));
}

#[test]
fn compile_fails_on_reactive_in_pure_fn() {
    let source = source_from("bad.fl", "let a = 5\nfn f() { let x = a + 1 }\n");
    assert!(compile(source).is_err());
}

#[cfg(unix)]
#[test]
fn read_line_reads_from_stdin() {
    use std::process::Command;

    let program = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../programs/ok/io/read_line.fl"
    );
    let manifest_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let flux_bin = env!("CARGO_BIN_EXE_flux");
    let script = format!("echo hello | {flux_bin} run {program}");

    let output = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(manifest_dir)
        .output()
        .expect("run read_line via shell pipe");

    assert!(
        output.status.success(),
        "flux run failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello"),
        "expected read_line output in stdout, got: {stdout}"
    );
}
